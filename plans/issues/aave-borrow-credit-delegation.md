# Issue: Aave V3 Borrow Through Router Fails — Missing Credit Delegation

## Status
**Resolved** — all 7 fork E2E tests pass.

## Severity
**High** — any intent that includes an Aave V3 `borrow` step through the router will revert on mainnet.

## Description

When the compiler batches calls through the `IntentRouter`, the router is the `msg.sender` for all calls. For Aave V3's `borrow(asset, amount, rateMode, referralCode, onBehalfOf)`, the `onBehalfOf` is set to the **user** (signer), but the `msg.sender` is the **router**.

Aave V3 requires that when `msg.sender != onBehalfOf`, the `msg.sender` must have been granted **credit delegation** by the `onBehalfOf` address. This is done via the variable debt token's `approveDelegation(delegatee, amount)` function.

Additionally, Aave V3 sends borrowed tokens to `msg.sender` (the router), not to `onBehalfOf` (the user). The router must sweep the borrowed tokens back to the user.

## Root Cause

Two issues:

1. **Credit delegation**: The compiler does not generate an `approveDelegation` call, so the borrow reverts with Aave's custom error `0x1cb19ef3` (`InsufficientBorrowAllowance`).

2. **Token sweep**: Aave V3's `borrow()` sends borrowed tokens to `msg.sender` (the router), not `onBehalfOf` (the user). The enricher was not adding the borrowed asset to `tokens_to_sweep`, so borrowed tokens were stuck in the router.

## Fix Applied

### 1. Enricher: Add borrowed asset to sweep tokens

**File:** [`crates/intent-script/src/compiler/enrich.rs`](../../crates/intent-script/src/compiler/enrich.rs)

Added explicit handling for `AaveV3Borrow` in the enricher. When batching via router, the borrowed asset is added to `tokens_to_sweep` so the router sweeps it back to the user after execution.

### 2. Fork E2E tests: Add `approveDelegation` calls

**File:** [`contracts/test/IntentForkE2E.t.sol`](../../contracts/test/IntentForkE2E.t.sol)

- Added reusable `_approveDelegation()` helper
- Added `approveDelegation` calls before batch execution in all 3 borrow tests
- Added DAI to `tokensToSweep` in the manually-built `executeSigned` batch
- Changed exact DAI assertions to `assertApproxEqAbs` (router may have dust from mainnet state)

### 3. Regenerated fixtures

The compiler now generates `tokensToSweep` arrays that include the borrowed asset:
- `deposit_borrow_batch.json`: `tokensToSweep: 1` (DAI)
- `complex_defi_batch.json`: `tokensToSweep: 2` (WETH + DAI)

## Verification

```bash
make generate-fixtures
cd contracts && forge test --mc IntentForkE2E --fork-url https://ethereum-rpc.publicnode.com -vvv
```

All 7 tests pass:
- `test_fork_wrapETH` ✅
- `test_fork_swapUSDC_WETH` ✅
- `test_fork_aaveDepositUSDC` ✅
- `test_fork_depositBorrow` ✅
- `test_fork_stakeETH_lido` ✅
- `test_fork_complexDefi_executeDirect` ✅
- `test_fork_complexDefi_executeSigned` ✅

## UX Note: Credit Delegation as a Prerequisite

Credit delegation (`approveDelegation`) must be called by the **user** before the batch, similar to how `token.approve(router, amount)` must be called before the batch can `transferFrom`. This is an inherent limitation of EOA wallets.

With max approvals cached (one-time setup), the per-intent UX is a single MetaMask popup.
