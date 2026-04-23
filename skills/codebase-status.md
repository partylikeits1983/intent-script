# Codebase Status & Capabilities

> **Load this file** when you need to know what works, what's missing, test counts, known issues, and the current state of the project.

## Current Capabilities

### Supported Protocols

| Protocol | Version | Actions | On-Chain Contract | Adapter File |
|----------|---------|---------|-------------------|-------------|
| **Uniswap** | V3 | `swap` (exactInputSingle) | `0xE592427A0AEce92De3Edee1F18E0157C05861564` | `adapters/uniswap_v3.rs` |
| **Uniswap** | V3 LP | `lp_mint`, `lp_increase`, `lp_decrease`, `lp_collect` | NPM `0xC36442b4a4522E871399CD717aBDD847Ab11FE88` | `adapters/uniswap_v3_lp.rs` |
| **Aave** | V3 | `deposit`, `borrow`, `withdraw`, `repay` (via `close_position`) | `0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2` | `adapters/aave_v3.rs` |
| **Morpho Blue** | — | `deposit`/`borrow`/`withdraw` with required `market`; optional `as: "collateral"` discriminator | `0xBBBBBbbBBb9cC5e90e3b3Af64bdAF62C37EEFFCb` | `adapters/morpho.rs` |
| **Lido** | — | `stake` (ETH→stETH), `wrap` (stETH→wstETH), `unwrap` (wstETH→stETH), `request_withdrawal`, `claim_withdrawal` | stETH `0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84`, Queue `0x889edC2eDab5f40e902b864aD4d7AdE8E412F9B1` | `adapters/lido.rs` |
| **WETH9** | — | `wrap` (ETH→WETH), `unwrap` (WETH→ETH) | `0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2` | `adapters/wrap.rs` |
| **1inch** | Fusion v6 | `swap` (calldata passthrough) | `0x111111125421cA6dc452d289314280a0f8842A65` | `adapters/oneinch.rs` |
| **Balancer V2** | — | `flashloan` (0% fee, recursive inner pipeline) + `long`/`short`/`close_position` leverage sugar | Vault `0xBA12222222228d8Ba445958a75a0704d566BF2C8` | `adapters/balancer.rs`, `compiler/leverage.rs` |
| **Across** | V3 | `bridge` (source-chain `depositV3` only) | SpokePool `0x5c7BCd6E7De5423a257D81B442095A1a6ced35C5` | `adapters/across.rs` |
| **ERC-20** | — | `approve`, `transferFrom`, `permit`, `transfer` | Any ERC-20 | `adapters/erc20.rs` |
| **Send** | — | ERC-20 transfer, ETH send, ERC-721 safeTransferFrom (first-class step types — do not re-implement) | Any | `adapters/send.rs` |

### IntentRouter fee mechanism

The IntentRouter collects a protocol skim at sweep time: `feeBps` (default **10 bps** = 0.10%) of each swept ERC-20 balance is transferred to `feeRecipient` before the remainder is returned to the signer. ETH refunds apply the same skim. Fees are hard-capped at **100 bps** (1%) and can only be changed by the owner via `queueFee(newBps, newRecipient)` → `applyFee()` after a **24-hour timelock**. The compiler reads the active `fee_bps` from `protocols.intent_router.fee_bps` and threads it into `step_produces` so that `"amount": "all"` downstream consumers see the post-skim floor.

### Supported Multi-Step Flows (via IntentRouter)

| Flow | Steps | Auto-Inserted by Compiler |
|------|-------|---------------------------|
| **Swap** | swap | transferFrom + approve + exactInputSingle + sweep |
| **Deposit into Aave** | deposit | transferFrom + approve + supply |
| **Deposit + Borrow** | deposit → borrow | transferFrom + approve + supply + borrow + sweep(borrowed) |
| **Swap + Deposit** | swap → deposit | transferFrom + approve + swap(→router) + approve + supply |
| **Swap + Deposit + Borrow** | swap → deposit → borrow | transferFrom + approve + swap(→router) + approve + supply + borrow + sweep(collateral, borrowed) |
| **Stake + Wrap** | stake → wrap(stETH) | stake(ETH→stETH) + approve(stETH→wstETH) + wrap + sweep(wstETH) |
| **Unwrap wstETH** | unwrap(wstETH) | transferFrom(wstETH) + unwrap + sweep(stETH) |
| **Withdraw from Aave** | withdraw | withdraw (user must have existing position) |
| **Lido Withdrawal Request** | request_withdrawal | transferFrom + approve(queue) + requestWithdrawals[WstETH] (NFTs → signer) |
| **Lido Withdrawal Claim** | claim_withdrawal | claimWithdrawals (ETH → caller) |
| **Uniswap V3 LP Mint** | lp_mint | transferFrom×2 + approve(NPM)×2 + mint (NFT → signer) + sweep(pair) |
| **Uniswap V3 LP Increase** | lp_increase | transferFrom×2 + approve(NPM)×2 + increaseLiquidity + sweep(pair) |
| **Uniswap V3 LP Decrease + Collect** | lp_decrease → lp_collect | decreaseLiquidity + collect + sweep(pair) |
| **Morpho supply + borrow** | deposit(as=collateral) → borrow | transferFrom + approve(pool) + supplyCollateral + borrow(receiver=router) + sweep(loan) |
| **Balancer flashloan** | flashloan { then: [...] } | vault.flashLoan(recipient=router, …) — router.receiveFlashLoan decodes inner Call[], executes each, transfers owed amount back |
| **Leveraged long/short** | long | close_position | Desugars to Balancer flashloan wrapping supply→borrow→swap (open) or repay→withdraw→swap (close) |
| **Across bridge** | bridge | transferFrom + approve(spoke_pool) + depositV3 (no sweep — tokens in flight) |

### Execution Modes

| Mode | Condition | Output |
|------|-----------|--------|
| **SingleTx** | 1 call (e.g., wrap ETH) | Single unsigned transaction |
| **Eip712Intent** | 2+ calls with router configured | Batched `executeDirect()` tx + EIP-712 typed data for `executeSigned()` |
| **TxSequence** | 2+ calls, no router | Multiple unsigned transactions |

### Network Support

| Network | Chain ID | Assets | Protocols | Status |
|---------|----------|--------|-----------|--------|
| Ethereum mainnet | 1 | ✅ 8 tokens | ✅ 5 protocols | Fully configured |
| Sepolia | 11155111 | ❌ | ❌ | Chain config only |
| Base | 8453 | ❌ | ❌ | Chain config only |
| Arbitrum | 42161 | ❌ | ❌ | Chain config only |

---

## V1 Feature Completeness

| Area | Status | Notes |
|------|--------|-------|
| Intent → Valid Plan | ✅ Done | Strict JSON parsing, max 5 steps, validation |
| Amount Safety | ✅ Done | Zero rejection, `"all"` syntax, cross-step flow validation |
| Compiler Basics | ✅ Done | Full pipeline, all adapters, auto-enrichment |
| Router Safety | ✅ Done | Allowlist, deadline enforcement, nonce replay protection |
| Aave Safety | ✅ Done | Health factor check (HF < 1.2 → reject, HF < 1.5 → warn) |
| EIP-712 Signing | ✅ Done | Domain separator, struct hashing, typed data output |
| Slippage Protection | ✅ Done | `min_amount_out`, `price`+`slippage`, zero rejection |
| Send Step | ✅ Done | ERC-20, ETH, ERC-721 transfers |
| **Simulation** | ❌ Not built | Needs RPC layer (`eth_call`). Belongs in CLI/frontend, not library |
| **User Preview** | ❌ Not built | No structured "you send X / you receive Y" in output |
| **Multi-chain configs** | ❌ Not built | Only Ethereum mainnet has asset/protocol configs |

---

## Test Infrastructure

### Rust Tests

| Test File | Count | What |
|-----------|-------|------|
| `crates/intent-script/tests/integration.rs` | ~37 | Compiler pipeline end-to-end (adds Lido queue + V3 LP cases) |
| `crates/intent-script/tests/enricher_tests.rs` | ~8 | Enrichment-specific tests |
| `crates/intent-script/tests/fuzz_amounts.rs` | ~5 | Amount parsing fuzz tests |
| `crates/intent-script/tests/generate_calldata.rs` | ~11 | Fixture generators for Foundry |
| `crates/intent-script/tests/generate_eip712_fixtures.rs` | ~6 | EIP-712 batch fixture generators |
| `crates/intent-script/src/eip712.rs` (unit tests) | 5 | EIP-712 hashing verification |
| `crates/intent-script/src/` (other unit tests) | ~6 | Amount parsing, etc. |
| `crates/evm-testing/tests/anvil_tests.rs` | 3 | Anvil fork tests (1 ignored) |

**Total Rust:** ~150 tests, all passing, 1 ignored (see Known Issues)

### Foundry Tests

| Test File | Count | What |
|-----------|-------|------|
| `contracts/test/IntentRouter.t.sol` | 17 | Unit tests with mocks (executeDirect + executeSigned) |
| `contracts/test/IntentRouterCalldata.t.sol` | 7 | Calldata verification from fixtures |
| `contracts/test/IntentRouterFees.t.sol` | 10 | Fee accrual / withdrawal |
| `contracts/test/IntentRouterReentrancy.t.sol` | 1 | Reentrancy guard |
| `contracts/test/IntentForkTests.t.sol` | 5 | Local mock integration tests |
| `contracts/test/IntentForkE2E.t.sol` | 7 | Fork E2E against mainnet |

**Total Foundry:** 47 tests, all passing

### Running Tests

```bash
# All Rust tests
make test
# or: cargo test -p intent-script

# Foundry unit tests (no fork needed)
make test-foundry
# or: cd contracts && forge test --no-match-test fork

# Fork E2E tests (requires ETH_RPC_URL env var)
make test-fork-e2e
# or: cd contracts && forge test --match-test fork --fork-url $ETH_RPC_URL

# Regenerate calldata + EIP-712 fixtures
make generate-fixtures

# Full workspace including evm-testing
cargo test --workspace

# Run a single example
cargo run -p intent-script -- crates/intent-script/examples/wrap_eth.json -c ./config -p
```

---

## Known Issues

### 1. Anvil WETH Withdraw Revert
- **Test:** `test_unwrap_weth_on_anvil` in `crates/evm-testing/tests/anvil_tests.rs`
- **Status:** `#[ignore]` — known Anvil environment bug
- **Cause:** WETH `withdraw()` reverts in Anvil fork due to gas stipend issue
- **Impact:** Compiler output is correct; only the Anvil test environment is affected
- **Details:** `plans/issues/weth-withdraw-anvil-revert.md`

### 2. `InvalidChain` Error Variant Overloaded
- The `CompileError::InvalidChain` variant is used for cross-step validation errors that aren't chain-related
- Semantic mismatch — should ideally be a separate variant
- Low priority, cosmetic issue

### 3. No Simulation Infrastructure
- The compiler produces unsigned txs but cannot verify they'll succeed on-chain
- Simulation requires an RPC connection (`eth_call`)
- The library is `no_std`-compatible and has no network access
- Simulation belongs in the CLI (`main.rs`) or frontend layer

### 4. Aave V3 WETH LTV set to 0 on Mainnet
- `test_fork_complexDefi_*` originally swapped USDC → WETH and borrowed against WETH collateral
- After Aave governance set WETH LTV to 0 (post-2024), borrow validation reverts with `LtvValidationFailed()` even though the compiler output is correct
- **Fix:** `examples/complex_defi.json` swaps USDC → wstETH (0.05% fee tier) and borrows DAI against wstETH collateral (LTV ≈ 78.5%). The test was updated to match

---

## Resolved Issues (Historical)

### Missing `transferFrom` in Batched Calldata
- **Issue:** Router couldn't pull tokens from user
- **Fix:** Enricher now inserts `transferFrom(user, router, amount)` for tokens not already in the router
- **Details:** `plans/issues/missing-transferfrom-in-batched-calldata.md`

### Aave V3 Borrow Credit Delegation
- **Issue:** `borrow(onBehalfOf=user)` reverts when `msg.sender=router` because no credit delegation
- **Fix:** (a) Tests add `approveDelegation` prerequisite. (b) Enricher adds borrowed asset to `tokens_to_sweep` since Aave sends borrowed tokens to `msg.sender` (router)
- **Details:** `plans/issues/aave-borrow-credit-delegation.md`

---

## Configured Assets (Ethereum Mainnet)

From `config/assets/ethereum.json`:

| Asset | Address | Decimals |
|-------|---------|----------|
| ETH | `0x0000000000000000000000000000000000000000` | 18 |
| WETH | `0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2` | 18 |
| USDC | `0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48` | 6 |
| USDT | `0xdAC17F958D2ee523a2206206994597C13D831ec7` | 6 |
| DAI | `0x6B175474E89094C44Da98b954EedeAC495271d0F` | 18 |
| WBTC | `0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599` | 8 |
| stETH | `0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84` | 18 |
| wstETH | `0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0` | 18 |
