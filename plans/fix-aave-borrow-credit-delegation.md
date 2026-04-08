# Plan: Fix Aave V3 Borrow Credit Delegation

## Problem Summary

When the compiler batches calls through the `IntentRouter`, the router is `msg.sender` for all calls. Aave V3's `borrow(asset, amount, rateMode, referralCode, onBehalfOf)` requires that when `msg.sender != onBehalfOf`, the `msg.sender` must have **credit delegation** via `variableDebtToken.approveDelegation(delegatee, amount)`.

The compiler does not account for this, so all borrow-through-router intents revert with Aave error `0x1cb19ef3` (`InsufficientBorrowAllowance`).

**3 fork E2E tests fail:**
- `test_fork_depositBorrow`
- `test_fork_complexDefi_executeDirect`
- `test_fork_complexDefi_executeSigned`

## Single-Popup UX Analysis

### Why `borrow(onBehalfOf=router)` doesn't work

The intuitive fix — change `onBehalfOf` from the user to the router so `msg.sender == onBehalfOf` — **does not work** because:

1. `supply(onBehalfOf=user)` deposits collateral into the **user's** Aave position
2. `borrow(onBehalfOf=router)` would try to borrow against the **router's** position, which has zero collateral
3. Aave checks the borrower's health factor, so the router would have insufficient collateral

The user's collateral and debt must be on the same Aave position (same address).

### Current UX reality

The system already requires a separate `token.approve(router, amount)` transaction before any batch that pulls ERC-20 tokens. This is an inherent limitation of EOA wallets — approvals must come from the user's address.

For borrow intents, the UX is:
1. **One-time setup**: `token.approve(router, MAX)` + `variableDebtToken.approveDelegation(router, MAX)` — these can be max-approved once and reused
2. **Per-intent**: Single `executeDirect(...)` or EIP-712 signature — **one popup**

With max approvals cached, the user experience is indeed a single popup per intent.

### Future: Smart account wallets (ERC-4337)

With smart contract wallets, the `approveDelegation` call can be bundled into the same UserOperation as the batch execution, achieving true single-popup UX even for first-time interactions. This is a future enhancement.

## Approach: Pre-batch `approveDelegation`

Credit delegation is analogous to ERC-20 `approve`: the **user** must call `variableDebtToken.approveDelegation(router, amount)` before the batch executes. This cannot be inside the batch because the router is `msg.sender` in the batch, and `approveDelegation` must be called by the delegator (the user).

## Changes Made

### 1. Fork E2E tests — add `approveDelegation` calls

**File:** [`contracts/test/IntentForkE2E.t.sol`](../contracts/test/IntentForkE2E.t.sol)

Added a reusable helper `_approveDelegation()` and called it in all 3 failing tests:

- **`test_fork_depositBorrow`**: Added `_approveDelegation(VDEBT_DAI, user, ROUTER_ADDR, 2000e18)` before batch execution
- **`test_fork_complexDefi_executeDirect`**: Added `_approveDelegation(VDEBT_DAI, user, ROUTER_ADDR, 1000e18)` before batch execution
- **`test_fork_complexDefi_executeSigned`**: Added `_approveDelegation(VDEBT_DAI, signer, address(signedRouter), 1000e18)` before batch execution

## Remaining Work (Future)

### Compiler-side prerequisite metadata

The compiler could optionally output a list of required prerequisites (approvals + delegations) so frontends know what to prompt for. This is not blocking — the tests pass without it.

| File | Change |
|------|--------|
| `config/protocols/ethereum.json` | Add `variable_debt_tokens` map to aave config |
| `crates/intent-script/src/registry/loader.rs` | Add `variable_debt_tokens` field to `ProtocolConfig`, add lookup helper |
| `crates/intent-script/src/ir/canonical.rs` | Add `prerequisites` field to `ResolvedIntent` |
| `crates/intent-script/src/compiler/enrich.rs` | Generate `approveDelegation` prerequisites for borrow steps |
| `crates/intent-script/src/output.rs` | Add `Prerequisite` type and include in output types + JSON serialization |

## Verification

```bash
make generate-fixtures
cd contracts && forge test --mc IntentForkE2E --fork-url https://ethereum-rpc.publicnode.com -vvv
```

All 7 tests should pass.
