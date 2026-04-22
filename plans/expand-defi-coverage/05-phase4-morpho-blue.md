# Sub-Task 05 — Phase 4: Morpho Blue

## Context

Add Morpho Blue as a second lending protocol, exercising a new pattern: **markets are config-driven**. Each market is identified by a 32-byte id (`keccak256(abi.encode(MarketParams))`), precomputed at config-authoring time to keep the compiler deterministic and offline.

## Prerequisites

- Sub-task 03 complete (`step_produces` is fee-aware).

## Files to read first

- `crates/intent-script/src/adapters/aave_v3.rs` — mirror its shape for Morpho.
- `crates/intent-script/src/ir/canonical.rs` — `AaveV3Supply`, `AaveV3Borrow`, `AaveV3Withdraw` variants as reference.
- `crates/intent-script/src/compiler/enrich.rs` — AaveV3 branches.
- `crates/intent-script/src/registry/loader.rs` — `ProtocolConfig` shape; add a `markets` field.
- `crates/intent-script/src/schema/public_ast.rs` — `DepositStep`, `BorrowStep`, `WithdrawStep`.

## Implementation

### 5.1 Config

`config/protocols/ethereum.json` — new `morpho` entry:

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
  }
}
```

The `id` is `keccak256(abi.encode(MarketParams))`. Compute with `cast keccak` during authoring; store for determinism.

Add `pub markets: Option<BTreeMap<String, MorphoMarketConfig>>` to the protocol config struct (serde-optional).

### 5.2 DSL

Reuse existing `deposit`, `borrow`, `withdraw` with `into`/`from: "morpho"` plus a required `market` field. Add optional `as: "collateral"` to distinguish collateral supply from loan supply:

```json
{ "deposit": { "asset": "WETH", "amount": "1.0", "into": "morpho",
               "market": "USDC-WETH-86", "as": "collateral" } }
{ "deposit": { "asset": "USDC", "amount": "1000", "into": "morpho",
               "market": "USDC-WETH-86" } }
{ "borrow":  { "asset": "USDC", "amount": "500", "from": "morpho",
               "market": "USDC-WETH-86" } }
```

### 5.3 Schema

```rust
pub struct DepositStep {
    pub asset: String, pub amount: String, pub into: String,
    #[serde(default)] pub market: Option<String>,
    #[serde(default)] pub r#as: Option<String>,  // "collateral" | None
}
// Same `market` field on BorrowStep and WithdrawStep; no `as` on borrow/withdraw.
```

### 5.4 IR

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

Six new `ResolvedStep` variants:
```rust
MorphoSupply         { pool, market_params, amount, on_behalf: Address },
MorphoSupplyCollat   { pool, market_params, amount, on_behalf: Address },
MorphoBorrow         { pool, market_params, amount, on_behalf: Address, receiver: Address },
MorphoWithdraw       { pool, market_params, amount, on_behalf: Address, receiver: Address },
MorphoWithdrawCollat { pool, market_params, amount, on_behalf: Address, receiver: Address },
MorphoRepay          { pool, market_params, amount, on_behalf: Address },
```

### 5.5 Normalize

- When `into == "morpho"` or `from == "morpho"`, look up `market` in `protocols.morpho.markets`. Reject if missing.
- Validate: for supply-as-loan / borrow / repay, `asset` must match `loan`. For supply-as-collateral, `asset` must match `collateral`.
- Emit the right Morpho variant.
- Propagate fee_bps to `step_produces` for borrow/withdraw/withdrawCollat (they produce tokens swept by the router).

### 5.6 Validate

- Reject `as == "collateral"` on borrow/repay/withdraw-loan steps (only valid on supply).
- Reject `asset` not matching the market's loan or collateral token.

### 5.7 Enrich

- Supply / SupplyCollat / Repay: auto-insert transferFrom + approve(pool, amount).
- Borrow / Withdraw / WithdrawCollat: `receiver=router` when batched; add output asset to `sweep_tokens`.

### 5.8 Adapter `adapters/morpho.rs` (NEW)

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

All calls pass `shares=0` and `data=""` (no Morpho callbacks for v1).

### 5.9 Dispatch

`adapters/mod.rs` — register the six new arms.

### 5.10 Tests

- `tests/integration.rs`: `test_morpho_supply_collateral_and_borrow`.
- `tests/generate_calldata.rs`: `morpho_supply_usdc.txt`, `morpho_borrow_usdc_against_weth.txt`.
- `contracts/test/MorphoFork.t.sol` (NEW): fork mainnet, supply WETH as collateral, borrow USDC, verify user USDC increases by (borrowed − fee).

### 5.11 Allowlist

Morpho Blue pool is a new allowlisted target. Add it to any deploy script that seeds the allowlist.

### 5.12 Example

`crates/intent-script/examples/morpho_collateral_borrow.json`:
```json
{
  "network": "ethereum",
  "from": "0x…",
  "steps": [
    { "deposit": { "asset": "WETH", "amount": "1.0", "into": "morpho", "market": "USDC-WETH-86", "as": "collateral" } },
    { "borrow":  { "asset": "USDC", "amount": "1500", "from": "morpho", "market": "USDC-WETH-86" } }
  ]
}
```

## Definition of done

- [ ] Six new `Morpho*` IR variants compile and dispatch correctly.
- [ ] Config loader reads `markets` with `id`, `loan`, `collateral`, `oracle`, `irm`, `lltv`.
- [ ] `make test && make test-foundry` green.
- [ ] `MorphoFork.t.sol` passes with `ETH_RPC_URL`.
- [ ] Morpho pool added to allowlist in deploy script.

## Verification

```bash
cd /Users/riemann/Desktop/intentOS/intent-script
make test && make test-foundry
ETH_RPC_URL=… cd contracts && forge test --mc MorphoFork --fork-url $ETH_RPC_URL -vvv
```

## Hand-off to sub-task 06

- The config-keyed-market pattern you built here is the reference for any future lending protocol.
- `MorphoMarketParams` is a shared struct; reuse it (don't duplicate) if sub-task 06 or later needs to compose flashloans with Morpho supply.
