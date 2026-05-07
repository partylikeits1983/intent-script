# Plan: Add Base chain support to compiler & UI (additive)

> **Note on plan location:** project-memory says durable plans live in `intent-script/plans/`. The harness limits this planning phase to the foamy-lemur path; once approved, copy this file to `intent-script/plans/add-base-chain-support.md` before implementation begins.

## Context

intentOS currently compiles intents only for Ethereum L1 (forked locally as Anvil chain id 31337). The compiler hardcodes Balancer V2 as the only flashloan provider for `long` / `short` / `close_position` sugar (`intent-script/crates/intent-script/src/compiler/leverage.rs:45`), and the IntentRouter contract (`intent-script/contracts/src/IntentRouter.sol:47`) has `BALANCER_VAULT` baked in as a constant. The UI mirrors this — `lib/config/protocols-anvil.json` only lists Ethereum-mainnet addresses, the WASM compiler wrapper at `intentOS-ui/lib/intent-compiler.ts:33-40` returns null for non-anvil/sepolia networks, and the leverage `via` enum in `intentOS-ui/lib/intent-tool-schema.ts:382` is locked to `["balancer"]`.

We want to add Base (chain id 8453) as a first-class compile target without touching existing L1 behavior:

- **Default fork after this change**: Base (`make start` boots Base; L1 is opt-in via `make start-l1`).
- **Base supports**: Uniswap V3, Aave V3 (lending + flashloan). **No Lido, no Balancer** on Base.
- **Flashloan policy**: keep Balancer as default on L1; **also** add Aave flashloan as an option on L1; Aave is the **only** flashloan provider on Base (incurs the standard ~5 bps Aave premium).
- **L1 tests stay green** — every existing integration test in `tests/integration.rs` continues to pass untouched.

The change is intentionally additive: no existing IR variant, config file, deployment, or test is renamed or removed.

---

## Design summary

### 1. Pluggable flashloan provider (compiler IR)

Currently the compiler emits `ResolvedStep::BalancerFlashloan { vault, tokens, amounts, inner_steps }` (`intent-script/crates/intent-script/src/ir/canonical.rs:351`). We add a sibling variant:

```rust
ResolvedStep::AaveFlashloan {
    pool: Address,
    asset: Address,
    amount: U256,
    premium_bps: u16,        // queried from registry; 5 on mainnet/Base today
    inner_steps: Vec<ResolvedStep>,
}
```

Aave V3 `flashLoanSimple` is single-asset, so the IR is single-asset (matches the leverage sugar's actual shape — leverage only ever uses one asset). Multi-asset flashloans stay Balancer-only for now.

`compiler/leverage.rs::expand_leverage` now selects the provider:

- `via == "balancer"` → `BalancerFlashloan` (existing path, unchanged math).
- `via == "aave"` → `AaveFlashloan`. The inner `UniswapV3Swap.amount_out_minimum` is increased from `flashloan_amount` to `flashloan_amount + premium`. The inner `borrow_amount` is recomputed from `flashloan_amount + premium` (premium = `flashloan_amount * premium_bps / 10_000`) so the borrowed leg has enough output to cover repayment + premium under slippage.
- `via` omitted → use the chain's default: `balancer` if it exists in the registry, otherwise `aave`. (For Base, no Balancer → defaults to Aave; for L1, defaults to Balancer — back-compat.)
- Anything else → existing `UnsupportedStep` error with the now-truthful list `[balancer, aave]`.

Apply the same logic in `expand_close` (lines 335+).

### 2. Aave flashloan adapter

New file: **`intent-script/crates/intent-script/src/adapters/aave_flashloan.rs`**, modelled on `balancer.rs`. Lowers `AaveFlashloan` → one outer `pool.flashLoanSimple(receiver, asset, amount, params, 0)` call where `params` is the ABI-encoded `Call[]` inner pipeline. Recipient = IntentRouter from the registry.

`adapters/mod.rs:47` dispatch updated to route both flashloan variants. The recursive `lower_step` function gets one new arm.

`enrich.rs` (`intent-script/crates/intent-script/src/compiler/enrich.rs:694-743`): the existing balancer-aware enrichment path generalizes — both providers transfer tokens to the receiver before the callback, so `inner_steps` enrichment stays the same. We add a parallel `match` arm for `AaveFlashloan` so enrichment recurses into its inner steps too.

`error.rs:841-853` flashloan validation messages updated to reference both providers without changing existing variant names.

### 3. IntentRouter contract — additive Aave callback (deployed on **both** chains)

Edit `intent-script/contracts/src/IntentRouter.sol`:

- Convert `BALANCER_VAULT` from a `constant` to an `immutable` set in the constructor (default current value on L1).
- Add `address public immutable AAVE_POOL` constructor parameter (set per-chain at deploy time; `address(0)` means "Aave callback disabled on this deployment").
- Add `function executeOperation(address asset, uint256 amount, uint256 premium, address initiator, bytes calldata params) external returns (bool)`:
  - `require(AAVE_POOL != address(0) && msg.sender == AAVE_POOL, "not aave pool");`
  - Same `FLASHLOAN_GUARD_SLOT` arm/clear pattern as `receiveFlashLoan` (line 367).
  - Decode `params` as `Call[]`, run inner pipeline.
  - Repayment: `IERC20(asset).approve(AAVE_POOL, amount + premium);` then `return true;`. (Aave pulls via `transferFrom`, unlike Balancer's balance-check pattern.)
- Update the flashloan sentinel arming in `_executeCalls` (line 311) to also recognise calls whose target is `AAVE_POOL` and whose selector matches Aave's `flashLoanSimple` selector. Add `AAVE_FLASHLOAN_SIMPLE_SELECTOR` constant (`0x42b0b77c`).
- `setAllowedTargets` allowlist seeding stays the same; the deploy script seeds whichever provider exists for that chain.

This makes the *same* Solidity codebase usable on both chains with different constructor args. Each chain's deployment becomes:

| Chain | balancerVault arg                           | aavePool arg                                 |
|-------|----------------------------------------------|----------------------------------------------|
| L1    | 0xBA12222222228d8Ba445958a75a0704d566BF2C8   | 0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2   |
| Base  | 0x0000000000000000000000000000000000000000   | 0xA238Dd80C259a72e81d7e4664a9801593F98d1c5   |

L1 keeps Balancer as primary and gains Aave as a second option; Base only has Aave.

> **Bytecode caveat:** Adding constructor params changes the deployed bytecode but **not** the CREATE address (CREATE address depends on deployer + nonce only). The existing deterministic `0x8464135c8F25Da09e49BC8782676a84730C318bC` address on the L1 fork stays the same. The `start-anvil.sh` deploy command needs the new ABI-encoded args appended to the bytecode — small edit.

### 4. Config files (new)

**`intent-script/config/protocols/base.json`** — Aave V3, Uniswap V3, IntentRouter only. Across SpokePool on Base if listed, otherwise omit (bridge is non-blocking). No Lido. No Balancer.

**`intent-script/config/assets/base.json`** — WETH, USDC (native Circle), USDbC (legacy bridged), cbETH, cbBTC.

Mirror copies in the UI:

- **`intentOS-ui/lib/config/protocols-base.json`**
- **`intentOS-ui/lib/config/assets-base.json`**

Address values to populate (verify each from canonical sources before commit — Aave docs, Uniswap docs, basescan):

| Symbol / Contract                  | Address                                      |
|-----------------------------------|----------------------------------------------|
| WETH                               | 0x4200000000000000000000000000000000000006   |
| USDC (native)                      | 0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913   |
| USDbC (bridged)                    | 0xd9aAEc86B65D86f6A7B5B1b0c42FFA531710b6CA   |
| cbETH                              | 0x2Ae3F1Ec7F1F5012CFEab0185bfc7aa3cf0DEc22   |
| cbBTC                              | 0xcbB7C0000aB88B473b1f5aFd9ef808440eed33Bf   |
| Aave V3 Pool                       | 0xA238Dd80C259a72e81d7e4664a9801593F98d1c5   |
| Aave V3 PoolAddressesProvider      | 0xe20fCBdBfFC4Dd138cE8b2E6FBb6CB49777ad64D   |
| Uniswap V3 SwapRouter02            | 0x2626664c2603336E57B271c5C0b26F421741e481   |
| Uniswap V3 QuoterV2                | 0x3d4e44Eb1374240CE5F1B871ab261CD16335B76a   |
| Uniswap V3 Factory                 | 0x33128a8fC17869897dcE68Ed026d694621f6FDfD   |
| Uniswap V3 NonfungiblePositionMgr  | 0x03a520b32C04BF3bEEf7BEb72E919cf822Ed34f1   |
| IntentRouter (Base fork)           | (deterministic; CREATE from deployer nonce 0 — same as L1 fork = `0x8464135c8F25Da09e49BC8782676a84730C318bC`) |

`aave.ltv_bps` table for Base must be re-derived from Aave's Base markets (LTVs differ slightly from L1 — e.g. WETH on Base ~80%, cbETH ~75%). `variable_debt_tokens` likewise queried from Aave's Base deployment. Add a new top-level field `aave.flashloan_premium_bps: 5` so the compiler reads it instead of hardcoding.

### 5. New fork script

**`intent-script/scripts/start-anvil-base.sh`** — clone of `start-anvil.sh` with these differences:

- `ETH_RPC_URL` defaults to `https://mainnet.base.org` (env-overridable; recommend Alchemy for speed).
- `--chain-id 8453` (real Base id, per user choice).
- Mints 100k USDC native (slot lookup TBD — USDC on Base may proxy; if the impl slot differs, fall back to impersonating a USDC whale via `anvil_impersonateAccount` and `cast send`).
- Mints 10k cbETH (verify storage slot or impersonate a whale).
- Funds dev accounts with native ETH via `anvil_setBalance`.
- Deploys IntentRouter with `(balancerVault=0x0, aavePool=0xA238Dd80C259a72e81d7e4664a9801593F98d1c5)`.
- Seeds allowlist with: WETH, USDC, USDbC, cbETH, cbBTC, Aave Pool, Aave variableDebtTokens, Uniswap SwapRouter02 + Quoter + PositionManager, IntentRouter itself.

Keep `start-anvil.sh` untouched (it is the L1 fork). Update `intent-script/makefile`:

- `make start` → `./scripts/start-anvil-base.sh` (was: L1).
- `make start-l1` → `./scripts/start-anvil.sh` (new alias for the existing behavior).

### 6. UI updates

- **`intentOS-ui/lib/config/index.ts:64-92`** — wire the new JSONs into `PROTOCOLS_BY_NETWORK` and `ASSETS_BY_NETWORK` so `network === "base"` no longer returns null.
- **`intentOS-ui/lib/intent-compiler.ts:33-40`** — already does `import(./config/protocols-${network}.json)`; once the files exist, no code change needed.
- **`intentOS-ui/lib/intent-tool-schema.ts`** — extend `via` enums (`flashloan` line 382, `long` 425, `short` 448, `close_position` 467) from `["balancer"]` to `["balancer", "aave"]`. Update the surrounding tool description so the LLM knows "On Base, only `aave` is available; on L1, default is `balancer`, but `aave` is supported (~5 bps fee)."
- **`intentOS-ui/lib/wagmi-config.ts:35-44`** — when `NEXT_PUBLIC_USE_LOCAL_FORK=true`:
  - Default chain becomes Base at id 8453, RPC `http://127.0.0.1:8545`.
  - Add an L1 fork option at id 31337 → `http://127.0.0.1:8546` (only when `NEXT_PUBLIC_LOCAL_L1_FORK=true`).
  - The `transports` map must route 8453 to localhost (override viem's default Base RPC) when in fork mode.
- **`intentOS-ui/components/local-fork-banner.tsx:20-85`** — generalize copy: detect expected chain (Base 8453 or L1 31337), show "intentOS Base Fork" or "intentOS L1 Fork" accordingly.
- **`intentOS-ui/lib/simulate-transaction.ts:73-102`** — `clientForChain()` switch: chain 8453 + fork mode → `http://127.0.0.1:8545`; chain 31337 → `http://127.0.0.1:8546`.
- **`intentOS-ui/lib/fetch-allowances-json.ts:106`** — replace hardcoded `http://127.0.0.1:8545` with chain-aware lookup using the same map.
- **`hooks/use-active-network.ts:5-26`** — already maps 8453 → "base"; verify and leave intact.

### 7. Tests

**Keep all 122 L1 tests intact.** `tests/integration.rs` `load_config()` (lines 21–27) stays as-is, hardcoded to anvil.

Add a new sibling test file: **`intent-script/crates/intent-script/tests/integration_base.rs`** that defines a parallel `load_base_config()` reading `config/assets/base.json` + `config/protocols/base.json`. Add coverage for:

- Plain swap WETH→USDC on Base (Uniswap V3).
- `deposit` into Aave on Base.
- `borrow` against Aave collateral.
- `long WETH leverage=2 collateral=USDC borrow=USDC` (sanity: rejects same-asset).
- `long WETH leverage=2 collateral=WETH borrow=USDC` via implicit Aave flashloan — assert the compiled IR is `AaveFlashloan`, the inner swap's `amount_out_minimum == flashloan_amount + premium`, and the borrow_amount is sized for `flashloan_amount + premium`.
- `long ... via="balancer"` on Base → expect a clear `UnsupportedProvider` error.
- L1 sanity: a single test using the existing `anvil` config that asks for `via="aave"` and asserts `AaveFlashloan` IR is produced (proves the new path doesn't regress L1).

E2E / UI evals: add one Base leverage eval in `intentOS-ui/evals/chat-intent/` that mirrors an existing leverage case but with `network: "base"`.

---

## Files to create

```
intent-script/config/protocols/base.json
intent-script/config/assets/base.json
intent-script/scripts/start-anvil-base.sh
intent-script/crates/intent-script/src/adapters/aave_flashloan.rs
intent-script/crates/intent-script/tests/integration_base.rs
intentOS-ui/lib/config/protocols-base.json
intentOS-ui/lib/config/assets-base.json
```

## Files to modify

```
intent-script/contracts/src/IntentRouter.sol           # immutable BALANCER_VAULT, new AAVE_POOL + executeOperation
intent-script/crates/intent-script/src/ir/canonical.rs # + AaveFlashloan variant
intent-script/crates/intent-script/src/compiler/leverage.rs   # provider selection in expand_leverage + expand_close
intent-script/crates/intent-script/src/compiler/enrich.rs     # AaveFlashloan enrichment arm
intent-script/crates/intent-script/src/adapters/mod.rs        # dispatch new variant
intent-script/crates/intent-script/src/error.rs        # message updates (no variant churn)
intent-script/scripts/start-anvil.sh                   # constructor args for IntentRouter (balancerVault, aavePool)
intent-script/makefile                                 # make start → base; make start-l1 → existing
intentOS-ui/lib/config/index.ts                        # expose base in PROTOCOLS_BY_NETWORK / ASSETS_BY_NETWORK
intentOS-ui/lib/intent-tool-schema.ts                  # via enums + LLM description
intentOS-ui/lib/wagmi-config.ts                        # base fork on :8545, l1 fork on :8546 (opt-in)
intentOS-ui/lib/simulate-transaction.ts                # clientForChain map
intentOS-ui/lib/fetch-allowances-json.ts               # chain-aware RPC URL
intentOS-ui/components/local-fork-banner.tsx           # base/l1 fork detection + copy
```

Untouched (intentional):
- `intent-script/crates/intent-script/src/adapters/balancer.rs`
- `intent-script/crates/intent-script/src/adapters/lido.rs`
- `intent-script/config/protocols/ethereum.json`, `intent-script/config/assets/ethereum.json`, `intent-script/config/assets/anvil.json`, `intent-script/config/protocols/anvil.json`
- All existing UI Lido / staking flows
- `tests/integration.rs` (every L1 test)

---

## Critical existing utilities to reuse

- `RegistryContext::router_address()` (`crates/intent-script/src/registry/loader.rs`) — already chain-agnostic; the AaveFlashloan adapter uses it just like Balancer does.
- `adapters::lower_step` recursion in `adapters/mod.rs` — reuse for inner pipeline lowering.
- Existing `parse_amount`, `parse_slippage`, `parse_leverage` helpers in `compiler/leverage.rs` — no change needed.
- `enrich.rs` ERC20TransferFrom auto-insert logic for inner pipeline — reuse via the new match arm.
- `intentOS-ui/lib/intent-compiler.ts` dynamic import pattern — already supports any network name; no signature change.
- `LocalForkBanner` `wallet_addEthereumChain` flow (`local-fork-banner.tsx:53`) — extend with a Base config object rather than rewriting.

---

## Verification

End-to-end smoke (after implementation):

1. **Compile-only sanity (no fork needed)**:
   - `cd intent-script && cargo test -p intent-script --test integration` → all 122 L1 tests pass.
   - `cargo test -p intent-script --test integration_base` → new Base tests pass.
   - `cargo build -p intent-script-wasm --target wasm32-unknown-unknown` succeeds.

2. **Contract tests**:
   - `cd intent-script/contracts && forge test` — existing Balancer-flow tests pass; new `executeOperation` tests assert: revert when `msg.sender != AAVE_POOL`, revert when guard not armed, success path approves `amount + premium` and returns true.

3. **Base fork live test**:
   - Start fork: `make start` (boots `start-anvil-base.sh`, deploys IntentRouter on Base fork).
   - In another terminal: run `intent-script/scripts/check-balances.ts` against `RPC_URL=http://127.0.0.1:8545` — confirms USDC/cbETH funded on dev account #0.
   - From the UI (`pnpm dev` in `intentOS-ui` with `NEXT_PUBLIC_USE_LOCAL_FORK=true`): connect MetaMask to Base fork (chain 8453, RPC http://127.0.0.1:8545), confirm LocalForkBanner shows "intentOS Base Fork".
   - Build a leverage intent in the chat UI: "long WETH 2x using USDC collateral on Base". Confirm the compiled tx targets Aave Pool's `flashLoanSimple` selector and the inner swap's min_out covers premium. Submit and confirm execution succeeds against the fork.
   - Build a swap intent: "swap 0.1 WETH for USDC on Base". Confirm Uniswap V3 SwapRouter02 call lands in the compiled output and executes successfully.
   - Build an Aave deposit and borrow on Base; confirm both succeed.
   - Attempt a Lido stake intent on Base → expect a clear "Lido is not configured for network base" compile error (driven by `protocols-base.json` not having a `lido` key — existing `UnknownProtocol` error path).

4. **L1 fork regression test**:
   - `make start-l1` → existing behavior.
   - Run an L1 leverage intent with `via: balancer` (default) — must work exactly as before.
   - Run an L1 leverage intent with `via: aave` — must produce an `AaveFlashloan` and execute against the same IntentRouter (now redeployed with both vault + pool wired).

5. **UI evals**:
   - `pnpm test:evals` — existing leverage evals pass.
   - New Base leverage eval passes (LLM correctly picks `via: aave` when network is base).

---

## Out of scope (explicit)

- Multi-asset Aave flashloans (V3 supports it, but leverage sugar only ever needs one asset).
- Bridging via Across between L1 and Base — the `bridge` primitive is mentioned in chains.json but not part of this task.
- Native USDC ↔ USDbC swap helpers on Base — leave to a separate task if needed.
- Removing Balancer references from the codebase — strictly additive.
- Sepolia, Arbitrum, Optimism, Polygon — they are listed in `chains.json` and `wagmi-config.ts` but not configured here.
