# Plan: Make compiler errors LLM-actionable

> **Note on plan location**: per project convention these live in
> `intent-script/plans/`, but plan-mode lets me edit only this file. After
> approval I'll copy this to `intent-script/plans/llm-actionable-compile-errors.md`.

## Context

The intent-script Rust compiler already has good structured-error machinery
(`StructuredError` in `crates/intent-script/src/error.rs`) with `code`,
`stage`, `step_index`, `path`, `fields`, `suggestion`, `available`, `hint`,
and `fix_instruction`. The WASM boundary emits this as JSON
(`INTENT_ERROR_V1::{json}`), and the UI's `system-prompt.md` already teaches
the LLM the contract (read `code`, apply `fix_instruction`).

Despite that, an LLM trying to self-correct a JSON DSL bug today gets
inconsistent quality:

1. **Lossy classification.** `CompileError::Validation(String)` and
   `CompileError::InvalidChain(String)` collapse ~50 distinct failure modes
   into one variant whose only payload is prose. A downstream classifier
   (`classify_validation_message`, `classify_invalid_chain_message` in
   `error.rs:524-` and `error.rs:723-`) substring-matches the prose to
   recover a stable code. Any raise site whose wording drifts silently falls
   through to `validation_generic` with this fix-instruction:
   _"Read the message, adjust the indicated step, and re-emit the intent."_
   That gives the LLM nothing.
2. **Step index by regex.** `extract_step_index` (`error.rs:512`)
   strips a leading `"Step N:"` prefix off the prose to recover the step
   index. The context is right there at every `for (i, step) in
   steps.iter().enumerate()` raise site — it just isn't being passed.
3. **`fields` map is empty for many high-traffic codes.** E.g.
   `borrow_without_collateral` doesn't tell the LLM which asset, which
   amount, or what the existing collateral situation is. The LLM has to
   guess from prose.
4. **Adapter errors are flat.** All 26 distinct adapter failure modes
   (Aave pool missing, Morpho market field missing, Uniswap fee tier
   unknown, etc.) emit the same `code: "adapter_error"` with no sub-code.
5. **CLI is prose-only.** The WASM binding emits structured JSON but
   `crates/intent-script/src/main.rs:45-71` prints only Display. Any
   agentic flow that drives the CLI gets the worst error surface.
6. **System prompt doesn't enumerate codes.** `intentOS-ui/lib/system-prompt.md`
   has playbook patterns A-F (lines 260-282) but no authoritative reference
   table mapping codes to fix shapes, so it drifts as new codes land.

## Decisions (from clarifying Qs)

- **Refactor scope: surgical.** Keep `Validation(String)` and
  `InvalidChain(String)`. Harden the classifier so every raise site maps to
  a non-generic code; embed step_index in parsable form where known; add a
  regression test that no raise site falls through to `validation_generic`.
- **Adapter codes: split into typed sub-codes.**
  `protocol_contract_missing`, `adapter_field_missing`,
  `uniswap_fee_tier_unknown`, `morpho_market_required`,
  `aave_asset_not_supported` — driven by what the 26 raise sites actually
  fail on.
- **System prompt: add error-codes reference.** Append a concise table
  mapping each compile-error `code` to the canonical fix shape.
- **CLI: add `--structured` flag** — emits `to_structured()` JSON on
  failure for agentic use.

## Work breakdown

### 1. Audit every Validation / InvalidChain raise site
**Files:** `crates/intent-script/src/compiler/validate.rs`,
`crates/intent-script/src/compiler/normalize.rs`,
`crates/intent-script/src/compiler/budget.rs` (if present),
`crates/intent-script/src/compiler/leverage.rs`.

For each `Err(CompileError::Validation(...))` / `Err(CompileError::InvalidChain(...))`:
- Confirm the prose contains the exact substring the classifier is looking
  for (e.g. `"borrow requires collateral"`). Any drift → fix the prose OR
  add a new classifier branch.
- Where the call site has a step index in scope, ensure the prose starts
  with `"Step {i+1}: "` so `extract_step_index` recovers it. (1-based for
  the human, 0-based when surfaced to the LLM.)
- Where the call site has the offending asset / protocol / amount in
  scope, embed it in the prose in a parseable position so the classifier
  can lift it into `fields`.

### 2. Enrich the prose-classifier in `error.rs`
**File:** `crates/intent-script/src/error.rs`.

For each currently-recognized pattern in `classify_validation_message` and
`classify_invalid_chain_message`:
- Populate `fields` with the asset symbol, amount, protocol — extracted via
  small helpers that pull single-quoted tokens out of the prose (mirrors
  what `extract_step_index` does for the index).
- Tighten `fix_instruction` to be a one-liner the LLM can apply mechanically
  (e.g. _"Replace `swap.to` with a token different from `swap.from = '{asset}'`"_).
- Where useful, add `path` like `steps[N].swap.to` instead of leaving it
  `None`.

Add new classifier branches for high-traffic raise sites currently falling
through to `validation_generic`. Concrete additions (driven by what
`compiler/validate.rs` and `compiler/normalize.rs` actually raise):

| New code                           | Trigger prose substring                              |
|------------------------------------|------------------------------------------------------|
| `flashloan_balance_drift`          | "consumes ... but only ... available"                |
| `flashloan_inner_steps_exceeded`   | "max ... inner steps"                                |
| `aave_asset_not_supported`         | "asset ... not supported by Aave"                    |
| `lp_tick_misaligned`               | "tick ... not aligned to spacing"                    |
| `lp_range_invalid`                 | "tick_lower >= tick_upper"                           |
| `swap_amount_zero_or_missing`      | "swap.amount must be ..."                            |
| `running_balance_underflow`        | "running balance ... underflow"                      |

### 3. Split `CompileError::Adapter`
**Files:** `crates/intent-script/src/error.rs` and every adapter under
`crates/intent-script/src/adapters/`.

Replace `CompileError::Adapter(String)` with structured variants:
```rust
ProtocolContractMissing { protocol: String, contract: String },
AdapterFieldMissing { adapter: &'static str, field: &'static str, step_index: Option<usize> },
UniswapFeeTierUnknown { fee: u32, available: Vec<u32> },
MorphoMarketRequired { step_index: Option<usize> },
AaveAssetNotSupported { asset: String, action: &'static str, step_index: Option<usize> },
AdapterOther(String), // catch-all for the long tail; classifier handles it
```

Sweep adapter raise sites (26 total per the audit) — most are at
`adapters/aave_v3.rs`, `adapters/morpho.rs`, `adapters/uniswap_v3*.rs`,
`adapters/lido.rs`. Keep `Display` strings unchanged where possible to
avoid breaking existing prose-asserting tests.

Add corresponding arms to `to_structured()`:
- `protocol_contract_missing` — fields: `protocol`, `contract`. fix:
  _"This is a registry config gap, not a bad intent. Ask the user to
  report it; do not retry."_
- `adapter_field_missing` — fields: `adapter`, `field`. fix: _"Add the
  required `{field}` field to the `{adapter}` step at steps[{N}]."_
- `uniswap_fee_tier_unknown` — fields: `fee`, `available`. fix: _"Use one
  of the supported fee tiers for this token pair: {available}."_
- `morpho_market_required` — fix: _"Morpho steps require an explicit
  `market` field. Add `\"market\": \"<id>\"` (use the user's
  Morpho-positions block to find an active market)."_
- `aave_asset_not_supported` — fields: `asset`, `action`. fix: _"Aave on
  this network has no `{action}` reserve for `{asset}`. Use a supported
  asset, or route through a swap first."_

### 4. Add CLI `--structured` flag
**File:** `crates/intent-script/src/main.rs`.

Add `--structured` to the existing arg parsing. On compile failure with the
flag set, call `e.to_structured()`, serialize to JSON via `serde_json`, and
print to stderr (matching the WASM `INTENT_ERROR_V1::{json}` shape so
agentic CLI flows can reuse the same parser).

### 5. Update `intentOS-ui/lib/system-prompt.md`
**File:** `intentOS-ui/lib/system-prompt.md`.

Append a new section right after the existing playbook patterns (~line 282)
titled `### Compile error codes reference`. Lists every stable code from
`error.rs` `to_structured()`, the typical `fields` keys, and a one-line
canonical fix. Keep it terse (one row per code) so it doesn't bloat the
context window.

The list comes from a single source of truth: `error.rs::to_structured()`.
Where `code` is set to a literal `&'static str`, lift those strings into a
`pub const ALL_COMPILE_ERROR_CODES: &[&str]` at the bottom of `error.rs`,
and add a unit test asserting every arm of `to_structured()` returns a code
present in that list. The reference table in the prompt is hand-written
(not auto-generated) but the const + test catch drift.

### 6. Regression test: no raise site falls through to `validation_generic`
**File:** new test file `crates/intent-script/tests/error_classification_tests.rs`.

A table-driven test that for each known raise pattern (a curated list of
~30 invalid intents — one per Validation/InvalidChain branch + the new
adapter sub-codes) compiles the intent and asserts:
- `code != "validation_generic"` and `code != "adapter_other"`
- `step_index` is populated when the raise site has one in scope
- `fields` contains the expected keys (e.g. `borrow_without_collateral`
  should have `asset`)

This is the linchpin — once it passes, future maintainers who edit prose
or add raise sites without classifier coverage will see the test fail.

### 7. Existing tests
**Files:** `crates/intent-script/tests/adversarial_intents_tests.rs`,
`crates/intent-script/tests/protocol_negative_tests.rs`,
`crates/intent-script/tests/schema_strictness_tests.rs`.

These assert on `err.contains("substring")`. Display strings are unchanged
(decision: keep prose so existing assertions keep passing); only structured
outputs change. Run all three test files at the end to confirm no
regressions; only update assertions that were checking prose that we
changed during the audit.

## Files to modify

| File                                                              | Change |
|-------------------------------------------------------------------|--------|
| `crates/intent-script/src/error.rs`                               | Add adapter sub-variants; new classifier branches; populate `fields` for existing arms; `ALL_COMPILE_ERROR_CODES` const + drift test |
| `crates/intent-script/src/compiler/validate.rs`                   | Audit raise sites; embed step_index/asset/protocol in parsable prose |
| `crates/intent-script/src/compiler/normalize.rs`                  | Same audit |
| `crates/intent-script/src/compiler/leverage.rs`                   | Same audit (smaller) |
| `crates/intent-script/src/adapters/{aave_v3,morpho,uniswap_v3,uniswap_v3_lp,lido,balancer,wrap,erc20,across,send}.rs` | Replace `CompileError::Adapter(format!(...))` with the new typed variants |
| `crates/intent-script/src/main.rs`                                | Add `--structured` flag |
| `crates/intent-script/tests/error_classification_tests.rs`        | New regression test (table-driven) |
| `intentOS-ui/lib/system-prompt.md`                                | Append compile-error codes reference table |
| `intent-script/plans/llm-actionable-compile-errors.md`            | Mirror this plan post-approval |

No changes to:
- `crates/intent-script-wasm/src/lib.rs` — already emits structured JSON
- `intentOS-ui/lib/intent-errors.ts` — TypeScript shape already matches
- `intentOS-ui/components/finalize-intent-tool.tsx` — retry loop already wired

## Verification

```bash
# 1. Compiler unit tests + new regression test
cargo test -p intent-script --tests

# 2. Make sure existing fork tests still pass
cd contracts && forge build && cd ..
cargo test -p intent-script --test generate_calldata
cargo test -p intent-script --test generate_integration_fixtures

# 3. CLI structured output smoke test
cargo run -p intent-script -- compile --input - --structured \
  <<< '{"network":"ethereum","from":"0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045","steps":[{"borrow":{"asset":"USDC","amount":"1000","from":"aave"}}]}'
# Expect stderr: INTENT_ERROR_V1::{"pipeline":"compile","code":"borrow_without_collateral",...}

# 4. Hand-craft 5 broken intents and confirm structured output is fix-applicable:
#    - swap to self
#    - borrow without collateral
#    - native ETH into Aave
#    - flashloan balance drift (test 3 from the prior session)
#    - unknown protocol
#    For each, verify `code`, `step_index`, `fields`, and `fix_instruction`
#    are populated and the fix_instruction would actually fix the bug if
#    applied verbatim.

# 5. Round-trip: take an intent that fails compilation, apply the
#    `fix_instruction` mechanically, verify the corrected intent compiles.
#    This is the test the LLM will do at runtime.
```

Acceptance:
- All existing tests pass.
- The new regression test passes for ~30 known-bad intents.
- For each broken intent in step 4, `fix_instruction` is concrete enough
  that mechanical application yields a compiling intent.

## Out of scope

- Fuzz testing the classifier with adversarial prose drift.
- Auto-generating the prompt's reference table from `error.rs` at build
  time. (Const + drift test catches it without build-time codegen.)
- Reworking the simulation-side `StructuredError` shape (lives in the UI;
  already mirrored).
- Adding a REST `/api/compile` endpoint with structured errors. (Mentioned
  in user memory as planned but not yet built — separate effort.)
- Localizing error messages.
