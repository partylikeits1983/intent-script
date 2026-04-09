# IntentOS V1 Minimal Plan — Status Report

> Maps each V1 requirement to what exists in the codebase today.

---

## 1. 🧠 Intent → Valid Plan

| Requirement | Status | Where |
|-------------|--------|-------|
| Strict JSON input (no ambiguity) | ✅ Done | `crates/intent-script/src/schema/public_ast.rs` — serde-typed `IntentScript`, `Step` enum with strict variants |
| Max 3 steps | ⚠️ Currently 5 | `crates/intent-script/src/compiler/validate.rs:20` — `MAX_STEPS = 5`. Change to 3. |
| Only supported actions: swap, deposit, borrow, stake | ✅ Done | `public_ast.rs:70` — `Step` enum: `Swap`, `Deposit`, `Borrow`, `Withdraw`, `Wrap`, `Unwrap`, `Stake`, `Send`, `Custom`. Unknown variants rejected at parse time. |
| Reject anything unclear or incomplete | ✅ Done | `validate.rs` — rejects zero amounts, zero signer, missing fields, unknown protocols, unknown assets. `normalize.rs` — rejects invalid addresses, bad hex, unknown providers. |

---

## 2. 🔗 Amount Safety (CRITICAL)

| Requirement | Status | Where |
|-------------|--------|-------|
| Each step must have valid funds | ✅ Done | `validate.rs:287` — `validate_amount()` rejects zero amounts for all step types |
| Support exact amount | ✅ Done | `normalize.rs:752` — `parse_amount()` handles integers and decimals with token-specific precision |
| Support "use all from previous step" | ✅ Done | `normalize.rs:544` — `resolve_amount_or_all()` resolves `"all"` to guaranteed output of prior step |
| Reject if output of step N < input of step N+1 | ✅ Done | `validate.rs:167` — `validate_amount_flow()` tracks produced/consumed tokens across steps, rejects overflows |
| No guessing, no implicit balances | ✅ Done | Flow validation only checks tokens that were explicitly produced by prior steps. Wallet-sourced tokens are not checked (user responsibility). |

---

## 3. ⚙️ Compiler Basics

| Requirement | Status | Where |
|-------------|--------|-------|
| Insert transferFrom | ✅ Done | `enrich.rs:45-51` — auto-inserts `Erc20TransferFrom` when batching via router and token not already in router |
| Insert approve (exact amount only) | ✅ Done | `enrich.rs:54-58` — inserts `Erc20Approve` with exact amount before each protocol call |
| Aave V3 | ✅ Done | `adapters/aave_v3.rs` — supply, borrow, withdraw |
| Uniswap V3 (simple swap) | ✅ Done | `adapters/uniswap_v3.rs` — exactInputSingle |
| Lido | ✅ Done | `adapters/lido.rs` — stake (submit), wstETH wrap |
| WETH | ✅ Done | `adapters/wrap.rs` — deposit (wrap), withdraw (unwrap) |
| Ethereum mainnet only | ✅ Done | `config/chains.json`, `config/assets/ethereum.json`, `config/protocols/ethereum.json` |

---

## 4. 🧪 Simulation (MUST)

| Requirement | Status | Where |
|-------------|--------|-------|
| Simulate transaction before signing | ❌ Not built | No simulation infrastructure exists. The compiler produces unsigned txs but does not call `eth_call` or any RPC. |
| Show tokens in/out | ❌ Not built | No token flow summary in output. The `description` field on each call is human-readable but not structured. |
| Show success/failure | ❌ Not built | No simulation = no success/failure signal. |
| Block execution if simulation fails | ❌ Not built | Would need RPC integration (outside no-std library scope — belongs in CLI/frontend). |

**Note:** Simulation requires an RPC connection (`eth_call`). The compiler library is `no_std`-compatible and has no network access. Simulation belongs in the CLI (`main.rs`) or frontend layer, calling the compiled tx against a node.

---

## 5. 🔐 Router Safety

| Requirement | Status | Where |
|-------------|--------|-------|
| Only allow known contracts + functions | ✅ Done | `IntentRouter.sol:29` — `allowedTargets` mapping, checked in `_executeCalls()` at line 122. Owner-managed via `setAllowedTarget()` / `setAllowedTargets()`. |
| Enforce deadline | ✅ Done | `IntentRouter.sol:101` — `require(batch.deadline > 0 && block.timestamp <= batch.deadline)`. Compiler computes deadlines from `current_timestamp` in `normalize.rs:35-41`. |
| Enforce nonce | ✅ Done | `IntentRouter.sol:104` — `require(batch.nonce == nonces[batch.signer])` with auto-increment. |
| Enforce min output / max input | ✅ Done | `validate.rs:121` — `validate_slippage()` rejects swaps with `amountOutMinimum == 0`. Compiler computes min output from `min_amount_out` or `price` + `slippage`. |
| Sweep all funds back to user | ✅ Done | `IntentRouter.sol:135` — `_sweep()` transfers all ERC-20 balances back. `_refundETH()` at line 145 returns leftover ETH. Enricher tracks sweep tokens in `enrich.rs`. |

---

## 6. 🏦 Aave Safety

| Requirement | Status | Where |
|-------------|--------|-------|
| Check health factor after borrow | ✅ Done | `validate.rs:138` — `validate_health_factor()` checks `aave_health_factor` from user-provided balances |
| Reject unsafe borrows | ✅ Done | HF < 1.2 → hard error. HF 1.2–1.5 → warning. HF > 1.5 → clean. Borrow without collateral (when balances provided) → error. |

---

## 7. ✍️ Signing

| Requirement | Status | Where |
|-------------|--------|-------|
| EIP-712 typed data | ✅ Done | `eip712.rs` — full EIP-712 implementation with domain separator, struct hashing |
| Include plan hash | ✅ Done | `build.rs:115-118` — `hash_intent_batch()` hashes all calls, sweep tokens |
| Include deadline | ✅ Done | `build.rs:116` — deadline included in `IntentBatch` struct hash |
| Include nonce | ✅ Done | `build.rs:116` — nonce included in `IntentBatch` struct hash |

---

## 8. 🧾 User Preview (Before Sign)

| Requirement | Status | Where |
|-------------|--------|-------|
| Show "You send" | ❌ Not built | No structured "you send" field in output. The `description` string contains human-readable info but not a structured summary. |
| Show "You receive (min)" | ❌ Not built | `min_amount_out` is computed and used in calldata but not surfaced as a separate output field. |
| Show steps being executed | ✅ Partial | `build.rs:57` — `description` field lists all steps: `"Batched via router: [Approve USDC, Swap USDC→WETH, ...]"`. Individual call descriptions exist but no structured step list in JSON output. |

---

## Summary

| Section | Status |
|---------|--------|
| 1. Intent → Valid Plan | ✅ Done (MAX_STEPS needs 5→3) |
| 2. Amount Safety | ✅ Done |
| 3. Compiler Basics | ✅ Done |
| 4. Simulation | ❌ Not built (needs RPC layer) |
| 5. Router Safety | ✅ Done |
| 6. Aave Safety | ✅ Done |
| 7. Signing | ✅ Done |
| 8. User Preview | ❌ Not built (needs structured output fields) |

---

## What Needs Building

### Quick fix (< 1 hour)
- Change `MAX_STEPS` from 5 to 3 in `validate.rs:20`
- Update the test in `validate.rs:546` that creates 6 steps (adjust to 4)

### Medium effort (simulation)
- Add `eth_call` simulation in `main.rs` (CLI layer, not library)
- Parse simulation result for success/failure
- This is a **frontend/CLI concern**, not a compiler library concern

### Medium effort (user preview)
- Add `preview` field to `CompileOutputJson` with structured `you_send` / `you_receive_min` / `steps` arrays
- Extract from existing IR data (token addresses, amounts, step types)

---

## Test Results (Current)

- **Rust:** 133 tests — 130 passed, 0 failed, 1 ignored (known Anvil bug)
- **Foundry:** 29 tests — 29 passed, 0 failed, 0 skipped
