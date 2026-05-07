# Plan: Default Uniswap V3 fee tier to 100 (0.01%) for stable-stable swaps + clarify WASM rebuild flow

> **Plan-location note:** project-memory says durable plans live in `intent-script/plans/`. The harness limits this planning phase to the foamy-lemur path; once approved, copy this file to `intent-script/plans/swap-stable-stable-default-fee.md` before implementation.

## Context

A user trying to *swap 50000 USDC to USDT* on the local Base fork is hitting **"Execution reverted for an unknown reason"** at simulation time. The `to` address is the freshly-deployed IntentRouter and the inner call is `exactInputSingle` on Uniswap V3's `SwapRouter02`. Asset addresses, allowlist, approval, chain id, and EIP-712 domain are all consistent — the swap call itself is the only thing reverting.

**Root cause** (verified by Explore): the compiler currently defaults the Uniswap V3 fee tier to **3000** (0.3%) at `intent-script/crates/intent-script/src/compiler/normalize.rs:418`. On Base the canonical USDC↔USDT V3 pool is at fee **100** (0.01%); the 3000-tier pool either doesn't exist or has zero liquidity. When `factory.getPool(USDC, USDT, 3000)` returns `address(0)`, `exactInputSingle` calls `swap(...)` against the zero address, which the EVM treats as a no-op — success with empty return data. SwapRouter02 then fails to decode that empty return value and reverts with **empty bytes**, which viem surfaces as *"Execution reverted for an unknown reason"*.

Last round we already (a) added `100` to the compiler's accepted fee-tier set (`parse_uniswap_fee_tier`), (b) added `100` to the UI tool schema, (c) added system-prompt guidance telling the LLM to use `100` for stable-stable swaps. Despite that, the user is still seeing the same revert because:

1. **The WASM bundle wasn't rebuilt.** The Rust changes live in `intent-script/crates/intent-script/`, but the UI loads a pre-built bundle from `intentOS-ui/public/intent_script_wasm_bg.wasm`. Next.js doesn't watch Rust source — `pnpm dev` keeps serving the stale `.wasm` until the user runs `pnpm build:wasm` (which invokes `wasm-pack` and copies the output into `lib/wasm/intent-script/` and `public/`).
2. **Even after a rebuild, the LLM still has to *choose* fee 100.** The system prompt tells it to, but LLMs occasionally drop hints, especially when the user gives terse follow-ups like *"spot, min slippage"* that bypass deeper reasoning.

The intended outcome:

- **The compiler stops relying on the LLM to pick the right fee tier for stable pairs.** When the user omits `fee` and both `from` and `to` resolve to known stablecoins, the compiler picks **100** automatically. Otherwise it keeps the current `3000` default. The LLM's explicit `fee: "100"` (or any other tier) is still respected.
- **The user's "swap 50k USDC to USDT" intent compiles and simulates successfully** without any further prompt-engineering or LLM intervention.
- **A clear rebuild-WASM checklist is documented** so future Rust-side changes don't silently fail in the running UI.

## Design

### 1. Stable-stable default fee tier in the Rust compiler

In `intent-script/crates/intent-script/src/compiler/normalize.rs`, the `Step::Swap` arm (lines 390–470) has `s.from` and `s.to` (raw user-supplied symbols, e.g. `"USDC"`, `"USDT"`) in scope when the fee tier is parsed at line 418. Add a small helper:

```rust
/// Returns true for asset symbols that are USD-pegged stablecoins. Used to
/// pick a smarter default Uniswap V3 fee tier for stable-stable pairs (the
/// 0.01% / fee=100 tier holds the deep liquidity for USDC↔USDT, USDC↔DAI
/// etc.). Case-insensitive to tolerate "usdc" vs "USDC" hand-typed input.
/// Mirrors the UI's `STABLE_SYMBOLS` set in
/// `intentOS-ui/lib/portfolio-summary.ts:5` so the two layers stay in sync;
/// any new stable added one place should be added the other too.
fn is_stable_symbol(alias: &str) -> bool {
    matches!(
        alias.to_ascii_uppercase().as_str(),
        "USDC" | "USDT" | "USDBC" | "DAI" | "USDE" | "FRAX" | "SDAI"
    )
}
```

Then change line 418 from:

```rust
let fee: u32 = s.fee.as_deref().unwrap_or("3000").parse().map_err(...)?;
```

to:

```rust
let default_fee_tier = if is_stable_symbol(&s.from) && is_stable_symbol(&s.to) {
    "100"
} else {
    "3000"
};
let fee: u32 = s.fee.as_deref().unwrap_or(default_fee_tier).parse().map_err(...)?;
```

This is the minimal, surgical change. It only kicks in when the user/LLM omits `fee` — explicit `fee: "3000"` on a stable pair still routes to the 3000 pool (and reverts, but that's the user's choice).

### 2. New integration test

Add to `intent-script/crates/intent-script/tests/integration_base.rs`:

- `test_base_swap_usdc_to_usdt_defaults_to_fee_100`: compile a `swap` step with `from: "USDC"`, `to: "USDT"`, no `fee` field; assert the compiled `UniswapV3Swap` IR carries `fee: 100`.
- `test_base_swap_usdc_to_weth_defaults_to_fee_3000`: compile a `swap` step with `from: "USDC"`, `to: "WETH"`, no `fee`; assert the compiled IR still carries `fee: 3000` (no over-reach).
- `test_base_swap_usdc_to_usdt_explicit_fee_3000_respected`: compile with explicit `fee: "3000"`; assert the IR carries `fee: 3000` (user override wins).

These test names and shape mirror the existing `test_base_swap_weth_to_usdc` so the file stays consistent.

### 3. WASM rebuild checklist (documentation only — no code change)

Add a short note to `intent-script/plans/swap-stable-stable-default-fee.md` (the durable copy of this plan) reminding future contributors:

> Any time you edit Rust under `intent-script/crates/`, the UI's compiled WASM bundle becomes stale. Run **`pnpm build:wasm`** from `intentOS-ui/` to rebuild and copy the new `.wasm` + `.js` glue into `intentOS-ui/public/` and `intentOS-ui/lib/wasm/intent-script/`. `pnpm dev` does NOT auto-detect the new WASM — refresh the browser tab after the rebuild completes.

The checklist is documentation; it doesn't ship as code.

## Files to modify

```
intent-script/crates/intent-script/src/compiler/normalize.rs    # is_stable_symbol helper + conditional default
intent-script/crates/intent-script/tests/integration_base.rs    # 3 new test cases
intent-script/plans/swap-stable-stable-default-fee.md           # durable plan copy with WASM-rebuild note
```

Untouched (intentional):

- The compiler's `parse_uniswap_fee_tier` already accepts `100` (added last round) — no change.
- The UI's tool schema already lists `100` and the system prompt already guides the LLM toward it — no change.
- `intentOS-ui/lib/portfolio-summary.ts::STABLE_SYMBOLS` stays as the UI-side mirror; the two lists are deliberately duplicated rather than abstracted into a shared module, since the WASM compiler has no JS imports.
- No contract changes, no script changes, no UI component changes.

## Critical existing utilities to reuse

- `resolve_asset_address(&s.from, registry)` (`normalize.rs:391`) — already does the alias→address resolution we need; keeping `s.from`/`s.to` as the source of truth for the `is_stable_symbol` check avoids duplicating any of that logic.
- `parse_uniswap_fee_tier` (`normalize.rs:1571`) — already accepts `"100"` from last round, so the new default flows through without further changes.
- `STABLE_SYMBOLS` set in `intentOS-ui/lib/portfolio-summary.ts:5` and `intentOS-ui/lib/uniswap-v3-price.ts:52-53` — the canonical UI-side stable list; the new Rust helper mirrors it exactly.

## Verification

1. **Rust unit + integration tests** (no fork required):
   ```bash
   cd intent-script
   cargo test -p intent-script
   ```
   All 128 existing L1 tests + the 8 prior Base tests + the 3 new `test_base_swap_*` cases pass.

2. **Rebuild WASM** (the step the user has been missing):
   ```bash
   cd intentOS-ui
   pnpm build:wasm
   ```
   Verify `intentOS-ui/public/intent_script_wasm_bg.wasm` mtime updated.

3. **Live retry of the failing swap**:
   - Stop and re-run `./scripts/run-local-anvil-base.sh` (anvil at chain 31337).
   - Restart UI: `cd intentOS-ui && NEXT_PUBLIC_USE_LOCAL_FORK=true pnpm dev`.
   - In the chat: *"swap 50000 USDC to USDT, spot price, min slippage"*. The LLM may or may not include `fee: "100"`; either way the compiler now defaults to it.
   - Approve card appears (50000 USDC → router); confirm.
   - Simulation panel renders pre/post asset deltas successfully (≈ -50000 USDC, +49995 USDT or thereabouts) — no "execution reverted" error.
   - Sign + execute the intent; receipt resolves on the fork.

4. **Negative regression** — explicit `fee: "3000"` should still be honored even on stable pairs (so users debugging tiers can opt out of the default):
   ```json
   { "swap": { "from": "USDC", "to": "USDT", "amount": "100", "fee": "3000", "slippage": "0.5" } }
   ```
   Compile produces `UniswapV3Swap { fee: 3000, ... }`; simulation will revert (no pool), but that's the expected user-driven behavior, not a compiler bug.

5. **Volatile-pair regression** — `swap` from `USDC → WETH` on Base with no `fee` field:
   - Default stays `3000` (USDC/WETH on Base does have a 3000-tier pool with reasonable liquidity, so simulation should succeed).
   - `test_base_swap_usdc_to_weth_defaults_to_fee_3000` asserts this in the test suite.

## Out of scope

- Adding an `is_stable: true` field to the per-network asset JSON files. The hardcoded list is simpler and covers every asset we currently ship; revisit if/when we add a long-tail stablecoin that the hardcoded list doesn't catch.
- Auto-fallback across fee tiers (e.g. retry at fee 100 if the 3000-tier pool reverts). Would require runtime quoter calls and a meaningful refactor to the simulation layer; the deterministic default + explicit override gets us to the right answer for every pair we care about today.
- Updating the system-prompt fee-tier table further. The previous round's edit already names fee 100 as mandatory for stable-stable; once the WASM is rebuilt, both the LLM's explicit choice and the new compiler default route to the same place.
