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

## DSL at a glance

One-line index into the step types detailed below. Each row shows the primitive, what it's for, and a minimal JSON shape — follow the links for full field tables and variations.

| Primitive | Purpose | Minimal shape |
|---|---|---|
| [`swap`](#swap--token-swap) | Uniswap V3 or 1inch token swap | `{ "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "min_amount_out": "0.48" } }` |
| [`deposit`](#deposit--aave-v3-or-morpho-blue-supply) | Supply to Aave V3 or Morpho Blue | `{ "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }` |
| [`borrow`](#borrow--aave-v3-or-morpho-blue-borrow) | Borrow against existing collateral | `{ "borrow": { "asset": "DAI", "amount": "2000", "from": "aave" } }` |
| [`withdraw`](#withdraw--aave-v3-or-morpho-blue-withdraw) | Pull supplied collateral back | `{ "withdraw": { "asset": "USDC", "amount": "5000", "from": "aave" } }` |
| [`wrap`](#wrap--wrap-native-or-steth) | ETH → WETH or stETH → wstETH | `{ "wrap": { "asset": "ETH", "amount": "1.5" } }` |
| [`unwrap`](#unwrap--unwrap-weth-or-wsteth) | WETH → ETH or wstETH → stETH | `{ "unwrap": { "asset": "WETH", "amount": "2.0" } }` |
| [`stake`](#stake--lido-staking) | Lido native-ETH staking (no wrap) | `{ "stake": { "asset": "ETH", "amount": "10.0", "into": "lido" } }` |
| [`request_withdrawal`](#request_withdrawal--lido-withdrawal-queue-request) | Queue stETH/wstETH for unstaking | `{ "request_withdrawal": { "asset": "stETH", "amounts": ["0.5"], "from": "lido" } }` |
| [`claim_withdrawal`](#claim_withdrawal--lido-withdrawal-queue-claim) | Redeem a mature Lido NFT | `{ "claim_withdrawal": { "protocol": "lido", "request_ids": [1234], "hints": [42] } }` |
| [`lp_mint`](#lp_mint--uniswap-v3-lp-mint) | New Uni V3 concentrated position | `{ "lp_mint": { "protocol": "uniswap", "token0": "USDC", "token1": "WETH", "fee": "3000", "tick_lower": -200040, "tick_upper": -199980, "amount0": "1000", "amount1": "0.3", "min_amount0": "990", "min_amount1": "0.29" } }` |
| [`lp_increase`](#lp_increase--uniswap-v3-lp-increase-liquidity) | Add liquidity to existing LP NFT | `{ "lp_increase": { "position_id": "123", "token0": "USDC", "token1": "WETH", "amount0": "500", "amount1": "0.15", "min_amount0": "495", "min_amount1": "0.148" } }` |
| [`lp_decrease`](#lp_decrease--uniswap-v3-lp-decrease-liquidity) | Remove liquidity (usually + collect) | `{ "lp_decrease": { "position_id": "123", "token0": "USDC", "token1": "WETH", "liquidity": "1000000000000000000", "min_amount0": "495", "min_amount1": "0.148" } }` |
| [`lp_collect`](#lp_collect--uniswap-v3-lp-collect) | Sweep owed tokens/fees | `{ "lp_collect": { "position_id": "123", "token0": "USDC", "token1": "WETH" } }` |
| [`send`](#send--transfer-tokensethnfts) | ERC-20 / ETH / ERC-721 transfer | `{ "send": { "asset": "USDC", "amount": "100", "to": "0x..." } }` |
| [`bridge`](#bridge--across-v3-cross-chain-transfer) | Across V3 source-chain deposit | `{ "bridge": { "via": "across", "asset": "USDC", "amount": "1000", "to_chain": "arbitrum", "recipient": "0x...", "relayer_fee_bps": "5" } }` |
| [`flashloan`](#flashloan--balancer-v2-flashloan-with-inner-pipeline) | Balancer V2 flashloan wrapping an inner pipeline | `{ "flashloan": { "via": "balancer", "assets": [{"asset":"WETH","amount":"2.0"}], "then": [ ... ] } }` |
| [`long`](#long--short--leverage-open) | **Aave-backed** leverage open (sugar) | `{ "long": { "collateral": "WETH", "borrow": "USDC", "amount": "1.0", "leverage": "5", "slippage": "50", "price": "3200" } }` |
| [`short`](#long--short--leverage-open) | **Aave-backed** leverage open (sugar) | `{ "short": { "collateral": "WETH", "borrow": "USDC", "amount": "1.0", "leverage": "3", "price": "3200" } }` |
| [`close_position`](#close_position--leverage-close) | **Aave-backed** leverage close (sugar) | `{ "close_position": { "collateral": "WETH", "borrow": "USDC", "current_debt": "4180.0", "current_collateral": "5.0", "slippage": "50" } }` |

> **Leverage is Aave-only.** `long` / `short` / `close_position` compile to an Aave V3 supply→borrow→swap pipeline wrapped in a Balancer flashloan (`compiler/leverage.rs`). There is no Morpho branch in the sugar; use the non-levered Morpho `deposit` / `borrow` / `withdraw` primitives for Morpho positions.

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

### `deposit` — Aave V3 or Morpho Blue Supply

```json
{ "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }
{ "deposit": { "asset": "WETH", "amount": "1.0",  "into": "morpho",
               "market": "USDC-WETH-86", "as": "collateral" } }
{ "deposit": { "asset": "USDC", "amount": "5000", "into": "morpho",
               "market": "USDC-WETH-86" } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `asset` | string | ✅ | Token alias to deposit. For Morpho must match the market's loan side (default) or collateral side (when `as: "collateral"`). |
| `amount` | string | ✅ | Human-readable amount, or `"all"` |
| `into` | string | ✅ | Protocol name: `"aave"` or `"morpho"` |
| `market` | string | ✅ *(morpho only)* | Market alias from `protocols.morpho.markets` (e.g. `"USDC-WETH-86"`). Aave rejects this field. |
| `as` | `"collateral"` \| `"loan"` (default) | — | Morpho only. Selects `supplyCollateral` vs `supply` on the loan side. |

### `borrow` — Aave V3 or Morpho Blue Borrow

```json
{ "borrow": { "asset": "DAI",  "amount": "2000", "from": "aave" } }
{ "borrow": { "asset": "USDC", "amount": "1500", "from": "morpho",
              "market": "USDC-WETH-86" } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `asset` | string | ✅ | Token alias to borrow. For Morpho must match the market's loan side. |
| `amount` | string | ✅ | Human-readable amount |
| `from` | string | ✅ | Protocol name: `"aave"` or `"morpho"` |
| `market` | string | ✅ *(morpho only)* | Market alias from `protocols.morpho.markets` |

### `withdraw` — Aave V3 or Morpho Blue Withdraw

```json
{ "withdraw": { "asset": "USDC", "amount": "5000", "from": "aave" } }
{ "withdraw": { "asset": "USDC", "amount": "1500", "from": "morpho",
                "market": "USDC-WETH-86" } }
{ "withdraw": { "asset": "WETH", "amount": "1.0",  "from": "morpho",
                "market": "USDC-WETH-86", "as": "collateral" } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `asset` | string | ✅ | Token alias to withdraw |
| `amount` | string | ✅ | Human-readable amount, or `"all"` |
| `from` | string | ✅ | Protocol name: `"aave"` or `"morpho"` |
| `market` | string | ✅ *(morpho only)* | Market alias |
| `as` | `"collateral"` \| `"loan"` (default) | — | Morpho only. `"collateral"` calls `withdrawCollateral`. |

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

> **ERC-20, native ETH, and ERC-721 sends are all first-class step types** — do not simulate them with ad-hoc `swap` or `custom` patterns. The compiler auto-inserts the appropriate `transferFrom`/`approve` when the signer's tokens still need to be pulled into the router.

### `bridge` — Across V3 Cross-Chain Transfer

Single-sided deposit: emits only the source-chain `depositV3` call. Receiving on the destination chain is authored as a separate intent.

```json
{ "bridge": {
    "via": "across",
    "asset": "USDC",
    "amount": "1000",
    "to_chain": "arbitrum",
    "recipient": "0xabc...",
    "relayer_fee_bps": "5"
} }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `via` | string | ✅ | Bridge provider; must be `"across"` in v1 |
| `asset` | string | ✅ | Token alias. Native ETH is **rejected** — pre-wrap to WETH. |
| `amount` | string | ✅ | Human-readable input amount |
| `to_chain` | string | ✅ | Destination alias from `chains.json` (e.g. `"arbitrum"`, `"base"`, `"optimism"`) |
| `recipient` | string | ✅ | Non-zero recipient address on the destination chain |
| `relayer_fee_bps` | string | ✅ | Basis points; **hard-capped at 50 (0.5%)** |

**Requires** the script's top-level `current_timestamp` (used as `quote_timestamp`; `fill_deadline = quote_timestamp + 4h`). v1 uses `output_token = input_token`; `exclusive_relayer = 0x0` (any relayer can fill).

### `flashloan` — Balancer V2 Flashloan with Inner Pipeline

0% fee flashloan from the Balancer Vault. The `then:` inner pipeline runs inside `router.receiveFlashLoan` with the flashloaned tokens already on the router's balance; each inner step is subject to the usual enrichment (approvals, transferFroms). The compiler rejects the intent if the inner pipeline cannot produce back enough of each flashloaned token to repay the Vault.

```json
{ "flashloan": {
    "via": "balancer",
    "assets": [{ "asset": "WETH", "amount": "2.0" }],
    "then": [
      { "deposit": { "asset": "WETH", "amount": "2.0", "into": "aave" } },
      { "borrow":  { "asset": "USDC", "amount": "4000", "from": "aave" } },
      { "swap":    { "from": "USDC", "amount": "4000", "to": "WETH",
                     "min_amount_out": "2.0" } }
    ]
} }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `via` | string | ✅ | Provider; must be `"balancer"` in v1 |
| `assets` | array | ✅ | One or more `{ asset, amount }` entries to flashloan |
| `then` | array | ✅ | Inner pipeline (≤5 steps, nested flashloans rejected) |

### `long` / `short` — Leverage Open

Desugars to a Balancer flashloan wrapping an Aave `supply → borrow → swap` inner pipeline. `short` is `long` with collateral and borrow swapped.

```json
{ "long":  { "collateral": "WETH", "borrow": "USDC", "amount": "1.0",
             "leverage": "5",  "slippage": "50", "price": "3200" } }
{ "short": { "collateral": "WETH", "borrow": "USDC", "amount": "1.0",
             "leverage": "3",  "slippage": "50", "price": "3200" } }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `collateral` | string | ✅ | Asset deposited to Aave as margin |
| `borrow` | string | — | Borrow asset. Default: volatile collateral → `"USDC"`, stable → `"WETH"`. |
| `amount` | string | ✅ | User's equity contribution in `collateral` units |
| `leverage` | string | ✅ | Target multiplier; capped per-asset by `aave.ltv_bps` (e.g. WETH 80% → max 5x). `"1"` is rejected — use `deposit` instead. |
| `slippage` | string | — | Max swap slippage in bps. Default 50 (0.5%). Hard-capped at 500 (5%). |
| `price` | string | ✅ when `leverage > 1` | Borrow-tokens per 1 collateral-token. Caller-supplied oracle value until the on-chain quote primitive lands. |
| `via` | string | — | Flashloan provider; default `"balancer"`. |
| `safety_margin_bps` | number | — | Extra per-call margin subtracted from effective LTV. Default 0. |

Important authoring rules:
- Do **not** prepend a separate `wrap` step before `long` / `short` just because the user said `ETH`. Author the leverage step directly and use the leverage step's `collateral` field (`"WETH"` for ETH exposure, `"wstETH"` for wrapped staked ETH exposure).
- `long` / `short` only compile when the active config includes the Aave leverage metadata the sugar depends on, especially `protocols.aave.ltv_bps`. If that metadata is missing, the correct behavior is to treat leveraged sugar as unavailable in that environment rather than retrying the same step shape.
- When leverage sugar is unavailable, fall back to plain-text explanation or a non-levered alternative such as `wrap` + `deposit`, rather than emitting a broken `long` / `short` intent.

### `close_position` — Leverage Close

Undoes a prior `long`/`short`. Requires the frontend to thread in the current Aave debt and collateral (read off-chain via `getUserAccountData`) because the compiler has no RPC.

```json
{ "close_position": {
    "collateral":         "WETH",
    "borrow":             "USDC",
    "current_debt":       "4180.0",
    "current_collateral": "5.0",
    "slippage":           "50"
} }
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `collateral` | string | ✅ | Must match the position being closed |
| `borrow` | string | ✅ | Must match the position being closed |
| `current_debt` | string | ✅ | Current debt in `borrow`-token units (> 0) |
| `current_collateral` | string | ✅ | Current collateral in `collateral`-token units (> 0) |
| `slippage` | string | — | Default 50 bps; same 500 bps cap as `long`/`short` |
| `via` | string | — | Default `"balancer"` |

Desugars to `flashloan(borrow = current_debt) { repay → withdraw → swap(collateral → borrow, min_out = current_debt) }`.

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
2. **Steps**: 1–5 outer steps (max 5 via `MAX_STEPS`); flashloan inner pipelines are bounded to 5 steps each via `MAX_FLASHLOAN_INNER_STEPS` and depth == 1 (nested flashloans rejected)
3. **Amounts**: Must be positive (> 0)
4. **Slippage**: Swaps must have `min_amount_out` or `price`+`slippage`; zero `amount_out_minimum` is rejected; LP mint/increase/decrease require at least one `min_amount*` > 0
5. **Asset compatibility**: No native ETH into Aave; no swap-to-self
6. **Amount flow**: Cross-step token consumption cannot exceed guaranteed production (fee-aware: post-skim floor)
7. **Health factor**: Borrows rejected when Aave HF < 1.2
8. **Send targets**: Cannot send to the zero address
9. **Bridge**: `relayer_fee_bps ≤ 50`; rejects native ETH (pre-wrap to WETH); requires `current_timestamp` at the script top level
10. **Flashloan**: only `via: "balancer"` in v1; inner pipeline must be repayable per-token (validated in `validate_flashloan` with `fee_bps = 0` — inner tokens are returned to the Vault, not swept)
11. **Leverage**: `leverage >= 1` (literal 1 rejected — use plain `deposit`); per-asset cap derived from `protocols.aave.ltv_bps`; `slippage ≤ 500 bps`; `collateral != borrow`; `leverage > 1` requires explicit `price` field; leverage sugar is unavailable when the active config omits required Aave metadata such as `ltv_bps`
12. **LP fees**: Uniswap V3 LP fee tier restricted to `{500, 3000, 10000}`; `position_id` must be explicit (no `"last_minted"`)

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
