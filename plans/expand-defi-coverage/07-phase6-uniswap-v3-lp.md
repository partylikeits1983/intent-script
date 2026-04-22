# Sub-Task 07 — Phase 6: Uniswap V3 LP

## Context

Add the full LP lifecycle on Uniswap V3 via the NonfungiblePositionManager (NPM): mint, increase, decrease, collect. Exercises the NFT custody pattern enabled by sub-task 02's `onERC721Received`.

## Prerequisites

- Sub-task 02 complete (router has `onERC721Received`).
- Sub-task 03 complete (`step_produces` fee-aware).

## Files to read first

- `crates/intent-script/src/adapters/uniswap_v3.rs` — existing swap adapter; LP adapter will be a sibling.
- Uniswap V3 NonfungiblePositionManager source for signatures of `mint`, `increaseLiquidity`, `decreaseLiquidity`, `collect`.
- `crates/intent-script/src/compiler/enrich.rs` — observe dual-token transferFrom patterns.

## Implementation

### 7.1 Config

Extend the `uniswap` entry in `config/protocols/ethereum.json`:
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

### 7.2 DSL

```json
{ "lp_mint": { "protocol": "uniswap",
               "token0": "USDC", "token1": "WETH",
               "fee": "3000",
               "tick_lower": -200040, "tick_upper": -199980,
               "amount0": "1000", "amount1": "0.3",
               "min_amount0": "990", "min_amount1": "0.29" } }
{ "lp_increase": { "position_id": "12345",
                   "amount0": "500", "amount1": "0.15",
                   "min_amount0": "495", "min_amount1": "0.148" } }
{ "lp_decrease": { "position_id": "12345", "liquidity": "all",
                   "min_amount0": "950", "min_amount1": "0.28" } }
{ "lp_collect":  { "position_id": "12345" } }
```

**Constraint:** `position_id` MUST be an explicit integer. No `"last_minted"` sugar — that would introduce mid-batch state which breaks the linear step model. Mint-then-collect must be two separate intents.

### 7.3 Schema + IR

```rust
pub enum Step { …, LpMint(LpMintStep), LpIncrease(LpIncreaseStep),
                LpDecrease(LpDecreaseStep), LpCollect(LpCollectStep) }

pub struct LpMintStep {
    pub protocol: String,
    pub token0: String, pub token1: String,
    pub fee: String,                              // "500" | "3000" | "10000"
    pub tick_lower: i32, pub tick_upper: i32,
    pub amount0: String, pub amount1: String,
    pub min_amount0: String, pub min_amount1: String,
    #[serde(default)] pub deadline: Option<u64>,
}
// LpIncreaseStep / LpDecreaseStep / LpCollectStep similarly
```

IR:
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
UniswapV3LpDecrease { npm, token_id: U256, liquidity: u128, amount0_min, amount1_min, deadline },
UniswapV3LpCollect  { npm, token_id: U256, recipient: Address, amount0_max: u128, amount1_max: u128 },
```

### 7.4 Normalize

- Lexicographically sort `(token0, token1)` by address. If caller provides them reversed, swap addresses + swap amounts/mins to match NPM's invariant.
- Validate `fee ∈ {500, 3000, 10000}`.
- Validate `tick_lower < tick_upper` and both are multiples of `tickSpacing(fee)`:
  ```rust
  fn tick_spacing(fee: u32) -> i32 {
      match fee { 500 => 10, 3000 => 60, 10000 => 200, _ => unreachable!() }
  }
  ```
- Disallow `"all"` on LP amount fields for v1.
- `recipient` on mint: **`recipient = signer`** so the NFT goes straight to the user (router never custodies it). See enrich note below.
- `recipient` on decrease/collect: `signer`. The NPM requires the caller to own or be approved for `tokenId`; see prerequisite below.

### 7.5 Validate

- `amount0_min > 0 || amount1_min > 0` on mint/increase (slippage protection).
- `amount0_min > 0 || amount1_min > 0` on decrease.

### 7.6 Enrich

**Mint / Increase:**
- Auto-insert transferFrom for BOTH token0 and token1.
- Auto-insert approve(npm, amount) for BOTH tokens.
- Mint: recipient = signer → NFT never enters router → NO sweep step for the NFT.
- Increase: no NFT transfer at all (existing position stays where it is).
- Any leftover token0/token1 dust from slippage lands in the router (NPM sends `amount0Desired − amount0Used` back to `msg.sender`). Add both tokens to `sweep_tokens`.

**Decrease / Collect:**
- The NPM requires router to be owner-or-approved of the NFT. User must `NPM.approve(router, tokenId)` as a pre-intent (analogous to Aave credit delegation). **Document this loudly in the skills file during sub-task 09.**
- For decrease: tokens end up as uncollected fees inside the position. The user's pattern is `decrease → collect` in one intent. For v1, decrease alone does nothing user-visible — the parent plan accepts this; keep it simple.
- For collect: NPM sends token0/token1 to `recipient=router`; add both to `sweep_tokens`.

### 7.7 Adapter `adapters/uniswap_v3_lp.rs` (NEW)

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

Use `type(uint128).max` (i.e. `u128::MAX`) for `amount0Max/amount1Max` in collect (standard pattern — collect everything).

### 7.8 Dispatch

`adapters/mod.rs` — register four new arms.

### 7.9 Tests

- `tests/integration.rs`: `test_lp_mint_compiles`, `test_lp_increase`, `test_lp_decrease_collect`.
- `tests/enricher_tests.rs`: verify dual-token transferFrom + approves inserted; verify `recipient=signer` on mint.
- `tests/generate_calldata.rs`: `lp_mint_usdc_eth.txt`, `lp_rebalance_range.txt` (3-step: decrease → collect → mint).
- `contracts/test/LpFork.t.sol` (NEW): fork mainnet, mint USDC/WETH LP at 0.3%, increase, decrease, collect. Verify token returns minus fee.

### 7.10 Allowlist

Add NPM to deploy allowlist.

## Definition of done

- [ ] Four `UniswapV3Lp*` IR variants compile and dispatch.
- [ ] `recipient=signer` on mint (NFT bypasses router).
- [ ] `make test && make test-foundry` green.
- [ ] `LpFork.t.sol` passes.

## Verification

```bash
cd /Users/riemann/Desktop/intentOS/intent-script
make test && make test-foundry
ETH_RPC_URL=… cd contracts && forge test --mc LpFork --fork-url $ETH_RPC_URL -vvv
```

## Hand-off to sub-task 08

- The "user must pre-approve router for this NFT" pattern is now established. Future NFT-owning protocols will mirror it.
- The `send` step (already supported — see `00-corrections.md` §1) can be used to move an NFT from user to router if needed, but for LP, `recipient=signer` on mint avoids the round-trip.
