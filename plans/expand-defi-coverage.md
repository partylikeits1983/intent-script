# Expand DeFi Protocol Coverage + Router Fees

> Implementation plan for adding Uniswap V3 LP, Aave looping via Balancer flashloans, Morpho, Lido enhancements, Across bridging, and a router-level fee mechanism. Each phase is self-contained with a definition-of-done and a verification command. Phases are strictly ordered — do not skip ahead.

---

## Context

The intent-script compiler transforms LLM-generated JSON into validated EVM calldata. Its purpose is to *constrain* what an LLM can emit. This plan adds five protocol families plus a fee mechanism, in an order that builds foundations before dependents.

**Locked scope decisions (do not re-open):**
- Tornado Cash and Privacy Pools are **out of scope**. Do not add them. Reasoning: OFAC SDN listing (tornado) and no compiler validation value without an off-chain zk prover (privacy pools).
- Router fee: **10 bps at sweep time, 24-hour governance timelock, 100 bps hard cap**.
- Bridging: **Across V3 only, single-sided** (source-chain call only; destination is a separate intent).
- Flashloan provider: **Balancer V2 Vault** (0% fee).

**Baseline reading:** before starting, read `intent-script/skills/architecture.md`, `intent-script/skills/adding-new-adapters.md`, `intent-script/skills/router-and-eip712.md`, and `intent-script/skills/codebase-status.md`. Do not edit those — they're documentation, not the source of truth. The source of truth is the code.

**Known pre-existing issues** (fix opportunistically during relevant phases):
- `ir/canonical.rs:218` `step_produces(LidoStake)` returns the Lido pool address instead of stETH address — fix in Phase 4.
- `enrich.rs` `AaveV3Withdraw` doesn't add the withdrawn asset to `sweep_tokens` while `AaveV3Borrow` does — fix in Phase 3 (consistency; otherwise users see withdrawn tokens stuck in router).
- `config/protocols/ethereum.json` does not exist. `anvil.json` and `sepolia.json` do. Create `ethereum.json` in Phase 1 — copy anvil.json as a starting point since anvil assumes mainnet-forked addresses.

---

## Phase 0: Pre-flight (30 min)

**Goal:** verify baseline green, produce artifacts to diff against later.

1. `cd /Users/riemann/Desktop/intentOS/intent-script && make test` — must pass.
2. `make test-foundry` — must pass.
3. `make generate-fixtures` — commit the generated fixtures so we have a clean diff baseline.
4. Create `config/assets/ethereum.json` and `config/protocols/ethereum.json` by copying the anvil files:
   ```bash
   cp config/assets/anvil.json config/assets/ethereum.json
   cp config/protocols/anvil.json config/protocols/ethereum.json
   ```
   Add `"ethereum": { "chain_id": 1, "native_asset": "ETH", "wrapped_native": "WETH" }` to `config/chains.json`.

**Definition of done:** all tests green, `ethereum.json` exists, `chains.json` has ethereum entry.

**Verify:** `make test && make test-foundry && cargo run -p intent-script -- crates/intent-script/examples/wrap_eth.json -c ./config -p`.

---

## Phase 1: Router contract foundations (2–3 days)

**Goal:** land all `IntentRouter.sol` changes that downstream phases depend on (callbacks, fees, reentrancy guard), behind a solid test suite. Compiler does not change in this phase.

### 1.1 Add OZ ReentrancyGuard via inline copy

Don't pull in OpenZeppelin for one contract. Inline a 30-line `ReentrancyGuard`:

```solidity
// contracts/src/utils/ReentrancyGuard.sol
pragma solidity ^0.8.20;
abstract contract ReentrancyGuard {
    uint256 private constant _NOT_ENTERED = 1;
    uint256 private constant _ENTERED = 2;
    uint256 private _status = _NOT_ENTERED;
    modifier nonReentrant() {
        require(_status != _ENTERED, "ReentrancyGuard: reentrant call");
        _status = _ENTERED;
        _;
        _status = _NOT_ENTERED;
    }
}
```

### 1.2 ERC-721 receiver

```solidity
// contracts/src/interfaces/IERC721Receiver.sol
pragma solidity ^0.8.20;
interface IERC721Receiver {
    function onERC721Received(address, address, uint256, bytes calldata) external returns (bytes4);
}
```

Router implements it as a pure return of the selector (line addition inside `IntentRouter`):

```solidity
function onERC721Received(address, address, uint256, bytes calldata)
    external pure returns (bytes4) {
    return IERC721Receiver.onERC721Received.selector;  // 0x150b7a02
}
```

### 1.3 Fee mechanism with 24h timelock

Add to `IntentRouter.sol` after the constructor block:

```solidity
// ─── Fees ────────────────────────────────────────────────
uint16 public constant MAX_FEE_BPS = 100;   // 1.00% hard cap
uint256 public constant FEE_TIMELOCK = 1 days;

uint16  public feeBps;              // active rate
address public feeRecipient;        // active recipient
uint16  public pendingFeeBps;       // queued rate
address public pendingFeeRecipient; // queued recipient
uint64  public pendingFeeApplyAt;   // earliest timestamp to apply

event FeeQueued(uint16 bps, address recipient, uint64 applyAt);
event FeeApplied(uint16 bps, address recipient);
event FeeCollected(address indexed token, address indexed recipient, uint256 amount);

function queueFee(uint16 newBps, address newRecipient) external onlyOwner {
    require(newBps <= MAX_FEE_BPS, "fee > max");
    pendingFeeBps = newBps;
    pendingFeeRecipient = newRecipient;
    pendingFeeApplyAt = uint64(block.timestamp + FEE_TIMELOCK);
    emit FeeQueued(newBps, newRecipient, pendingFeeApplyAt);
}

function applyFee() external {
    require(pendingFeeApplyAt != 0, "no pending fee");
    require(block.timestamp >= pendingFeeApplyAt, "timelock");
    feeBps = pendingFeeBps;
    feeRecipient = pendingFeeRecipient;
    pendingFeeApplyAt = 0;
    emit FeeApplied(feeBps, feeRecipient);
}
```

### 1.4 Fee-aware sweep / refund

Replace `_sweep` body with:

```solidity
function _sweep(address[] calldata tokens, address recipient) internal {
    uint256 bps = feeBps;
    address feeTo = feeRecipient;
    bool feesOn = bps > 0 && feeTo != address(0);
    for (uint256 i = 0; i < tokens.length; i++) {
        uint256 bal = IERC20(tokens[i]).balanceOf(address(this));
        if (bal == 0) continue;
        uint256 fee = feesOn ? (bal * bps) / 10_000 : 0;
        if (fee > 0) {
            require(IERC20(tokens[i]).transfer(feeTo, fee), "fee xfer fail");
            emit FeeCollected(tokens[i], feeTo, fee);
        }
        require(IERC20(tokens[i]).transfer(recipient, bal - fee), "Token sweep failed");
    }
}

function _refundETH(address recipient) internal {
    uint256 bal = address(this).balance;
    if (bal == 0) return;
    uint256 bps = feeBps;
    address feeTo = feeRecipient;
    uint256 fee = (bps > 0 && feeTo != address(0)) ? (bal * bps) / 10_000 : 0;
    if (fee > 0) {
        (bool s,) = feeTo.call{value: fee}("");
        require(s, "fee eth fail");
        emit FeeCollected(address(0), feeTo, fee);
    }
    (bool sent,) = recipient.call{value: bal - fee}("");
    require(sent, "ETH refund failed");
}
```

### 1.5 Reentrancy guard on entry points

Apply `nonReentrant` to both `executeDirect` and `executeSigned`. Flashloan callback (added in Phase 5) will intentionally be called *inside* a non-reentrant context — design that around transient-storage guards, not the contract-level guard.

### 1.6 Tests (contracts/test/)

Create `contracts/test/IntentRouterFees.t.sol`:
- `test_QueueAndApplyFee_HappyPath`
- `test_ApplyFee_RevertsBeforeTimelock`
- `test_QueueFee_RevertsAboveMax`
- `test_Sweep_DeductsFee_Tokens`
- `test_Sweep_ZeroFee_NoDeduction`
- `test_Refund_DeductsFee_Eth`
- `test_FeeRecipientZero_NoFeeTaken`

Create `contracts/test/IntentRouterReentrancy.t.sol`:
- `test_ExecuteDirect_Reentrancy_Reverts` — malicious token's `transfer` re-enters `executeDirect`, expect revert.

Update `contracts/test/IntentRouter.t.sol::setUp` to call `router.queueFee(0, address(0))` + warp + `applyFee()` so existing tests run with zero fee (no math changes needed in old tests).

### Definition of done, Phase 1
- All new and existing Foundry tests pass: `cd contracts && forge test -vv`.
- No Rust compiler changes yet.
- `git diff --stat` shows only `contracts/` touched plus possibly `config/` from Phase 0.

---

## Phase 2: Compiler fee awareness (1 day)

**Goal:** make the compiler aware of the router fee so `step_produces` doesn't overestimate, which would cause `"all"` chains to revert.

### 2.1 Extend config schema

In `config/protocols/ethereum.json` (and anvil.json, sepolia.json), add to the `intent_router` entry:

```json
"intent_router": {
  "type": "router", "version": "v1",
  "contracts": { "router": "0x…" },
  "fee_bps": 10
}
```

### 2.2 Registry loader

Edit `crates/intent-script/src/registry/loader.rs`:
- Add `pub fee_bps: Option<u16>` to `ProtocolConfig` (serde-optional; default 0).
- Add method `RegistryContext::fee_bps(&self) -> u16` that returns `self.protocols.get("intent_router").and_then(|p| p.fee_bps).unwrap_or(0)`.

### 2.3 Thread into IR

In `crates/intent-script/src/ir/canonical.rs`, add `pub fee_bps: u16` to `ResolvedIntent` (struct at line 14). Populate it in `normalize.rs` where `ResolvedIntent` is constructed (search for `ResolvedIntent {` in normalize.rs — there's one construction site).

### 2.4 Update step_produces

`ir/canonical.rs:201-227` currently returns raw amounts. Change the signature:

```rust
pub fn step_produces(step: &ResolvedStep, fee_bps: u16) -> Option<(Address, U256)>
```

Apply the fee to every producing variant:
```rust
let reduced = amount * U256::from(10_000 - fee_bps) / U256::from(10_000);
Some((token, reduced))
```

Call sites to update:
- `compiler/normalize.rs::resolve_amount_or_all` (around line 648–674) — thread `fee_bps` through.
- Any `tests/*.rs` using `step_produces` — update calls.

**Edge case:** if a step's output will NOT be sweeped (e.g., flashloan repayment, where tokens get transferred to vault, not the user), the fee doesn't apply. For v1 just accept slight over-conservatism — `"all"` in downstream steps will be marginally low, which only causes missed-dust not reverts.

### 2.5 Tests

Add `crates/intent-script/tests/fee_aware_produce.rs`:
- Feed a script with `fee_bps=10`, swap with `min_amount_out=1000`, deposit `"all"` → assert deposited amount is `1000 * 9990 / 10_000 = 999`.
- Set `fee_bps=0` → assert no reduction.

### Definition of done, Phase 2
- `cargo test -p intent-script` passes.
- `cargo test -p intent-script -- fee_aware_produce` passes.

---

## Phase 3: Lido enhancements (1 day)

**Goal:** fix the existing `LidoStake` produce bug and add unwrap/withdraw-queue support. Smallest scope; validates the adapter-recipe still works cleanly post Phase 1/2.

### 3.1 Fix LidoStake producing wrong token

`ir/canonical.rs:218` — `LidoStake { lido, amount, referral }` lacks a steth address. Either:

**Option A (preferred, smaller):** add `steth: Address` to the variant.
```rust
LidoStake { lido: Address, steth: Address, amount: U256, referral: Address },
```
Update:
- `normalize.rs` where LidoStake is constructed — resolve steth from `protocols.lido.contracts.steth`.
- `adapters/lido.rs::lower_stake` — no change (sol! uses `lido` contract as target).
- `step_produces` — return `(steth, amount)` instead of `(lido, amount)`.
- `step_consumes` — no change (consumes ETH = Address::ZERO).
- `enrich.rs` — the Lido branch that does `tokens_in_router.insert(token_out)` / `sweep_tokens.push(token_out)` — switch to using `steth` field.

### 3.2 Add wstETH → stETH unwrap

Extend `public_ast.rs::UnwrapStep` — current variant only handles WETH. Detect `asset == "wstETH"` in `normalize.rs` and produce a new ResolvedStep variant:

```rust
WstETHUnwrap { wsteth: Address, steth: Address, amount: U256 },
```

`adapters/lido.rs`:
```rust
alloy_sol_types::sol! {
    function unwrap(uint256 _wstETHAmount) external returns (uint256);
}
pub fn lower_wsteth_unwrap(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> { … }
```

Enricher: consumes wstETH (auto-insert transferFrom + no approval needed; unwrap uses balance directly). Produces stETH in router → add to sweep.

### 3.3 Lido withdrawal queue (request + claim)

Add contract: `config/protocols/ethereum.json`:
```json
"lido": {
  "type": "staking", "version": "v1",
  "contracts": {
    "steth":            "0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84",
    "wsteth":           "0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0",
    "withdrawal_queue": "0x889edC2eDab5f40e902b864aD4d7AdE8E412F9B1"
  }
}
```

Add Step variants:
```rust
pub enum Step { …,
    RequestWithdrawal(RequestWithdrawalStep),
    ClaimWithdrawal(ClaimWithdrawalStep),
}
pub struct RequestWithdrawalStep {
    pub asset: String,          // "stETH" or "wstETH"
    pub amounts: Vec<String>,   // one per NFT to mint
    pub from: String,           // "lido"
}
pub struct ClaimWithdrawalStep {
    pub protocol: String,       // "lido"
    pub request_ids: Vec<u64>,  // NFT token ids
    pub hints: Vec<u64>,        // caller supplies from findCheckpointHints off-chain
}
```

ResolvedStep:
```rust
LidoRequestWithdrawal { queue: Address, steth: Address, amounts: Vec<U256>, owner: Address },
LidoClaimWithdrawal   { queue: Address, request_ids: Vec<U256>, hints: Vec<U256> },
```

Adapter `adapters/lido.rs`:
```rust
alloy_sol_types::sol! {
    function requestWithdrawals(uint256[] _amounts, address _owner) external returns (uint256[]);
    function requestWithdrawalsWstETH(uint256[] _amounts, address _owner) external returns (uint256[]);
    function claimWithdrawals(uint256[] _requestIds, uint256[] _hints) external;
}
```

Enricher for `LidoRequestWithdrawal`: pull stETH/wstETH via transferFrom + approve queue. Queue mints NFTs to `_owner=signer` (so no router NFT custody needed for the request itself — but we'll need `onERC721Received` from Phase 1 if the LLM ever chains request → send).

Validation: `hints.len() == request_ids.len()`; reject empty arrays.

### 3.4 Tests

- `tests/integration.rs`: `test_lido_unwrap_wsteth`, `test_lido_request_withdrawal_steth`, `test_lido_claim_withdrawal`.
- `tests/generate_calldata.rs`: add fixture `lido_request_withdrawal.txt`.
- `contracts/test/LidoFork.t.sol` (NEW): fork mainnet, stake 1 ETH, request 0.5 stETH withdrawal, warp 7 days, claim. Verify balance returned.
- Fix `tests/enricher_tests.rs` if any assertion was relying on the old buggy `step_produces(LidoStake)`.

### Definition of done, Phase 3
- `make test && make test-foundry` pass.
- New Lido fork test passes with `ETH_RPC_URL`.

---

## Phase 4: Morpho Blue (2 days)

**Goal:** add Morpho Blue as a new lending protocol. Exercises the config-driven-market pattern.

### 4.1 Config

`config/protocols/ethereum.json` — new entry:
```json
"morpho": {
  "type": "lending", "version": "blue",
  "contracts": { "pool": "0xBBBBBbbBBb9cC5e90e3b3Af64bdAF62C37EEFFCb" },
  "markets": {
    "USDC-WETH-86": {
      "loan":       "USDC",
      "collateral": "WETH",
      "oracle":     "0x48F7E36EB6B826B2dF4B2E630B62Cd25e89E40e2",
      "irm":        "0x870aC11D48B15DB9a138Cf899d20F13F79Ba00BC",
      "lltv":       "860000000000000000",
      "id":         "0xb323495f7e4148be5643a4ea4a8221eef163e4bccfdedc2a6f4696baacbc86cc"
    }
    /* Add USDC-WBTC, DAI-WETH etc. as needed — all with precomputed id */
  }
}
```

The `id` is `keccak256(abi.encode(MarketParams))`. Precompute with `cast keccak` during config authoring; store it to avoid computing every compile.

### 4.2 DSL

Reuse `deposit`/`borrow`/`withdraw` with `"into": "morpho"` + required `market` field. Add optional `as: "collateral"` to distinguish collateral supply from lend supply:

```json
{ "deposit": { "asset": "WETH", "amount": "1.0", "into": "morpho",
               "market": "USDC-WETH-86", "as": "collateral" } }
{ "deposit": { "asset": "USDC", "amount": "1000", "into": "morpho",
               "market": "USDC-WETH-86" } }
{ "borrow":  { "asset": "USDC", "amount": "500", "from": "morpho",
               "market": "USDC-WETH-86" } }
```

### 4.3 Code changes

`schema/public_ast.rs` — add optional fields to existing structs:
```rust
pub struct DepositStep {
    pub asset: String, pub amount: String, pub into: String,
    #[serde(default)] pub market: Option<String>,
    #[serde(default)] pub r#as: Option<String>,  // "collateral" | None
}
// Same for BorrowStep, WithdrawStep (market field, no `as` needed)
```

`ir/canonical.rs` — new variants:
```rust
MorphoSupply         { pool, market_params: MorphoMarketParams, amount, on_behalf: Address },
MorphoSupplyCollat   { pool, market_params, amount, on_behalf: Address },
MorphoBorrow         { pool, market_params, amount, on_behalf: Address },
MorphoWithdraw       { pool, market_params, amount, on_behalf: Address, receiver: Address },
MorphoWithdrawCollat { pool, market_params, amount, on_behalf: Address, receiver: Address },
MorphoRepay          { pool, market_params, amount, on_behalf: Address },
```

Shared struct:
```rust
pub struct MorphoMarketParams {
    pub loan_token: Address,
    pub collateral_token: Address,
    pub oracle: Address,
    pub irm: Address,
    pub lltv: U256,
}
```

`normalize.rs`:
- When `into == "morpho"` or `from == "morpho"`, look up `market` in `protocols.morpho.markets`.
- Reject if market not found or missing `market` field.
- Validate: `asset` matches `loan` for supply/borrow/repay; `asset` matches `collateral` when `as == "collateral"`.
- Emit the right MorphoX variant.

`validate.rs`:
- Reject if `as == "collateral"` combined with borrow/repay steps.
- Reject if `asset` doesn't match the market's loan or collateral token.

`enrich.rs`:
- Supply/supplyCollat: auto-insert transferFrom + approve(pool, amount).
- Borrow/Withdraw/WithdrawCollat: recipient=router when batched, add to `sweep_tokens`.
- Repay: transferFrom + approve, no sweep.

`adapters/morpho.rs` (NEW):
```rust
alloy_sol_types::sol! {
    struct MarketParams {
        address loanToken;
        address collateralToken;
        address oracle;
        address irm;
        uint256 lltv;
    }
    function supply(MarketParams marketParams, uint256 assets, uint256 shares,
                    address onBehalf, bytes data) external returns (uint256, uint256);
    function supplyCollateral(MarketParams marketParams, uint256 assets,
                    address onBehalf, bytes data) external;
    function borrow(MarketParams marketParams, uint256 assets, uint256 shares,
                    address onBehalf, address receiver) external returns (uint256, uint256);
    function withdraw(MarketParams marketParams, uint256 assets, uint256 shares,
                    address onBehalf, address receiver) external returns (uint256, uint256);
    function withdrawCollateral(MarketParams marketParams, uint256 assets,
                    address onBehalf, address receiver) external;
    function repay(MarketParams marketParams, uint256 assets, uint256 shares,
                    address onBehalf, bytes data) external returns (uint256, uint256);
}
```

All calls pass `shares=0`, `data=empty` (no callbacks).

`adapters/mod.rs` — register the six new dispatch arms.

### 4.4 Tests

- `tests/integration.rs`: `test_morpho_supply_collateral_and_borrow`.
- `tests/generate_calldata.rs`: `morpho_supply_usdc.txt`, `morpho_borrow_usdc_against_weth.txt`.
- `contracts/test/MorphoFork.t.sol` (NEW): fork mainnet, supply WETH as collateral, borrow USDC, verify user USDC increases by borrowed amount less fee.

### Definition of done, Phase 4
- `make test && make test-foundry` pass.
- Morpho fork test passes.

---

## Phase 5: Balancer flashloan + Aave looping (4–5 days)

**Goal:** enable leveraged loops via Balancer V2 flashloans. Biggest refactor — the enricher runs recursively.

### 5.1 Router callback (`contracts/src/IntentRouter.sol`)

```solidity
address public constant BALANCER_VAULT = 0xBA12222222228d8Ba445958a75a0704d566BF2C8;

// Transient storage guard (EIP-1153 / Solidity 0.8.24+). Check foundry.toml solc = "0.8.24".
// Layout: slot 0 = flashloan-in-progress nonce; set before vault call, checked in callback.
bytes32 constant FLASHLOAN_GUARD_SLOT = keccak256("intent.flashloan.guard");

function receiveFlashLoan(
    address[] calldata tokens,
    uint256[] calldata amounts,
    uint256[] calldata feeAmounts,
    bytes calldata userData
) external {
    require(msg.sender == BALANCER_VAULT, "not vault");
    bytes32 expected;
    bytes32 slot = FLASHLOAN_GUARD_SLOT;
    assembly { expected := tload(slot) }
    require(expected != bytes32(0), "no flashloan in progress");
    // clear guard
    assembly { tstore(slot, 0) }

    // Decode inner calls
    (Call[] memory innerCalls, bytes32 cookie) = abi.decode(userData, (Call[], bytes32));
    require(cookie == expected, "guard mismatch");

    // Execute inner — manual loop (can't call _executeCalls with memory Call[])
    for (uint256 i = 0; i < innerCalls.length; i++) {
        require(allowedTargets[innerCalls[i].target], "Target not allowed");
        (bool ok, bytes memory ret) = innerCalls[i].target.call{value: innerCalls[i].value}(innerCalls[i].callData);
        if (!ok) assembly { revert(add(ret, 32), mload(ret)) }
    }

    // Repay: inner calls MUST have left enough balance for vault to pull.
    // Balancer expects the router to transfer tokens back (not approve). Transfer here.
    for (uint256 i = 0; i < tokens.length; i++) {
        uint256 owed = amounts[i] + feeAmounts[i];
        require(IERC20(tokens[i]).transfer(BALANCER_VAULT, owed), "flashloan repay fail");
    }
}
```

**Important Balancer detail:** Balancer Vault calls `receiveFlashLoan` and then **pulls** tokens via `safeTransfer` expectation — actually it checks `balanceOf` before and after. The repay model: at callback end, router must `transfer` the owed amount back to Vault. Double-check this against Balancer V2 Vault source (`FlashLoans.sol`) before implementing — if semantics differ from description, update accordingly.

**Setting the guard:** the OUTER batch contains a step that calls `vault.flashLoan(router, tokens, amounts, userData)`. Before that call, the batch must set the transient-storage guard. Cleanest way: add a dedicated `_setFlashloanGuard(bytes32)` internal helper and call it as a special `SetGuard` pre-step. Simpler alternative: embed the guard-set inside `executeDirect/executeSigned` any time the batch contains a call to `vault.flashLoan`. Pragmatic choice: do it explicitly from the compiler as a pre-call step targeting the router itself. But the router's allowlist blocks calls to itself... so instead, **add the guard-set as a `beforeCall` branch in `_executeCalls`** when the target is BALANCER_VAULT and the selector matches `flashLoan`:

```solidity
// Inside _executeCalls, before .call:
if (calls[i].target == BALANCER_VAULT && bytes4(calls[i].callData[0:4]) == IVault.flashLoan.selector) {
    bytes32 cookie = keccak256(abi.encode(block.number, i, msg.sender));
    bytes32 slot = FLASHLOAN_GUARD_SLOT;
    assembly { tstore(slot, cookie) }
    // cookie needs to match the userData-encoded cookie. The compiler must
    // encode the same cookie... but compiler doesn't know block.number.
    //
    // REVISED: use a fixed per-batch cookie derived from msg.sender + nonce.
    // The compiler's userData encodes cookie = keccak256(signer,nonce,index-in-batch).
    // Simpler still: use a constant non-zero sentinel and only check != 0.
}
```

**Simpler final design** (use this):
- Guard = a boolean-ish transient flag set to 1 before vault call, checked != 0 in callback, cleared inside callback. No cookie/cookie-mismatch. Security comes from `msg.sender == VAULT` check, not the cookie.
- Rationale: the only way to invoke `receiveFlashLoan` is for the Vault to call it, and the Vault only calls it in response to `flashLoan(router, …)`. The flag exists only to prevent someone from directly calling `receiveFlashLoan` even with a spoofed msg.sender (e.g., via a delegatecall in a compromised target). Set → execute → clear.

### 5.2 Config

`config/protocols/ethereum.json`:
```json
"balancer": {
  "type": "flashloan_provider", "version": "v2",
  "contracts": { "vault": "0xBA12222222228d8Ba445958a75a0704d566BF2C8" }
}
```

### 5.3 DSL

```json
{ "flashloan": {
    "via": "balancer",
    "assets": [{ "asset": "WETH", "amount": "2.0" }],
    "then": [
      { "deposit": { "asset": "WETH", "amount": "3.0", "into": "aave" } },
      { "borrow":  { "asset": "USDC", "amount": "5500", "from": "aave" } },
      { "swap":    { "from": "USDC", "amount": "all", "to": "WETH", "min_amount_out": "2.0" } }
    ]
  }
}
```

### 5.4 Compiler changes

`schema/public_ast.rs`:
```rust
pub enum Step { …, Flashloan(FlashloanStep) }
pub struct FlashloanStep {
    pub via: String,
    pub assets: Vec<FlashloanAsset>,
    pub then: Vec<Step>,
}
pub struct FlashloanAsset { pub asset: String, pub amount: String }
```

`ir/canonical.rs`:
```rust
pub enum ResolvedStep { …,
    BalancerFlashloan {
        vault: Address,
        tokens: Vec<Address>,
        amounts: Vec<U256>,
        inner_calls: Vec<ConcreteCall>,      // lowered inner pipeline
        inner_sweep: Vec<Address>,            // tokens to sweep at outer level
        user_data: Bytes,                     // pre-encoded (Call[], cookie)
    },
}
```

`compiler/normalize.rs`: recognize `Step::Flashloan` → resolve vault addr + asset addrs + amounts. Recursively normalize `then:` steps (they become a sub-pipeline). Note: inner `from` address is still the user signer (flashloan doesn't change who the effective caller is; all inner ops are `msg.sender=router`, `on_behalf_of=signer`).

`compiler/validate.rs`:
- Reject nested flashloans (`then:` step cannot contain another `Flashloan`).
- Compute `produced_by_inner[token]` from `step_produces` over the inner pipeline.
- For each flashloaned `(token, amount)`: require `produced_by_inner[token] >= amount` OR the inner pipeline contains an explicit `swap`/`withdraw` producing that token with sufficient `step_produces`. If not, `CompileError::Validation("Flashloan not repayable: ...")`.
- Max inner depth = 1; max inner steps = 5.

`compiler/enrich.rs` — new recursive path:
1. For each outer step that's `BalancerFlashloan`:
   a. Create a fresh enrich context (`tokens_in_router` pre-populated with flashloan tokens, since Vault sends them to router before calling back).
   b. Run the inner enrich pass on `inner_resolved_steps`.
   c. **Auto-append repayment**: for each flashloaned token, append a step that ensures the tokens are in the router at the end of the inner pipeline. Since the router needs to `transfer` back in `receiveFlashLoan`, no approval is needed — just balance presence. Validator already proved this.
2. Lower inner steps → `Vec<ConcreteCall>`.
3. Encode `user_data = abi.encode((Call[], bytes32))` — the cookie is a constant sentinel `bytes32(uint256(1))`.

`compiler/lower.rs`:
```rust
ResolvedStep::BalancerFlashloan { vault, tokens, amounts, inner_calls, user_data, .. } => {
    balancer::lower_flashloan(vault, tokens, amounts, inner_calls, user_data)
}
```

`adapters/balancer.rs` (NEW):
```rust
alloy_sol_types::sol! {
    function flashLoan(
        address recipient, address[] tokens, uint256[] amounts, bytes userData
    ) external;
}

pub fn lower_flashloan(
    vault: &Address, tokens: &[Address], amounts: &[U256],
    inner_calls: &[ConcreteCall], user_data: &Bytes,
) -> Result<Vec<ConcreteCall>> {
    let calldata = flashLoanCall {
        recipient: /* router address — pass via parameter or registry */,
        tokens: tokens.to_vec(),
        amounts: amounts.to_vec(),
        userData: user_data.clone(),
    }.abi_encode();
    Ok(vec![ConcreteCall { to: *vault, calldata: Bytes::from(calldata), value: U256::ZERO, … }])
}
```

Note: `lower_flashloan` needs the router address. Either: (a) pass it into `lower_step` as part of the registry context (some adapters already receive registry; see `adapters/mod.rs:14`), or (b) bake the router into the ResolvedStep. Prefer (a) — `lower_step(step, registry)` exists.

### 5.5 Tests

- `tests/integration.rs`: `test_balancer_flashloan_simple` (flashloan WETH, deposit into Aave, borrow USDC, swap back to WETH — verify inner call order, repayment call encoded).
- `tests/flashloan_tests.rs` (NEW):
  - `test_flashloan_rejects_unrepayable`
  - `test_flashloan_rejects_nested`
  - `test_flashloan_inner_enrich_preserves_tokens_in_router`
  - `test_flashloan_repayment_transfer_appended`
- `tests/generate_calldata.rs`: `loop_aave_3x_weth.txt`.
- `contracts/test/IntentRouterFlashloan.t.sol` (NEW): fork mainnet, run a 3× leverage loop, verify final Aave HF > 1.5, verify user's net position matches expected.
- Add BALANCER_VAULT to whitelist in all fork tests that use flashloans.

### Definition of done, Phase 5
- `make test && make test-foundry && make test-fork-e2e` pass.
- Flashloan fork test completes with correct final state.
- Solidity compiler pragma bumped to `^0.8.24` if necessary for `tstore`/`tload`.

---

## Phase 6: Uniswap V3 LP (4 days)

**Goal:** add LP lifecycle (mint/increase/decrease/collect). Exercises the NFT flow enabled by Phase 1's `onERC721Received`.

### 6.1 Config

`config/protocols/ethereum.json` — extend `uniswap`:
```json
"uniswap": {
  "type": "dex", "version": "v3",
  "contracts": {
    "router":           "0xE592427A0AEce92De3Edee1F18E0157C05861564",
    "quoter":           "0xb27308f9F90D607463bb33eA1BeBb41C27CE5AB6",
    "position_manager": "0xC36442b4a4522E871399CD717aBDD847Ab11FE88"
  }
}
```

### 6.2 DSL

```json
{ "lp_mint": { "protocol": "uniswap", "token0": "USDC", "token1": "WETH",
               "fee": "3000", "tick_lower": -200040, "tick_upper": -199980,
               "amount0": "1000", "amount1": "0.3",
               "min_amount0": "990", "min_amount1": "0.29" } }
{ "lp_increase": { "position_id": "12345", "amount0": "500", "amount1": "0.15",
                   "min_amount0": "495", "min_amount1": "0.148" } }
{ "lp_decrease": { "position_id": "12345", "liquidity": "all",
                   "min_amount0": "950", "min_amount1": "0.28" } }
{ "lp_collect":  { "position_id": "12345" } }
```

**Important constraint:** `position_id` cannot be `"last_minted"` — resolve only explicit NFT ids. Mint-then-collect must be two separate intents. This keeps the linear step model.

### 6.3 Schema + IR

`public_ast.rs`:
```rust
pub enum Step { …, LpMint(LpMintStep), LpIncrease(LpIncreaseStep),
    LpDecrease(LpDecreaseStep), LpCollect(LpCollectStep) }

pub struct LpMintStep {
    pub protocol: String,
    pub token0: String, pub token1: String,
    pub fee: String,                        // "500" | "3000" | "10000"
    pub tick_lower: i32, pub tick_upper: i32,
    pub amount0: String, pub amount1: String,
    pub min_amount0: String, pub min_amount1: String,
    #[serde(default)] pub deadline: Option<u64>,
}
// ...similarly LpIncreaseStep / LpDecreaseStep / LpCollectStep
```

`ir/canonical.rs`:
```rust
UniswapV3LpMint {
    npm: Address,
    token0: Address, token1: Address,
    fee: u32,
    tick_lower: i32, tick_upper: i32,
    amount0: U256, amount1: U256,
    amount0_min: U256, amount1_min: U256,
    recipient: Address, deadline: U256,
},
UniswapV3LpIncrease { npm, token_id: U256, amount0, amount1, amount0_min, amount1_min, deadline },
UniswapV3LpDecrease { npm, token_id: U256, liquidity: u128, amount0_min, amount1_min, deadline, recipient: Address },
UniswapV3LpCollect  { npm, token_id: U256, recipient: Address, amount0_max: U128, amount1_max: U128 },
```

### 6.4 Normalize / validate

`normalize.rs`:
- Sort (token0, token1) lexicographically. If caller provides them reversed, swap + swap amounts/mins + emit warning. This matches NPM's invariant.
- Validate fee in {500, 3000, 10000}.
- Validate `tick_lower < tick_upper` and both multiples of tickSpacing:
  ```rust
  fn tick_spacing(fee: u32) -> i32 { match fee { 500=>10, 3000=>60, 10000=>200, _=>unreachable!() } }
  ```
- Disallow `"all"` on LP amount fields for v1.
- `recipient = router` when batched (so the NFT lands in router, then is sweeped out via SendErc721).

`validate.rs`:
- `amount0_min > 0 || amount1_min > 0` (slippage protection — can't be both zero).

### 6.5 Enrich

For `UniswapV3LpMint` / `UniswapV3LpIncrease`:
- Auto-insert `transferFrom` for BOTH tokens (skip per `tokens_in_router`).
- Auto-insert `approve(npm, amount)` for BOTH tokens.
- After the LP step, push a `SendErc721 { contract: npm, from: router, to: signer, token_id: <unknown for mint> }`...

**Problem:** for mint, `token_id` is unknown at compile time. Options:
- **A:** Skip the sweep for mint; rely on the fact that `safeMint` transfers NFT to `recipient` parameter directly — if we pass `recipient=signer`, NFT never enters router.
- **B:** Keep recipient=router for consistency and accept that the NFT is stuck until the user does a manual sweep.

**Go with A.** Mint: `recipient=signer` (NFT goes directly to user). Increase: no NFT transfer (existing position NFT stays wherever it is). Decrease/Collect: the NPM requires router to be owner-or-approved of the NFT. User must `npm.approve(router, tokenId)` as a prerequisite (like credit delegation). Document this in `intent-script/skills/adding-new-adapters.md`.

For dust tokens from `decrease`: NPM sends token0/token1 to `recipient=router`; add both to `sweep_tokens`.

### 6.6 Adapter `adapters/uniswap_v3_lp.rs` (NEW)

```rust
alloy_sol_types::sol! {
    struct MintParams {
        address token0; address token1; uint24 fee;
        int24 tickLower; int24 tickUpper;
        uint256 amount0Desired; uint256 amount1Desired;
        uint256 amount0Min; uint256 amount1Min;
        address recipient; uint256 deadline;
    }
    function mint(MintParams params) external payable
        returns (uint256 tokenId, uint128 liquidity, uint256 amount0, uint256 amount1);

    struct IncreaseLiquidityParams {
        uint256 tokenId; uint256 amount0Desired; uint256 amount1Desired;
        uint256 amount0Min; uint256 amount1Min; uint256 deadline;
    }
    function increaseLiquidity(IncreaseLiquidityParams params) external payable
        returns (uint128 liquidity, uint256 amount0, uint256 amount1);

    struct DecreaseLiquidityParams {
        uint256 tokenId; uint128 liquidity;
        uint256 amount0Min; uint256 amount1Min; uint256 deadline;
    }
    function decreaseLiquidity(DecreaseLiquidityParams params) external
        returns (uint256 amount0, uint256 amount1);

    struct CollectParams {
        uint256 tokenId; address recipient;
        uint128 amount0Max; uint128 amount1Max;
    }
    function collect(CollectParams params) external
        returns (uint256 amount0, uint256 amount1);
}
```

Use `type(uint128).max` for `amount0Max/amount1Max` in collect (standard pattern).

### 6.7 Tests

- `tests/integration.rs`: `test_lp_mint_compiles`, `test_lp_increase`, `test_lp_decrease_collect`.
- `tests/enricher_tests.rs`: verify dual-token transferFrom + approves inserted; verify `recipient=signer` on mint.
- `tests/generate_calldata.rs`: `lp_mint_usdc_eth.txt`, `lp_rebalance_range.txt` (3-step: decrease → collect → mint).
- `contracts/test/LpFork.t.sol` (NEW): fork mainnet, mint USDC/WETH LP, increase, decrease, collect. Verify token return.

### Definition of done, Phase 6
- `make test && make test-foundry && make test-fork-e2e` pass.
- LP fork test runs a full mint → rebalance cycle.

---

## Phase 7: Across bridging, single-sided (1 day)

**Goal:** one-sided bridge via Across. Source-chain call only.

### 7.1 Config

`config/protocols/ethereum.json`:
```json
"across": {
  "type": "bridge", "version": "v3",
  "contracts": { "spoke_pool": "0x5c7BCd6E7De5423a257D81B442095A1a6ced35C5" }
}
```

### 7.2 DSL

```json
{ "bridge": { "via": "across", "asset": "USDC", "amount": "1000",
              "to_chain": "arbitrum", "recipient": "0x…",
              "relayer_fee_bps": "5" } }
```

### 7.3 Schema + IR

```rust
pub enum Step { …, Bridge(BridgeStep) }
pub struct BridgeStep {
    pub via: String,
    pub asset: String, pub amount: String,
    pub to_chain: String, pub recipient: String,
    pub relayer_fee_bps: String,
}

// canonical.rs
AcrossDepositV3 {
    spoke_pool: Address,
    depositor: Address,
    recipient: Address,
    input_token: Address,
    output_token: Address,     // same address on dest; for v1 assume canonical token
    input_amount: U256,
    output_amount: U256,       // input_amount - relayer fee
    destination_chain_id: U256,
    exclusive_relayer: Address,  // Address::ZERO
    quote_timestamp: u32,
    fill_deadline: u32,
    exclusivity_deadline: u32,
    message: Bytes,              // empty
},
```

### 7.4 Normalize

- Look up `to_chain` in `chains.json` → chain_id.
- Compute `output_amount = input_amount * (10_000 - relayer_fee_bps) / 10_000`.
- `quote_timestamp = script.current_timestamp` (require it to be present).
- `fill_deadline = quote_timestamp + 4 * 3600` (4h).
- `exclusivity_deadline = 0` (open to any relayer).
- `output_token = input_token` (for v1; Across supports cross-token but that's v2).

### 7.5 Validate

- `relayer_fee_bps <= 50` (0.5%).
- `recipient != 0x0`.
- `to_chain` exists in chains.json.
- Reject native ETH; require wrapped (Across deposits WETH not ETH). Consider auto-wrapping via a pre-step... actually no — keep constraint explicit, let the LLM/UI compose a wrap+bridge intent.

### 7.6 Enrich

Standard transferFrom + approve(spoke_pool). No sweep (tokens are in flight cross-chain).

### 7.7 Adapter `adapters/across.rs`

```rust
alloy_sol_types::sol! {
    function depositV3(
        address depositor, address recipient,
        address inputToken, address outputToken,
        uint256 inputAmount, uint256 outputAmount,
        uint256 destinationChainId, address exclusiveRelayer,
        uint32 quoteTimestamp, uint32 fillDeadline, uint32 exclusivityDeadline,
        bytes message
    ) external payable;
}
```

### 7.8 Tests

- `tests/integration.rs`: `test_bridge_across_usdc_to_arbitrum`.
- `tests/generate_calldata.rs`: `bridge_usdc_to_arbitrum.txt`.
- `contracts/test/BridgeFork.t.sol`: fork mainnet, deposit; verify `FundsDeposited` event.

### Definition of done, Phase 7
- `make test && make test-foundry && make test-fork-e2e` pass.

---

## Phase 8: Final integration (1 day)

### 8.1 Update skill docs

The skills files are agent-facing documentation. Update:
- `intent-script/skills/codebase-status.md` — new rows in supported-protocols table, new test counts.
- `intent-script/skills/json-dsl-spec.md` — new step types (flashloan, bridge, lp_*, request_withdrawal, claim_withdrawal, morpho via `into="morpho"`).
- `intent-script/skills/llm-intent-generation.md` — new action recipes and examples.
- `intent-script/skills/adding-new-adapters.md` — add NFT-sweep and flashloan-inner-enrich patterns as appendix.

### 8.2 Update router allowlist deployment

Ensure the deployment script whitelists all new targets:
- Balancer Vault
- Morpho Blue
- Uniswap V3 NonfungiblePositionManager
- Lido Withdrawal Queue
- Across SpokePool

Check `contracts/script/` for the deploy script. If it's hardcoded, update it.

### 8.3 End-to-end LLM round-trip

Prompt a Claude agent with the updated `llm-intent-generation.md` system prompt, feeding test prompts:
- "Open a USDC/ETH LP position at 0.3% fee between 1800 and 2200 USDC/ETH"
- "3× leveraged ETH on Aave using a Balancer flashloan"
- "Supply 5000 USDC to Morpho USDC/WETH market"
- "Request withdrawal of 1 stETH from Lido"
- "Bridge 1000 USDC to Arbitrum via Across"

Each must produce JSON that compiles successfully.

### 8.4 Regenerate fixtures

`make generate-fixtures` — commit the new fixture files.

### 8.5 Final verification

```bash
cd /Users/riemann/Desktop/intentOS/intent-script
make test && make test-foundry && ETH_RPC_URL=... make test-fork-e2e
```

All green = done.

---

## Critical Files Reference

| Area | Files |
|---|---|
| Public AST | `crates/intent-script/src/schema/public_ast.rs` |
| IR | `crates/intent-script/src/ir/canonical.rs` |
| Normalize | `crates/intent-script/src/compiler/normalize.rs` |
| Validate | `crates/intent-script/src/compiler/validate.rs` |
| Enrich | `crates/intent-script/src/compiler/enrich.rs` |
| Lower dispatcher | `crates/intent-script/src/adapters/mod.rs` |
| Existing adapters | `crates/intent-script/src/adapters/*.rs` |
| New adapters | `balancer.rs`, `morpho.rs`, `uniswap_v3_lp.rs`, `across.rs` (all NEW in `adapters/`) |
| Registry | `crates/intent-script/src/registry/loader.rs` |
| Router contract | `contracts/src/IntentRouter.sol` |
| Router utils | `contracts/src/utils/ReentrancyGuard.sol` (NEW), `contracts/src/interfaces/IERC721Receiver.sol` (NEW) |
| Config | `config/chains.json`, `config/assets/ethereum.json` (NEW), `config/protocols/ethereum.json` (NEW) |
| Rust tests | `crates/intent-script/tests/*.rs` |
| Foundry tests | `contracts/test/*.t.sol` |

---

## Global Invariants (Do Not Violate)

1. **Library is `no_std` compatible.** No `std::time`, no file I/O, no network. All config flows in as `&str`.
2. **Compiler is deterministic.** Same input → same output bytes. No `SystemTime::now()` in the library; deadlines use `script.current_timestamp`.
3. **Allowlist is the ONLY thing preventing unsafe targets.** Every new protocol must be added to the deploy-time allowlist. Tests use `vm.store` or explicit `setAllowedTarget`.
4. **EIP-712 typed data must match Solidity.** If `IntentBatch` struct changes (it won't in this plan), update `eip712.rs` identically.
5. **Max 5 outer steps, max 5 inner flashloan steps, max flashloan depth 1.** Keep LLM attack surface small.
6. **Every swap/LP/bridge MUST have slippage protection** — no zero-slippage allowed.
7. **No silent destructive behavior.** Sweeps must transfer to user; NFTs that can't be swept must be rescue-able (consider adding `ownerRescue(address token)` in a future PR).
8. **Tornado Cash and Privacy Pools are OUT OF SCOPE.** Do not add them. Do not add them to the allowlist.

---

## Rollback Plan Per Phase

Each phase is a self-contained PR. If a phase's fork tests regress in production:
- Revert the phase's PR; earlier phases stand independently.
- Phase 1 (router changes) is the only one requiring a redeploy to revert. Minimize Phase 1 ambitions.

---

## Time Estimate

| Phase | Estimate |
|---|---|
| 0: Pre-flight | 30 min |
| 1: Router foundations | 2–3 days |
| 2: Compiler fee awareness | 1 day |
| 3: Lido enhancements | 1 day |
| 4: Morpho | 2 days |
| 5: Balancer flashloan + looping | 4–5 days |
| 6: Uniswap V3 LP | 4 days |
| 7: Across bridging | 1 day |
| 8: Integration | 1 day |
| **Total** | **~3 weeks of focused work** |
