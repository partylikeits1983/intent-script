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

## Remaining Work (Future) — DONE 2026-05-06

### Compiler-side prerequisite metadata — shipped

The compiler now emits `vDebtToken.approveDelegation(router, amount)` as a
prerequisite tx alongside ERC-20 approves, so the UI's
`prerequisiteApprovals` chain handles credit delegation end-to-end. Selector:
`0xc04a8a10` (`keccak256("approveDelegation(address,uint256)")[:4]`).

**Implementation (all merged):**

| File | Change |
|------|--------|
| `config/protocols/{ethereum,anvil}.json`, `intentOS-ui/lib/config/protocols-anvil.json` | Added `variable_debt_tokens` map to aave config (USDC, USDT, WETH, DAI, WBTC, wstETH) |
| `crates/intent-script/src/registry/loader.rs` | Added `variable_debt_tokens` field to `ProtocolConfig`; added `aave_variable_debt_token(asset)` lookup helper |
| `crates/intent-script/src/ir/canonical.rs` | Added `required_delegations: Vec<(Address, U256)>` field on `ResolvedIntent` (parallel to `required_pulls`) |
| `crates/intent-script/src/compiler/enrich.rs` | `AaveV3Borrow` arm aggregates per-vDebt borrow amounts when batching through the router; recursion through `BalancerFlashloan` propagates inner delegations to outer |
| `crates/intent-script/src/compiler/build.rs` | `build()` signature gained `current_delegations` + `required_delegations`; emits `approveDelegation(router, amount)` UnsignedTx in `prerequisite_approvals` for under-delegated vDebt entries (same `None == legacy` back-compat as ERC-20 approves) |
| `crates/intent-script/src/schema/public_ast.rs` | `AllowancesInput` gained optional `delegations: HashMap<String, String>` keyed by borrowed-asset alias |
| `crates/intent-script/src/compiler/mod.rs` | Parses `delegations`, resolves each alias → vDebt address via the registry, passes the snapshot map to `build()` |
| `crates/intent-script/tests/allowance_tests.rs` | 4 new tests locking in the prereq emission, saturation skip, partial-delegation full-amount re-grant, and legacy-compile back-compat |
| `crates/intent-script/tests/integration.rs` | `test_deposit_and_borrow_single_tx` extended to assert delegation prereq for the DAI borrow |
| `intentOS-ui/lib/fetch-allowances-json.ts` | Multicall now reads `borrowAllowance(user, router)` for every vDebt token alongside ERC-20 allowances; emits `{ tokens, delegations }` |
| `intentOS-ui/lib/required-approvals.ts` | Added `decodeApproveDelegationTx` + `kind: "credit_delegation"` discriminator on `MissingApproval`; preview card labels delegation entries as "USDT debt" via vDebt-→-underlying reverse-lookup |
| `intentOS-ui/components/finalize-intent-tool.tsx` | `handleApprove` branches on `kind` and calls `approveDelegation` via the new `CREDIT_DELEGATION_ABI` for delegation prereqs |
| `intentOS-ui/lib/diagnose-revert.ts` | `aave_borrow_delegation` branch matches the custom-error selector `0x1cb19ef3` (in addition to Aave's string codes), so the symptom that motivated this whole feature is named even when bypass paths leak through |

## Verification

```bash
make generate-fixtures
cd contracts && forge test --mc IntentForkE2E --fork-url https://ethereum-rpc.publicnode.com -vvv
```

All 7 tests should pass.
