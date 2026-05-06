# Plan: Add Aave/Balancer/Uniswap integration tests to intent-script

> **Note on plan location**: per project convention these would normally live in
> `intent-script/plans/`, but plan-mode lets me edit only this file. Move/copy
> the contents to `intent-script/plans/integration-tests-aave-leverage.md` when
> you're ready to keep it durable.

## Context

The intent-script repo has solid fork-test coverage for individual primitives
(`test_fork_aaveDepositUSDC`, `test_fork_swapUSDC_WETH`, `test_fork_complexDefi_executeDirect`),
but the existing leverage fixtures (`long_eth_4x_batch.bin`, `short_eth_3x.bin`)
aren't wired to executable fork tests. We want four user-flow integration tests
that each:

1. Author the intent in raw JSON DSL (in `crates/intent-script/examples/`)
2. Compile JSON → calldata via the Rust compiler (`make generate-fixtures`)
3. Execute the calldata against a forked mainnet IntentRouter and assert balances

This validates the JSON DSL → compiler → router pipeline for realistic
deposit/borrow/leverage flows and gives us a place to keep adding scenarios.

## Decisions (from clarifying Qs)

- **Pattern**: existing precompiled-fixtures pattern — JSON in `examples/`, `make generate-fixtures` produces `test/fixtures/<name>.txt`, Solidity test reads via `_readCalldata(name)`. No FFI, no inline JSON in `.t.sol`.
- **Test 2 collateral**: substitute wstETH for WETH (Aave V3 set WETH reserve LTV → 0 on mainnet post-2024).
- **Test 3 direction**: flip to true 3x long (WETH collateral + USDC debt).
- **DSL form for tests 3 & 4**: hand-rolled (explicit `flashloan`/`deposit`/`borrow`/`swap` steps), not `long`/`short` sugar.

## Test list

### Test 1 — Aave: deposit 10k USDT, borrow 1 WETH

`crates/intent-script/examples/aave_deposit_usdt_borrow_weth.json`

```json
{
  "network": "ethereum",
  "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
  "current_timestamp": 1714000000,
  "steps": [
    { "deposit": { "asset": "USDT", "amount": "10000", "into": "aave" } },
    { "borrow":  { "asset": "WETH", "amount": "1.0",   "from": "aave" } }
  ]
}
```

Solidity test `test_fork_aaveDepositUSDT_borrowWETH`:
- `_dealERC20(USDT, user, 10000e6)`, user approves router for 10000e6 USDT
- `_approveDelegation(VDEBT_WETH, user, ROUTER_ADDR, 1e18)`
- Read `aave_deposit_usdt_borrow_weth` fixture, execute via router
- Assert: `aUSDT(user) > 0`, `WETH(user) ≥ 1e18`, USDT spent, router cleared of USDT/WETH

**Risk to verify at implementation**: USDT on Aave V3 mainnet historically had
LTV/borrowableInIsolation restrictions. If borrow reverts, fall back to deposit
USDC (10k) → borrow 1 WETH. Confirm by checking `getReserveConfigurationData`
on the fork, or just attempt and adapt.

### Test 2 — Aave: deposit 5 wstETH, borrow 1k USDC

`crates/intent-script/examples/aave_deposit_wsteth_borrow_usdc.json`

```json
{
  "network": "ethereum",
  "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
  "current_timestamp": 1714000000,
  "steps": [
    { "deposit": { "asset": "wstETH", "amount": "5.0",  "into": "aave" } },
    { "borrow":  { "asset": "USDC",   "amount": "1000", "from": "aave" } }
  ]
}
```

Solidity test `test_fork_aaveDepositWSTETH_borrowUSDC`:
- `_dealERC20(WSTETH, user, 5e18)`, approve router
- `_approveDelegation(VDEBT_USDC, user, ROUTER_ADDR, 1000e6)`
- Read fixture, execute, assert `aWSTETH > 0`, `USDC(user) ≥ 1000e6`, router cleared.

(Mirrors the `complex_defi` collateral choice — wstETH LTV ≈ 78.5%, no LTV-0 issue.)

### Test 3 — 3x leveraged long ETH via Balancer flashloan (hand-rolled)

`crates/intent-script/examples/long_eth_3x_balancer.json`

```json
{
  "network": "ethereum",
  "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
  "current_timestamp": 1714000000,
  "steps": [
    {
      "flashloan": {
        "via": "balancer",
        "assets": [{ "asset": "USDC", "amount": "20000" }],
        "then": [
          { "swap":    { "from": "USDC", "amount": "30000", "to": "WETH", "min_amount_out": "9.0" } },
          { "deposit": { "asset": "WETH", "amount": "9.0",  "into": "aave" } },
          { "borrow":  { "asset": "USDC", "amount": "20000","from": "aave" } }
        ]
      }
    }
  ]
}
```

Pipeline: user has 10k USDC of their own + flashloan 20k USDC = 30k USDC total
→ swap all 30k USDC → ~10 WETH (slippage-protected at min 9.0) → deposit 9.0 WETH
to Aave → borrow 20k USDC → router auto-repays 20k flashloan to Balancer.
Net position: ~9 WETH Aave collateral + 20k USDC Aave debt = 3x ETH-long exposure
on user's 10k USDC stake.

Solidity test `test_fork_long_eth_3x_balancer`:
- `_dealERC20(USDC, user, 10000e6)`, approve router
- `_approveDelegation(VDEBT_USDC, user, ROUTER_ADDR, 20000e6)`
- Read fixture, execute, assert `aWETH(user) ≥ 9e18`, USDC debt token balance ≈ 20000e6, USDC drained from user wallet, router cleared.

**Risk to verify**: does the compiler correctly emit a 10k USDC `transferFrom`
from user → router when the inner `swap` consumes 30k but the flashloan only
brings 20k? If not, restructure as two top-level steps:
```json
"steps": [
  { "deposit": { "asset": "USDC", "amount": "10000", "into": "aave" } },
  { "flashloan": { "via": "balancer", "assets": [{"asset":"USDC","amount":"20000"}],
      "then": [
        { "swap":    { "from":"USDC","amount":"20000","to":"WETH","min_amount_out":"6.0" } },
        { "deposit": { "asset":"WETH","amount":"6.0","into":"aave" } },
        { "borrow":  { "asset":"USDC","amount":"20000","from":"aave" } } ] } }
]
```
(mixed-collateral fallback: ~10k aUSDC + ~6 aWETH, 20k USDC debt). Pick whichever
the compiler accepts cleanly; document the choice in a comment on the JSON file.

### Test 4 — 1x short ETH (hand-rolled, no flashloan)

`crates/intent-script/examples/short_eth_1x.json`

```json
{
  "network": "ethereum",
  "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
  "current_timestamp": 1714000000,
  "steps": [
    { "deposit": { "asset": "USDC", "amount": "10000", "into": "aave" } },
    { "borrow":  { "asset": "WETH", "amount": "1.0",   "from": "aave" } },
    { "swap":    { "from": "WETH",  "amount": "1.0",   "to": "USDC", "min_amount_out": "2900" } }
  ]
}
```

Solidity test `test_fork_short_eth_1x`:
- `_dealERC20(USDC, user, 10000e6)`, approve router
- `_approveDelegation(VDEBT_WETH, user, ROUTER_ADDR, 1e18)`
- Read fixture, execute, assert `aUSDC(user) > 0`, `WETH debt(user) ≈ 1e18`, USDC delta in wallet ≈ +(swap proceeds − 10000), router cleared.

## Files to add

| File | Purpose |
|---|---|
| `crates/intent-script/examples/aave_deposit_usdt_borrow_weth.json` | Test 1 intent |
| `crates/intent-script/examples/aave_deposit_wsteth_borrow_usdc.json` | Test 2 intent |
| `crates/intent-script/examples/long_eth_3x_balancer.json` | Test 3 intent |
| `crates/intent-script/examples/short_eth_1x.json` | Test 4 intent |
| `contracts/test/IntentForkScenariosE2E.t.sol` | New Solidity test file (4 tests) |

## Files to modify

- `crates/intent-script/tests/generate_calldata.rs` — register the 4 new examples so `make generate-calldata` writes their `.txt`/`_value.txt` fixtures. (Find the existing list of registered examples; add by analogy.)
- `crates/intent-script/tests/generate_eip712_fixtures.rs` — same, if EIP-712 fixtures are needed (only required if any test uses `executeSigned`; for these four `executeDirect` is fine, so this may be skippable).
- `contracts/test/IntentForkE2E.t.sol` — add a `VDEBT_WETH` constant for reuse (e.g., `0xeA51d7853EEFb32b6ee06b1C12E6dcCA88Be0fFE`) **OR** declare it in the new file. Also add a `USDT` constant if not present.

No router/compiler/DSL-schema code changes. All four scenarios are expressible with existing intent kinds (`deposit`, `borrow`, `swap`, `flashloan`).

## New test file shape: `contracts/test/IntentForkScenariosE2E.t.sol`

Follow the exact structure of `contracts/test/IntentForkE2E.t.sol:26-184`:

- Inherits `Test`
- Same `setUp()`: `vm.etch` IntentRouter at `ROUTER_ADDR`, init `_status` slot to 1, `_allowTarget` for USDT/wstETH/USDC/WETH/AAVE_POOL/UNI_ROUTER/BALANCER_VAULT, fund user with 1000 ETH
- Reuse helpers: `_readCalldata`, `_readValue`, `_dealERC20`, `_assertRouterCleared`, `_approveDelegation`
- Four test functions named `test_fork_aaveDepositUSDT_borrowWETH`, `test_fork_aaveDepositWSTETH_borrowUSDC`, `test_fork_long_eth_3x_balancer`, `test_fork_short_eth_1x`

To avoid duplicating ~120 lines of helper boilerplate across two files, **prefer extracting** the common setup/helpers into a shared base contract. Two clean options:

1. **Lift to `IntentForkBase.t.sol`** if its API matches (it already has router etch, EIP-712 builder, fixture reader per exploration). Have both `IntentForkE2E` and `IntentForkScenariosE2E` inherit from it.
2. **Create `IntentForkE2EBase.t.sol`** holding everything currently in `IntentForkE2E.t.sol:26-184`, then make `IntentForkE2E` and `IntentForkScenariosE2E` thin children.

Pick option 1 if `IntentForkBase` already covers it; otherwise option 2. Either is non-blocking — copying the helpers verbatim into the new file is acceptable for a first pass and matches existing precedent (today the base file exists but `IntentForkE2E` doesn't extend it).

## Critical files to reference (read these during implementation)

- `contracts/test/IntentForkE2E.t.sol` — overall pattern; mirror lines `26-184` (constants, `setUp`, helpers) and `256-334` (Aave deposit and deposit+borrow tests) for tests 1, 2, 4.
- `contracts/test/IntentForkE2E.t.sol:375-440` (`test_fork_complexDefi_executeDirect`) — closest template for credit-delegation + multi-step intents; copy the slippage/dust-tolerance assertion style.
- `contracts/test/IntentForkBase.t.sol` — check whether helpers can be inherited rather than duplicated.
- `crates/intent-script/examples/flashloan_aave_loop.json` — template for test 3 (Balancer flashloan with nested `then` steps).
- `crates/intent-script/examples/aave_borrow.json` — template for tests 1/2 (deposit + borrow as sibling steps).
- `crates/intent-script/examples/swap_uniswap_slippage.json` — slippage syntax reference for swap steps in tests 3 and 4.
- `crates/intent-script/tests/generate_calldata.rs` — find where existing examples are listed; add the four new ones by analogy.
- `config/protocols/ethereum.json` — confirm Aave/Balancer/Uniswap addresses match what the test file already hardcodes.
- `Makefile` — `generate-calldata` and `generate-fixtures` targets are the offline compile step.

## Verification

End-to-end smoke test after implementing:

```bash
cd intent-script

# 1. Compile each new JSON with the CLI to confirm it parses:
cargo run -p intent-script -- compile \
  --input crates/intent-script/examples/aave_deposit_usdt_borrow_weth.json \
  --network ethereum
# (repeat for the other three)

# 2. Generate fixtures (writes test/fixtures/<name>.txt etc):
make generate-fixtures

# 3. Run only the new test file against mainnet fork:
cd contracts
forge test --mc IntentForkScenariosE2E --fork-url $ETH_RPC_URL -vvv

# 4. Confirm pre-existing E2E tests still pass:
forge test --mc IntentForkE2E --fork-url $ETH_RPC_URL -vv

# 5. Full CI flow:
cd ..
make ci
```

Per-test acceptance criteria (assertions inside each `test_fork_*` function):

| Test | Must hold post-execution |
|---|---|
| 1 | `aUSDT(user) > 0`, `WETH(user) ≥ 1e18`, `USDT(user) == 0`, router USDT/WETH balance == 0 |
| 2 | `aWSTETH(user) > 0`, `USDC(user) ≥ 1000e6`, `WSTETH(user) == 0`, router cleared |
| 3 | `aWETH(user) ≈ 9e18` (within slippage), USDC variable debt ≈ 20000e6, `USDC(user) == 0`, Balancer flashloan repaid (no callback revert), router cleared of USDC/WETH |
| 4 | `aUSDC(user) > 0`, WETH variable debt ≈ 1e18, `USDC(user) ≈ swap proceeds − 10000e6` (positive), router cleared |

Pin a fork block in `foundry.toml` (or via `--fork-block-number`) once tests pass, to lock test stability against future Aave parameter changes. Pick a recent block where wstETH and USDT still have nonzero LTV.

## Out of scope

- Refactoring `IntentForkE2E.t.sol` to use `IntentForkBase` as a parent (separate cleanup).
- Adding `executeSigned` (EIP-712) variants of these tests — current four use `executeDirect` only. Easy to add later via the existing `_buildDigest` helper.
- New DSL kinds, new protocol integrations, compiler changes.
- Deploying to a non-mainnet network (anvil, base, arbitrum) — same JSON works elsewhere by changing `"network"`, but multi-network coverage is a follow-up.
