# Intent-Script: Next Steps

## Current State (v1)

Working end-to-end compiler with:
- **Actions**: `wrap`, `unwrap`, `deposit` (Aave V3)
- **Auto-enrichment**: ERC-20 approvals inserted automatically
- **Output**: Unsigned tx JSON (`SingleTx` or `TxSequence`)
- **Config**: JSON files for chains, assets, protocols (no recompile to extend)
- **Tests**: Unit tests, integration tests, Anvil fork tests (wrap ✅, Aave deposit ✅)

---

## How to Add New Commands

Every new command follows the same pattern:

### 1. Add the step type to the public AST

In [`crates/intent-script/src/schema/public_ast.rs`](../crates/intent-script/src/schema/public_ast.rs):

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    // ... existing variants ...
    Swap(SwapStep),        // ← add new variant
}

#[derive(Debug, Deserialize)]
pub struct SwapStep {
    pub from: String,      // input token alias
    pub amount: String,    // human-readable amount
    pub to: String,        // output token alias
}
```

### 2. Add the resolved step to the canonical IR

In [`crates/intent-script/src/ir/canonical.rs`](../crates/intent-script/src/ir/canonical.rs):

```rust
pub enum ResolvedStep {
    // ... existing variants ...
    UniswapV3Swap {
        router: Address,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee: u32,
        recipient: Address,
        deadline: U256,
        amount_out_minimum: U256,
    },
}
```

### 3. Add normalization logic

In [`crates/intent-script/src/compiler/normalize.rs`](../crates/intent-script/src/compiler/normalize.rs), add a match arm in `normalize_step()` that resolves aliases and parses amounts.

### 4. Add enrichment logic (if needed)

In [`crates/intent-script/src/compiler/enrich.rs`](../crates/intent-script/src/compiler/enrich.rs), insert any auto-generated steps (e.g., ERC-20 approve before a swap).

### 5. Create the adapter

Create a new file in `crates/intent-script/src/adapters/` that implements the ABI encoding:

```rust
// src/adapters/uniswap_v3.rs
alloy_sol_types::sol! {
    function exactInputSingle(ExactInputSingleParams params) external returns (uint256);
    // ...
}

pub fn lower_swap(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    // ABI-encode the call and return ConcreteCall
}
```

### 6. Register the adapter

In [`crates/intent-script/src/adapters/mod.rs`](../crates/intent-script/src/adapters/mod.rs), add the dispatch:

```rust
ResolvedStep::UniswapV3Swap { .. } => uniswap_v3::lower_swap(step),
```

### 7. Add protocol config

In `config/protocols/ethereum.json`:

```json
{
  "uniswap": {
    "type": "dex",
    "version": "v3",
    "contracts": {
      "router": "0xE592427A0AEce92De3Edee1F18E0157C05861564",
      "quoter": "0xb27308f9F90D607463bb33eA1BeBb41C27CE5AB6"
    }
  }
}
```

### 8. Write tests

- Unit test in the adapter file
- Integration test in `crates/intent-script/tests/integration.rs`
- Anvil fork test in `crates/evm-testing/tests/anvil_tests.rs`

---

## Planned Commands

### Uniswap V3 — Swap

**JSON**:
```json
{ "swap": { "from": "USDT", "amount": "10000", "to": "USDC" } }
```

**What the compiler does**:
1. Resolve token addresses and decimals
2. Look up Uniswap V3 router address from protocol config
3. Insert ERC-20 approve for the router
4. Encode `exactInputSingle` or `exactInput` (for multi-hop)
5. Apply default slippage from compiler policy (e.g., 50 bps)
6. Set deadline to `block.timestamp + 300`

**Adapter**: `src/adapters/uniswap_v3.rs`

**ABI**:
```solidity
struct ExactInputSingleParams {
    address tokenIn;
    address tokenOut;
    uint24 fee;
    address recipient;
    uint256 deadline;
    uint256 amountIn;
    uint256 amountOutMinimum;
    uint160 sqrtPriceLimitX96;
}
function exactInputSingle(ExactInputSingleParams params) external returns (uint256);
```

**Complexity**: Medium — need fee tier selection (500, 3000, 10000) and slippage calculation.

---

### Uniswap V3 — Add Liquidity

**JSON**:
```json
{ "add_liquidity": { "token_a": "USDC", "token_b": "ETH", "amount_a": "5000", "amount_b": "2.0", "into": "uniswap", "fee_tier": "3000" } }
```

**What the compiler does**:
1. Resolve both token addresses
2. Insert ERC-20 approves for both tokens to the NonfungiblePositionManager
3. Encode `mint()` call with tick range (full range for v1)
4. Handle native ETH wrapping if one token is ETH

**Adapter**: `src/adapters/uniswap_v3_lp.rs`

**ABI** (NonfungiblePositionManager):
```solidity
struct MintParams {
    address token0;
    address token1;
    uint24 fee;
    int24 tickLower;
    int24 tickUpper;
    uint256 amount0Desired;
    uint256 amount1Desired;
    uint256 amount0Min;
    uint256 amount1Min;
    address recipient;
    uint256 deadline;
}
function mint(MintParams params) external returns (uint256 tokenId, uint128 liquidity, uint256 amount0, uint256 amount1);
```

**Complexity**: High — tick math, token ordering (token0 < token1), slippage on both sides.

---

### Uniswap V3 — Modify Liquidity (Increase)

**JSON**:
```json
{ "increase_liquidity": { "position_id": "12345", "token_a": "USDC", "amount_a": "1000", "token_b": "ETH", "amount_b": "0.5", "into": "uniswap" } }
```

**ABI**:
```solidity
struct IncreaseLiquidityParams {
    uint256 tokenId;
    uint256 amount0Desired;
    uint256 amount1Desired;
    uint256 amount0Min;
    uint256 amount1Min;
    uint256 deadline;
}
function increaseLiquidity(IncreaseLiquidityParams params) external returns (uint128, uint256, uint256);
```

---

### Uniswap V3 — Remove Liquidity

**JSON**:
```json
{ "remove_liquidity": { "position_id": "12345", "liquidity": "all", "from": "uniswap" } }
```

**What the compiler does**:
1. Encode `decreaseLiquidity()` to remove liquidity
2. Encode `collect()` to withdraw the tokens
3. Output as `TxSequence` (2 txs from EOA)

**ABI**:
```solidity
function decreaseLiquidity(DecreaseLiquidityParams params) external returns (uint256, uint256);
function collect(CollectParams params) external returns (uint256, uint256);
```

---

### Uniswap V3 — Collect Fees

**JSON**:
```json
{ "collect_fees": { "position_id": "12345", "from": "uniswap" } }
```

**ABI**:
```solidity
struct CollectParams {
    uint256 tokenId;
    address recipient;
    uint128 amount0Max;
    uint128 amount1Max;
}
function collect(CollectParams params) external returns (uint256, uint256);
```

---

### Aave V3 — Borrow

Already implemented in the IR and adapter. Just needs the normalizer `Step::Borrow` arm to be connected (currently returns `UnsupportedStep` for safety).

**JSON**:
```json
{ "borrow": { "asset": "WBTC", "amount": "0.01", "from": "aave" } }
```

---

### Aave V3 — Withdraw

Already implemented in the IR and adapter.

**JSON**:
```json
{ "withdraw": { "asset": "USDC", "amount": "1000", "from": "aave" } }
```

---

### Generic ERC-20 Transfer

**JSON**:
```json
{ "transfer": { "asset": "USDC", "amount": "100", "to": "0xRecipient" } }
```

Simple — just `token.transfer(to, amount)`.

---

### Custom ABI Call (Escape Hatch)

**JSON**:
```json
{
  "custom": {
    "to": "0xContractAddress",
    "abi": "function doSomething(uint256 x, address y)",
    "args": [42, "0xRecipient"],
    "value": "0"
  }
}
```

This is the escape hatch for any contract interaction not covered by built-in adapters.

---

## Architecture Improvements

### Compiler Policy

Add a `CompilerPolicy` config that controls defaults without changing the JSON:

```rust
struct CompilerPolicy {
    default_slippage_bps: u32,        // e.g., 50 (0.5%)
    default_deadline_seconds: u64,     // e.g., 300 (5 min)
    approval_policy: ApprovalPolicy,   // MaxUint256 vs exact amount
    swap_venue: SwapVenuePolicy,       // Uniswap, 1inch, etc.
}
```

### Multi-chain Support

Add config files for more chains:
- `config/assets/base.json`
- `config/assets/arbitrum.json`
- `config/protocols/base.json`
- `config/protocols/arbitrum.json`

### Quoter Integration (Future)

For swaps, integrate with on-chain quoters or off-chain APIs to:
- Get expected output amounts
- Calculate slippage bounds
- Select optimal fee tiers
- Route through multiple pools

### Smart Account Support (Future)

For `RequiresExecutor` output, support:
- ERC-4337 UserOperations
- Safe multisig batched transactions
- Custom executor contracts that can batch approve+swap+deposit atomically

---

## Priority Order

1. **Uniswap V3 Swap** — most requested DeFi action
2. **Aave Borrow/Withdraw** — already half-implemented
3. **ERC-20 Transfer** — trivial to add
4. **Uniswap V3 LP operations** — complex but high value
5. **Custom ABI call** — escape hatch for everything else
6. **Compiler Policy** — needed before production use
7. **Multi-chain configs** — needed for Base/Arbitrum support
