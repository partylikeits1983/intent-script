# Sub-Task 04 — Phase 3: Lido Enhancements

## Context

Three things ship together here because they all touch `adapters/lido.rs` and `ResolvedStep::Lido*`:

1. **Rename** the misnamed `lido` field in `ResolvedStep::LidoStake` to `steth` for clarity (not a functional change — see `00-corrections.md` §2).
2. Add **wstETH → stETH unwrap** (Lido has both wrapped and unwrapped staking tokens).
3. Add **Lido withdrawal queue**: `requestWithdrawals` (mints NFTs) and `claimWithdrawals` (burns NFTs, returns ETH).

## Prerequisites

- Sub-task 03 complete (`step_produces` has `fee_bps`).
- Sub-task 02 complete (`onERC721Received` exists — request-then-send chains will need it eventually).

## Files to read first

- `crates/intent-script/src/schema/public_ast.rs` — existing `StakeStep`, `UnwrapStep`.
- `crates/intent-script/src/ir/canonical.rs` — `LidoStake`, `WstETHWrap`, `step_produces`, `step_consumes`.
- `crates/intent-script/src/compiler/normalize.rs` — search `LidoStake`, `WstETHWrap`, `"wsteth"`, `"steth"`.
- `crates/intent-script/src/compiler/enrich.rs` — Lido branch (~lines 158-162).
- `crates/intent-script/src/adapters/lido.rs` — existing `lower_stake`, `lower_wsteth_wrap`.
- `config/protocols/ethereum.json` — add `withdrawal_queue` contract.

## Implementation

### 4.1 Rename `lido` → `steth` in `LidoStake`

In `ir/canonical.rs`, change:
```rust
LidoStake { lido: Address, amount: U256, referral: Address }
```
to:
```rust
LidoStake { steth: Address, amount: U256, referral: Address }
```

Update references in:
- `normalize.rs` construction site.
- `enrich.rs` Lido branch.
- `adapters/lido.rs::lower_stake` (sol! call target remains the same address — it's stETH that accepts `submit`).
- `step_produces` / `step_consumes`.

No behavior change. Tests should still pass.

### 4.2 wstETH → stETH unwrap

**DSL:** Extend `Unwrap` step so `{ "unwrap": { "asset": "wstETH", "amount": "…" } }` is legal.

**Normalize:** Branch on `asset`:
- `WETH` → existing `Unwrap` variant
- `wstETH` → new `WstETHUnwrap` variant

**IR:** Add to `canonical.rs`:
```rust
WstETHUnwrap { wsteth: Address, steth: Address, amount: U256 },
```

**Adapter `adapters/lido.rs`:** Add:
```rust
alloy_sol_types::sol! {
    function unwrap(uint256 _wstETHAmount) external returns (uint256);
}

pub fn lower_wsteth_unwrap(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    // call `unwrap(amount)` on `wsteth` address; returns stETH amount
}
```

**Dispatch:** wire into `adapters/mod.rs`.

**Enrich:** WstETHUnwrap consumes wstETH (auto-insert transferFrom; no approve — `unwrap` burns caller's wstETH). Produces stETH → add stETH to `sweep_tokens`.

**step_produces / step_consumes:** Add entries.

### 4.3 Lido withdrawal queue (request + claim)

**Config:** Add to `config/protocols/ethereum.json` Lido contracts:
```json
"lido": {
  …,
  "contracts": {
    "steth":            "0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84",
    "wsteth":           "0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0",
    "withdrawal_queue": "0x889edC2eDab5f40e902b864aD4d7AdE8E412F9B1"
  }
}
```

**DSL step variants:**
```rust
pub enum Step { …,
    RequestWithdrawal(RequestWithdrawalStep),
    ClaimWithdrawal(ClaimWithdrawalStep),
}

pub struct RequestWithdrawalStep {
    pub asset: String,        // "stETH" or "wstETH"
    pub amounts: Vec<String>, // one amount per NFT to mint
    pub from: String,         // must be "lido"
}

pub struct ClaimWithdrawalStep {
    pub protocol: String,      // must be "lido"
    pub request_ids: Vec<u64>, // NFT token ids
    pub hints: Vec<u64>,       // caller supplies from findCheckpointHints off-chain
}
```

**IR:**
```rust
LidoRequestWithdrawal { queue: Address, steth: Address, amounts: Vec<U256>, owner: Address },
LidoClaimWithdrawal   { queue: Address, request_ids: Vec<U256>, hints: Vec<U256> },
```

**Validate:**
- `hints.len() == request_ids.len()`.
- Reject empty arrays.
- `asset` ∈ {"stETH", "wstETH"}.
- `from` / `protocol` == "lido".

**Enrich:**
- `LidoRequestWithdrawal`: pull stETH or wstETH via transferFrom; approve queue; queue mints NFTs to `owner=signer` (router does not custody the NFT for the request itself).
- `LidoClaimWithdrawal`: no transferFrom (queue burns NFTs and sends ETH to signer). Signer must have pre-approved the queue to move the NFT *if* claiming from router — for v1 require `owner=signer` so router isn't involved.

**Adapter `adapters/lido.rs`:**
```rust
alloy_sol_types::sol! {
    function requestWithdrawals(uint256[] _amounts, address _owner) external returns (uint256[]);
    function requestWithdrawalsWstETH(uint256[] _amounts, address _owner) external returns (uint256[]);
    function claimWithdrawals(uint256[] _requestIds, uint256[] _hints) external;
}
```

Pick the right selector based on `asset`.

### 4.4 Tests

- `tests/integration.rs`: `test_lido_unwrap_wsteth`, `test_lido_request_withdrawal_steth`, `test_lido_claim_withdrawal`.
- `tests/generate_calldata.rs`: new fixture `lido_request_withdrawal.txt`.
- `contracts/test/LidoFork.t.sol` (NEW): fork mainnet, stake 1 ETH, request 0.5 stETH withdrawal, warp 7 days, claim, verify ETH returned.
- Check `tests/enricher_tests.rs` — if any assertion depended on the old `lido` field name, update.

### 4.5 Example

Add `crates/intent-script/examples/lido_request_withdrawal.json`:

```json
{
  "network": "ethereum",
  "from": "0x…",
  "steps": [
    { "request_withdrawal": { "asset": "stETH", "amounts": ["0.5"], "from": "lido" } }
  ]
}
```

## Definition of done

- [ ] `LidoStake` field renamed from `lido` to `steth`; all references updated.
- [ ] `wstETH` → `stETH` unwrap compiles end-to-end.
- [ ] Request / claim withdrawal steps compile and emit correct selectors.
- [ ] `config/protocols/ethereum.json` has `withdrawal_queue`.
- [ ] New fork test passes with `ETH_RPC_URL` set.
- [ ] `make test && make test-foundry` green.

## Verification

```bash
cd /Users/riemann/Desktop/intentOS/intent-script
make test && make test-foundry
ETH_RPC_URL=… cd contracts && forge test --mc LidoFork --fork-url $ETH_RPC_URL -vvv
```

## Hand-off to sub-task 05

- `step_produces` is fee-aware (from sub-task 03) and still applies to `LidoStake`.
- New IR variants (`WstETHUnwrap`, `LidoRequestWithdrawal`, `LidoClaimWithdrawal`) are examples of protocol-multi-contract patterns you may want to mirror when adding Morpho multi-market support.
