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

### `unwrap` — Unwrap WETH

```json
{ "unwrap": { "asset": "WETH", "amount": "2.0" } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `asset` | string | ✅ | Wrapped token alias (e.g., `"WETH"`) |
| `amount` | string | ✅ | Human-readable amount, or `"all"` |

### `stake` — Lido Staking

```json
{ "stake": { "asset": "ETH", "amount": "10.0", "into": "lido" } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `asset` | string | ✅ | Asset to stake (e.g., `"ETH"`) |
| `amount` | string | ✅ | Human-readable amount, or `"all"` |
| `into` | string | ✅ | Protocol name (e.g., `"lido"`) |

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
