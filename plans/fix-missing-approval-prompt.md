# Fix: approval prompt never appears for swap intents

> Final plan will be moved to `intent-script/plans/fix-missing-approval-prompt.md` after exit-plan-mode.

## Context

User runs "swap 5000 USDC → ETH" against the freshly redeployed IntentRouter (`0xe008520f77fD5c7EF5D0f6533969b8605065c59C`). The user has 0 USDC allowance to that router. Expected: UI shows an approval prompt before simulation/execution. Observed: simulation reverts with `ERC20: transfer amount exceeds allowance`, no approval prompt at all.

Why this regressed:
- The UI's "needs-approvals" phase in `components/finalize-intent-tool.tsx:462-481` fires only when `checkRequiredApprovals(compileOutput, …)` returns a non-empty array.
- `checkRequiredApprovals` (`lib/required-approvals.ts:64-95`) just decodes `compileOutput.prerequisiteApprovals` — it does no on-chain re-check.
- The Rust compiler emits `prerequisiteApprovals` only when (a) the UI passes an `allowancesJson`, AND (b) the reported allowance for that token is below the required pull, AND (c) the output type is `eip712_intent` (`crates/intent-script/src/compiler/build.rs:184-200`).
- For this swap the compiler is emitting an **empty** `prerequisiteApprovals` even though on-chain allowance is 0. So `MissingApproval[]` is empty → the UI skips the needs-approvals phase → it goes straight to simulation, which (correctly) reverts.

Root cause: the compiler-side allowance check decides "no approval needed" using whatever the UI passed as `allowancesJson`. If that JSON reports USDC allowance >= required, no prereq is emitted. We need to identify why the JSON is wrong (or short-circuit with a defensive UI check). My recent `simulate-transaction.ts` chain fix only handles the case where prereqs *are* emitted — it can't fix this case because there's nothing to chain.

## Recommended approach

Two-layer fix: a **defensive UI fallback** (always works regardless of compiler emission) + a **diagnostic log** to identify why the compiler is missing it.

### 1. Defensive UI fallback — show "Approve" prompt when sim hits `insufficient_allowance`

When simulation fails with `diagnosis.code === "insufficient_allowance"` AND `compileOutput.prerequisiteApprovals` is empty, derive the missing approvals from the intent's input tokens and surface them.

**File: `components/finalize-intent-tool.tsx`** (around the simulation-result handling, ~line 660)
- After `runSimulation` resolves, inspect every `SimulationResult` for `diagnosis.code === "insufficient_allowance"`.
- If found and `compileOutput.prerequisiteApprovals` is empty/undefined:
  - Read the intent's input tokens from `compileOutput.preview?.inputs[]` (each `TokenAmountJson` has `address` + `amountRaw`).
  - Resolve the router via existing `routerAddressFor(network)` from `lib/router-address.ts`.
  - For each input token, do a fresh `readContracts` of `allowance(address, router)` (mirror `lib/fetch-allowances-json.ts:62-83`).
  - For each token whose on-chain allowance < `amountRaw`, build a synthetic `MissingApproval` (`lib/required-approvals.ts:24-38` shape) using `lookupToken` for symbol/decimals.
  - Set phase to the existing `kind: "needs-approvals"` with that array.
- The existing `handleApprove` (line 686) and re-check loop (lines 729-739) then drive the user through the approval, after which the existing flow re-compiles + re-simulates.

Reuses existing infra: `MissingApproval`, `handleApprove`, `routerAddressFor`, `lookupToken`, `readContracts`. No new components needed.

### 2. Diagnostic log — surface why the compiler is missing the prereq

**File: `lib/simulate-transaction.ts`** (in `simulateCompiledOutput`, dev-only)
- Log `output.type`, `output.prerequisiteApprovals?.length ?? 0`, `output.transactions?.length ?? 0`, and the first 4-byte selector(s) of `directTx.data` so we can see at a glance:
  - Is the type really `eip712_intent`?
  - How many prereqs did the compiler emit?
- Also log the result of `fetchAllowancesJson` once before each compile call in `finalize-intent-tool.tsx` (the call site at lines 366, 421, 516) so we know what allowance numbers the compiler actually saw.

Gate everything on `process.env.NODE_ENV !== "production"`.

After the next failing run the console will tell us:
- If `prereqsCount === 0` and `allowances.USDC === "0"`: it's a Rust compiler bug — file a follow-up to fix `compiler/build.rs:184-200`.
- If `allowances.USDC !== "0"`: the UI is reporting stale allowance — fix `fetch-allowances-json.ts` (likely a router-address mismatch in `protocols-anvil.json` vs what's actually deployed).

The defensive fallback in step 1 makes the user-facing flow work regardless of which case it is.

### 3. Verify chain-sim fix is loaded

Quick sanity check before debugging deeper: `pkill -f "next dev"` and restart `pnpm dev`. Next.js dev sometimes doesn't HMR server-side modules (`lib/simulate-transaction.ts`). If the user's session still has the pre-fix simulator loaded, behavior matches what's reported regardless of file contents.

## Files to modify

- `intentOS-ui/components/finalize-intent-tool.tsx` — add `insufficient_allowance` recovery path that synthesizes `MissingApproval[]` from `preview.inputs` + on-chain allowance recheck, then transitions to `kind: "needs-approvals"`.
- `intentOS-ui/lib/simulate-transaction.ts` — dev-only diagnostic log.
- `intentOS-ui/components/finalize-intent-tool.tsx` (call sites) — dev-only log of fetchAllowancesJson result.

Existing utilities reused (do not reimplement):
- `lib/router-address.ts` → `routerAddressFor`
- `lib/required-approvals.ts` → `MissingApproval`, `checkRequiredApprovals`
- `lib/fetch-allowances-json.ts` → mirror its `readContracts` shape
- `lib/token-metadata.ts` → `lookupToken`
- `components/finalize-intent-tool.tsx` → `handleApprove`, `kind: "needs-approvals"` phase

## Verification

1. Restart `pnpm dev` (clean slate).
2. Connect dev account `0xf39Fd6e…` (or whichever account has 0 USDC allowance to the live router).
3. Send "swap 5000 USDC → ETH".
4. **Expected**: console shows `[intentos.compile] reads { type: 'eip712_intent', prereqsCount: 0|N, allowances: { USDC: '0', … } }`.
5. **Expected**: UI surfaces an "Approve 5000 USDC for IntentRouter" prompt (either via compiler-emitted prereqs OR via the new fallback).
6. Click approve, MetaMask sign, wait for receipt.
7. UI re-runs simulation. Sim succeeds (chain fix covers the chained path; even if it falls to single-tx, the now-existing on-chain allowance makes the directTx pass).
8. Sign + send the EIP-712 intent. Tx confirms on anvil.
9. Run "swap 5000 USDC → ETH" again with allowance now in place — UI should skip the approval phase and go straight to simulation success.

## Out of scope

- Not investigating the compiler's empty-prereq emission in this PR; the diagnostic log captures the data we need to scope a follow-up.
- Not re-running the WASM build (`pnpm build:wasm`) — TypeScript-side fix only.
- Not changing the chain-sim implementation (already fixed in the previous PR).
