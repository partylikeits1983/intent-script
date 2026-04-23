# Intent-Script JSON DSL Specification

> **Load this file** when you need to understand the JSON input format accepted by the compiler, including all step types, field requirements, amount syntax, and validation rules.

## Top-Level Structure

```json
{
  "network": "ethereum",
  "from": "0x...",
  "steps": [ ... ],
  "nonce": 0,
  "deadline": 1712345678,
  "current_timestamp": 1712344000,
  "balances": { ... }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `network` | string | ✅ | Network alias: `"ethereum"`, `"base"`, `"arbitrum"` |
| `from` | string | ✅ | Signer EOA address (hex, checksummed) |
| `steps` | array | ✅ | Ordered list of action steps (1–5 steps) |
| `nonce` | number | ❌ | EIP-712 replay protection nonce (default: 0) |
| `deadline` | number | ❌ | EIP-712 expiry as Unix timestamp |
| `current_timestamp` | number | ❌ | Current Unix timestamp for deadline computation |
| `balances` | object | ❌ | User's on-chain balances for enhanced validation |

**Serde types:** `IntentScript` in `crates/intent-script/src/schema/public_ast.rs:14`

---

## Step Types

Each step is a JSON object with exactly one key (the action name) mapping to its parameters. The `Step` enum is in `crates/intent-script/src/schema/public_ast.rs:70`.

### `swap` — Token Swap

```json
{ "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "min_amount_out": "0.48" } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `from` | string | ✅ | Input token alias |
| `amount` | string | ✅ | Human-readable input amount, or `"all"` |
| `to` | string | ✅ | Output token alias |
| `min_amount_out` | string | ✅* | Minimum output amount (slippage protection) |
| `price` | string | ❌ | Market price (output per 1 input). Alternative to `min_amount_out` |
| `slippage` | string | ❌ | Max slippage % (default: 0.5 when `price` set) |
| `fee` | string | ❌ | Uniswap V3 fee tier (default: `"3000"`) |
| `via` | string | ❌ | Router: `"uniswap"` (default) or `"1inch"` |
| `calldata` | string | ❌ | Pre-fetched calldata (required for `via: "1inch"`) |
| `deadline` | number | ❌ | Swap-specific deadline as Unix timestamp |

*Either `min_amount_out` or `price` must be provided. Zero slippage protection is rejected by the validator.

**Slippage precedence:** `min_amount_out` > `price`+`slippage` > nothing (warns + defaults to 0).

**Slippage examples:**
```json
{ "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "min_amount_out": "0.48" } }
{ "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "price": "0.0005", "slippage": "1.0" } }
{ "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "price": "0.0005" } }
```

> **Note:** 1inch swaps handle slippage via the 1inch Fusion protocol, not via these fields.

### `deposit` — Aave V3 Supply

```json
{ "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `asset` | string | ✅ | Token alias to deposit |
| `amount` | string | ✅ | Human-readable amount, or `"all"` |
| `into` | string | ✅ | Protocol name (e.g., `"aave"`) |

### `borrow` — Aave V3 Borrow

```json
{ "borrow": { "asset": "DAI", "amount": "2000", "from": "aave" } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `asset` | string | ✅ | Token alias to borrow |
| `amount` | string | ✅ | Human-readable amount |
| `from` | string | ✅ | Protocol name (e.g., `"aave"`) |

### `withdraw` — Aave V3 Withdraw

```json
{ "withdraw": { "asset": "USDC", "amount": "5000", "from": "aave" } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `asset` | string | ✅ | Token alias to withdraw |
| `amount` | string | ✅ | Human-readable amount, or `"all"` |
| `from` | string | ✅ | Protocol name (e.g., `"aave"`) |

### `wrap` — Wrap Native or stETH

```json
{ "wrap": { "asset": "ETH", "amount": "1.5" } }
{ "wrap": { "asset": "stETH", "amount": "10.0" } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `asset` | string | ✅ | `"ETH"` (→ WETH) or `"stETH"` (→ wstETH) |
| `amount` | string | ✅ | Human-readable amount, or `"all"` |

The normalizer detects `"stETH"` and produces `WstETHWrap` instead of `Wrap`.

### `unwrap` — Unwrap WETH or wstETH

```json
{ "unwrap": { "asset": "WETH",   "amount": "2.0" } }
{ "unwrap": { "asset": "wstETH", "amount": "1.0" } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `asset` | string | ✅ | `"WETH"` (→ ETH) or `"wstETH"` (→ stETH) |
| `amount` | string | ✅ | Human-readable amount, or `"all"` |

Normalizer branches on `asset`: `"wstETH"` produces `WstETHUnwrap` (calls `wstETH.unwrap(uint256)` which burns wstETH and returns stETH).

### `stake` — Lido Staking

```json
{ "stake": { "asset": "ETH", "amount": "10.0", "into": "lido" } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `asset` | string | ✅ | Asset to stake (e.g., `"ETH"`) |
| `amount` | string | ✅ | Human-readable amount, or `"all"` |
| `into` | string | ✅ | Protocol name (e.g., `"lido"`) |

### `request_withdrawal` — Lido Withdrawal Queue Request

Burns stETH or wstETH and mints one NFT per requested amount. Claim with `claim_withdrawal` once the withdrawal is finalized on the Lido queue.

```json
{ "request_withdrawal": { "asset": "stETH",  "amounts": ["5.0", "3.0"], "from": "lido" } }
{ "request_withdrawal": { "asset": "wstETH", "amounts": ["1.0"],         "from": "lido" } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `asset` | string | ✅ | `"stETH"` or `"wstETH"` |
| `amounts` | string[] | ✅ | One amount per NFT to mint; each element is human-readable |
| `from` | string | ✅ | Protocol name (must be `"lido"`) |

Each amount must be at least `MIN_STETH_WITHDRAWAL_AMOUNT = 100 wei` and at most `MAX_STETH_WITHDRAWAL_AMOUNT = 1000 stETH` (Lido protocol limits). The NFTs are minted to the signer, not the router.

### `claim_withdrawal` — Lido Withdrawal Queue Claim

Burns withdrawal NFTs and sends ETH back to the signer. Requires that the queue has finalized the requests (off-chain polling) and that `hints` are obtained via `WithdrawalQueue.findCheckpointHints(requestIds, firstIndex, lastIndex)`.

```json
{
  "claim_withdrawal": {
    "protocol": "lido",
    "request_ids": [12345, 12346],
    "hints":       [42,    42]
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `protocol` | string | ✅ | Protocol name (must be `"lido"`) |
| `request_ids` | number[] | ✅ | NFT ids returned by a prior `request_withdrawal` |
| `hints` | number[] | ✅ | Checkpoint hints, same length as `request_ids` |

The caller (signer or router) must own the NFTs. ETH is sent to the caller, not `onBehalfOf`.

### `lp_mint` — Uniswap V3 LP Mint

Mints a new concentrated-liquidity position NFT via the `NonfungiblePositionManager`.

```json
{
  "lp_mint": {
    "protocol": "uniswap",
    "token0": "USDC",
    "token1": "WETH",
    "fee": "3000",
    "tick_lower": -887220,
    "tick_upper":  887220,
    "amount0": "1000",
    "amount1": "0.5",
    "min_amount0": "990",
    "min_amount1": "0.495"
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `protocol` | string | ✅ | Must be `"uniswap"` in v1 |
| `token0` | string | ✅ | First token alias — must be the lexicographically smaller address |
| `token1` | string | ✅ | Second token alias — the larger address |
| `fee` | string | ✅ | Uniswap V3 fee tier: `"500"`, `"3000"`, or `"10000"` |
| `tick_lower` | number | ✅ | Lower price bound; must be a multiple of the tick spacing |
| `tick_upper` | number | ✅ | Upper price bound; must be `> tick_lower` and `≤ MAX_TICK` |
| `amount0` | string | ✅ | Desired token0 deposit (human-readable) |
| `amount1` | string | ✅ | Desired token1 deposit (human-readable) |
| `min_amount0` | string | ✅ | Minimum token0 to deposit (slippage protection) |
| `min_amount1` | string | ✅ | Minimum token1 to deposit (slippage protection) |
| `deadline` | number | ❌ | Swap-specific deadline as Unix timestamp |

**Token ordering constraint:** Uniswap V3 requires `token0 < token1` by address. If the user provides them in the wrong order the normalizer swaps them along with their amounts/mins and emits a warning.

**Tick spacing** per fee tier: 500→10, 3000→60, 10000→200. `MAX_TICK` is `887272`; `MIN_TICK` is `-887272`. Full-range positions typically use `±887220` (a multiple of 60).

### `lp_increase` — Uniswap V3 LP Increase Liquidity

Adds liquidity to an existing NFT position.

```json
{
  "lp_increase": {
    "position_id": "12345",
    "token0": "USDC",
    "token1": "WETH",
    "amount0": "500",
    "amount1": "0.25",
    "min_amount0": "495",
    "min_amount1": "0.247"
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `position_id` | string | ✅ | NFT token id as a decimal string |
| `token0` | string | ✅ | Position's token0 alias (must match on-chain) |
| `token1` | string | ✅ | Position's token1 alias |
| `amount0`, `amount1` | string | ✅ | Desired deposits (human-readable) |
| `min_amount0`, `min_amount1` | string | ✅ | Minimum deposits (slippage protection) |
| `deadline` | number | ❌ | Deadline as Unix timestamp |

`token0`/`token1` are required because the compiler has no RPC to introspect the position's metadata. The user must declare the correct pair so enrichment can emit approvals for the right ERC-20s.

### `lp_decrease` — Uniswap V3 LP Decrease Liquidity

Burns part (or all) of a position's liquidity. Proceeds accumulate as owed tokens on the position — use `lp_collect` to withdraw them.

```json
{
  "lp_decrease": {
    "position_id": "12345",
    "token0": "USDC",
    "token1": "WETH",
    "liquidity": "1000000",
    "min_amount0": "495",
    "min_amount1": "0.247"
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `position_id` | string | ✅ | NFT token id |
| `token0` | string | ✅ | Position's token0 alias (needed to parse `min_amount0` with the correct decimals) |
| `token1` | string | ✅ | Position's token1 alias |
| `liquidity` | string | ✅ | u128 liquidity amount to remove, or `"all"` |
| `min_amount0`, `min_amount1` | string | ✅ | Minimum token amounts returned (slippage protection) |
| `deadline` | number | ❌ | Deadline as Unix timestamp |

### `lp_collect` — Uniswap V3 LP Collect

Collects all owed tokens (accumulated fees + proceeds from `lp_decrease`) from a position. Always uses `type(uint128).max` for both amounts to mean "all owed".

```json
{
  "lp_collect": {
    "position_id": "12345",
    "token0": "USDC",
    "token1": "WETH"
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `position_id` | string | ✅ | NFT token id |
| `token0` | string | ✅ | Position's token0 alias (needed so enrich can add the pair to sweep) |
| `token1` | string | ✅ | Position's token1 alias |

### `send` — Transfer Tokens/ETH/NFTs

**ERC-20 send:**
```json
{ "send": { "asset": "USDC", "amount": "100", "to": "0x..." } }
```

**ETH send:**
```json
{ "send": { "asset": "ETH", "amount": "1.0", "to": "0x..." } }
```

**ERC-721 send:**
```json
{ "send": { "asset_type": "erc721", "contract": "0x...", "token_id": "42", "to": "0x..." } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `asset` | string | ✅* | Token alias (ERC-20/ETH) |
| `amount` | string | ✅* | Human-readable amount, or `"all"` |
| `to` | string | ✅ | Recipient address |
| `asset_type` | string | ❌ | `"erc20"` (default) or `"erc721"` |
| `contract` | string | ❌ | NFT contract address (erc721 only) |
| `token_id` | string | ❌ | NFT token ID (erc721 only) |

*Required for ERC-20/ETH sends. For ERC-721, `contract` and `token_id` are required instead.

---

## Amount Syntax

Amounts are human-readable strings with decimal support:

| Example | Meaning |
|---------|---------|
| `"1000"` | 1000 tokens (integer) |
| `"1.5"` | 1.5 tokens (decimal) |
| `"0.01"` | 0.01 tokens |
| `"all"` | Use the full guaranteed output from the previous step that produces this token |

The `"all"` keyword resolves at compile time (in `normalize.rs`) to the guaranteed minimum output of the most recent prior step that produces the same token. It cannot be used on the first step or when no prior step produces the token.

Amount parsing is in `crates/intent-script/src/compiler/normalize.rs` — the `parse_amount()` and `resolve_amount_or_all()` functions.

---

## Balances (Optional)

```json
{
  "balances": {
    "tokens": { "USDC": "50000.0", "WETH": "10.0" },
    "aave_positions": {
      "supplied": { "USDC": "50000.0" },
      "borrowed": { "DAI": "5000.0" },
      "health_factor": "1.85"
    }
  }
}
```

When provided, the compiler performs stricter validation:
- Borrows without collateral are rejected (not just warned)
- Withdrawals without positions are rejected
- Health factor < 1.2 rejects borrows
- Health factor < 1.5 produces a warning

**Serde types:** `UserBalances`, `AavePositions` in `crates/intent-script/src/schema/public_ast.rs:38`

---

## Validation Rules

Implemented in `crates/intent-script/src/compiler/validate.rs`:

1. **Signer**: Must be a valid non-zero Ethereum address
2. **Steps**: 1–5 steps required (max 5, `MAX_STEPS` constant)
3. **Amounts**: Must be positive (> 0)
4. **Slippage**: Swaps must have `min_amount_out` or `price`+`slippage`; zero `amount_out_minimum` is rejected
5. **Asset compatibility**: No native ETH into Aave; no swap-to-self
6. **Amount flow**: Cross-step token consumption cannot exceed guaranteed production
7. **Health factor**: Borrows rejected when Aave HF < 1.2
8. **Send targets**: Cannot send to the zero address

---

## Complete Examples

### Swap and Deposit
```json
{
  "network": "ethereum",
  "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
  "current_timestamp": 1712344000,
  "steps": [
    { "swap": { "from": "USDC", "amount": "5000", "to": "WETH", "min_amount_out": "2.0" } },
    { "deposit": { "asset": "WETH", "amount": "all", "into": "aave" } }
  ]
}
```

### Stake and Wrap
```json
{
  "network": "ethereum",
  "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
  "steps": [
    { "stake": { "asset": "ETH", "amount": "10.0", "into": "lido" } },
    { "wrap": { "asset": "stETH", "amount": "all" } }
  ]
}
```

### Complex DeFi (Swap → Deposit → Borrow)
```json
{
  "network": "ethereum",
  "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
  "current_timestamp": 1712344000,
  "steps": [
    { "swap": { "from": "USDC", "amount": "5000", "to": "WETH", "min_amount_out": "2.0" } },
    { "deposit": { "asset": "WETH", "amount": "2.0", "into": "aave" } },
    { "borrow": { "asset": "DAI", "amount": "1000", "from": "aave" } }
  ]
}
```

### Send USDC
```json
{
  "network": "ethereum",
  "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
  "steps": [
    { "send": { "asset": "USDC", "amount": "100", "to": "0x1234567890abcdef1234567890abcdef12345678" } }
  ]
}
```

### Lido Withdrawal Queue Round-Trip
```json
{
  "network": "ethereum",
  "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
  "steps": [
    { "request_withdrawal": { "asset": "stETH", "amounts": ["5.0"], "from": "lido" } }
  ]
}
```

```json
{
  "network": "ethereum",
  "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
  "steps": [
    { "claim_withdrawal": { "protocol": "lido", "request_ids": [12345], "hints": [42] } }
  ]
}
```

### Uniswap V3 LP Mint (Full Range)
```json
{
  "network": "ethereum",
  "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
  "steps": [
    {
      "lp_mint": {
        "protocol": "uniswap",
        "token0": "USDC",
        "token1": "WETH",
        "fee": "3000",
        "tick_lower": -887220,
        "tick_upper":  887220,
        "amount0": "1000",
        "amount1": "0.3",
        "min_amount0": "990",
        "min_amount1": "0.297"
      }
    }
  ]
}
```

### LP Decrease + Collect
```json
{
  "network": "ethereum",
  "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
  "steps": [
    {
      "lp_decrease": {
        "position_id": "12345",
        "token0": "USDC",
        "token1": "WETH",
        "liquidity": "all",
        "min_amount0": "950",
        "min_amount1": "0.28"
      }
    },
    {
      "lp_collect": {
        "position_id": "12345",
        "token0": "USDC",
        "token1": "WETH"
      }
    }
  ]
}
```
