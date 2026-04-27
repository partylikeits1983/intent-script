# Intent-Script: Project Overview

> **Load this file** when you need to understand what this project is, why it exists, and its core design philosophy.

## What It Is

A **Rust compiler** that transforms human-friendly JSON intent descriptions into unsigned EVM transactions (or EIP-712 typed data for relayer submission). The compiler is the "complexity sink" — it hides protocol details, token addresses, decimals, approvals, token routing, and calldata generation from the JSON input.

The system has two main components:

| Component | Location | Language | Purpose |
|-----------|----------|----------|---------|
| **Rust compiler** | `crates/intent-script/` | Rust | Transforms JSON intents into calldata |
| **Solidity router** | `contracts/src/IntentRouter.sol` | Solidity | Executes batched calls on-chain, sweeps tokens back to user |

## Why It Exists (Raison d'Être)

DeFi transactions are complex: they require knowing contract addresses, function selectors, ABI encoding, token decimals, approval flows, and protocol-specific quirks. This compiler exists so that:

1. **LLMs can produce DeFi transactions** — The JSON input format uses human-readable aliases (`"USDC"`, `"aave"`, `"uniswap"`) instead of addresses and ABI-encoded calldata. An LLM can generate valid intent JSON without knowing any Ethereum internals.

2. **Multi-step DeFi is atomic** — The IntentRouter contract batches multiple calls (swap + deposit + borrow) into a single atomic transaction. If any step fails, everything reverts.

3. **Users don't need to understand protocol internals** — The compiler automatically inserts `transferFrom`, `approve`, token sweeps, and handles token routing between steps.

## Design Principles

| Principle | What It Means |
|-----------|---------------|
| **Aliases over addresses** | JSON uses `"ETH"`, `"USDC"`, `"aave"`, `"uniswap"` — never raw addresses |
| **Human-readable amounts** | `"1.5"`, `"10000"` — compiler handles decimal conversion (USDC=6, WETH=18) |
| **Sequential steps** | Compiler infers dependencies, approvals, and token routing from step order |
| **Minimal required keys** | No ABI, calldata, addresses, or decimals in the JSON input |
| **Automatic enrichment** | Compiler inserts `transferFrom`, `approve`, token sweeps automatically |
| **Pure & deterministic** | No HTTP calls, no async, no `SystemTime` in the library. CLI provides `current_timestamp` |
| **`no_std` compatible** | Library works in WASM/no-std environments. File I/O is in the CLI binary only |
| **Config-driven** | Protocol addresses, asset decimals, chain IDs are in JSON config files — no recompile to extend |

## Supported DeFi Actions

| Action | JSON Key | Protocol | Example |
|--------|----------|----------|---------|
| Wrap ETH→WETH | `wrap` | WETH9 | `{ "wrap": { "asset": "ETH", "amount": "1.5" } }` |
| Unwrap WETH→ETH | `unwrap` | WETH9 | `{ "unwrap": { "asset": "WETH", "amount": "2.0" } }` |
| Wrap stETH→wstETH | `wrap` | Lido | `{ "wrap": { "asset": "stETH", "amount": "10.0" } }` |
| Stake ETH→stETH | `stake` | Lido | `{ "stake": { "asset": "ETH", "amount": "10.0", "into": "lido" } }` |
| Deposit into Aave | `deposit` | Aave V3 | `{ "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }` |
| Borrow from Aave | `borrow` | Aave V3 | `{ "borrow": { "asset": "DAI", "amount": "1000", "from": "aave" } }` |
| Withdraw from Aave | `withdraw` | Aave V3 | `{ "withdraw": { "asset": "USDC", "amount": "5000", "from": "aave" } }` |
| Swap via Uniswap | `swap` | Uniswap V3 | `{ "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "min_amount_out": "0.48" } }` |
| Send ERC-20 | `send` | — | `{ "send": { "asset": "USDC", "amount": "100", "to": "0x..." } }` |
| Send ETH | `send` | — | `{ "send": { "asset": "ETH", "amount": "1.0", "to": "0x..." } }` |
| Send NFT | `send` | — | `{ "send": { "asset_type": "erc721", "contract": "0x...", "token_id": "42", "to": "0x..." } }` |

## Execution Modes

| Mode | When | Output |
|------|------|--------|
| `SingleTx` | 1 call (e.g., wrap ETH) | Single unsigned tx |
| `Eip712Intent` | 2+ calls with router | Batched `executeDirect()` tx + EIP-712 typed data for `executeSigned()` |
| `TxSequence` | 2+ calls, no router | Multiple unsigned txs |

## Network Support

- **Ethereum mainnet** (chain ID 1) — fully configured with assets and protocol addresses
- Sepolia, Base, Arbitrum — chain configs exist but no asset/protocol configs yet

## Quick Reference: Running the Compiler

```bash
# Compile an intent JSON file
cargo run -p intent-script -- crates/intent-script/examples/wrap_eth.json -c ./config -p

# Run all Rust tests
make test

# Run Foundry tests
make test-foundry

# Run fork E2E tests (requires ETH_RPC_URL)
make test-fork-e2e

# Regenerate calldata + EIP-712 fixtures
make generate-fixtures
```
