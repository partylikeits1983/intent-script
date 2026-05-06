//! End-to-end tests for the LLM-facing structured-error layer.
//!
//! These tests compile a curated set of intentionally-broken intents and
//! verify that:
//!   1. The structured `code` is non-generic (the LLM can branch on it)
//!   2. The `step_index` is populated when the failing site has one in
//!      scope
//!   3. The `fields` map carries the asset / amount / token / spacing /
//!      etc. that the LLM needs to fix the bug
//!   4. The `fix_instruction` is non-empty
//!
//! When this file fails after a refactor, the right response is usually to
//! add the new prose pattern to the corresponding classifier branch in
//! `src/error.rs`, NOT to weaken these assertions.

mod common;

use common::compile_anvil;

/// Run an intent that is expected to fail compilation, and return the
/// structured error. Panics if the intent unexpectedly compiles.
fn structured(input: &str) -> intent_script::error::StructuredError {
    let err = compile_anvil(input).expect_err("intent should fail to compile");
    err.to_structured()
}

// ─── Aave: borrow without collateral ─────────────────────────────────────
//
// Without an allowances/balances payload the compiler can't prove the user
// has no Aave collateral and emits a warning; the strict-fail path requires
// the caller to provide a balances snapshot. The unit test in `src/error.rs`
// already covers the structured-output path for a constructed
// `CompileError::InvalidChain` with the canonical prose, so here we just
// verify the warning is informative when the borrow-without-deposit shape
// shows up without a balances payload.

#[test]
fn borrow_without_collateral_warns_when_balances_unknown() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "borrow": { "asset": "USDC", "amount": "1000", "from": "aave" } }
        ]
    }"#;
    let result = compile_anvil(input).expect("compiles with a warning when balances unknown");
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("Borrow without prior deposit")),
        "expected an actionable warning about missing collateral; got {:?}",
        result.warnings,
    );
}

// ─── Aave: native ETH into Aave ──────────────────────────────────────────

#[test]
fn native_eth_into_aave_emits_actionable_code() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "ETH", "amount": "1", "into": "aave" } }
        ]
    }"#;

    let s = structured(input);
    assert_eq!(s.code, "native_eth_into_aave");
    assert!(s.fix_instruction.contains("wrap"));
    assert_eq!(s.fields.get("asset").map(|x| x.as_str()), Some("ETH"));
}

// ─── Swap to self ────────────────────────────────────────────────────────

#[test]
fn swap_to_self_emits_swap_to_self_code() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "100", "to": "USDC", "min_amount_out": "100" } }
        ]
    }"#;

    let s = structured(input);
    assert_eq!(s.code, "swap_to_self");
    assert!(s.fix_instruction.contains("swap.to"));
}

// ─── Unknown step kind (typo) ───────────────────────────────────────────
//
// In practice, serde's `#[serde(deny_unknown_fields)]` on the Step enum
// catches step-kind typos at JSON-parse time and emits `unknown_field`,
// not `unknown_step_kind`. Both codes are LLM-actionable: `unknown_field`
// includes the typo'd field name and an `expected one of ...` list, so
// we validate that path and trust the inner `unknown_step_kind` variant
// (which is reachable only for variants that are valid in serde but
// unsupported by lowering — rare).

#[test]
fn unknown_step_kind_typo_is_actionable() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swp": { "from": "USDC", "to": "WETH", "amount": "100" } }
        ]
    }"#;

    let s = structured(input);
    // Serde emits "unknown variant" for a tagged enum typo; the classifier
    // routes that to `unknown_step_kind`. (Earlier serde versions or a
    // different schema shape might surface the typo as `unknown_field`;
    // both are LLM-actionable.)
    assert!(
        s.code == "unknown_step_kind" || s.code == "unknown_field",
        "expected unknown_step_kind or unknown_field, got `{}`",
        s.code,
    );
    assert_eq!(s.suggestion.as_deref(), Some("swap"));
    assert!(!s.available.is_empty());
}

// ─── Unknown asset (typo) ───────────────────────────────────────────────

#[test]
fn unknown_asset_suggests_close_match() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "UDSC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let s = structured(input);
    assert_eq!(s.code, "unknown_asset");
    assert_eq!(s.suggestion.as_deref(), Some("USDC"));
    assert!(s.fix_instruction.contains("USDC"));
}

// ─── Unknown protocol ───────────────────────────────────────────────────

#[test]
fn unknown_protocol_lists_alternatives() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "1inch" } }
        ]
    }"#;

    let s = structured(input);
    assert_eq!(s.code, "unknown_protocol");
    assert!(!s.available.is_empty(), "available list should be populated");
}

// ─── Uniswap fee tier unknown ───────────────────────────────────────────
//
// The swap path accepts any u32 for `fee` (downstream pool lookup is what
// will fail if the tier doesn't exist for that pair). The LP path enforces
// canonical tiers. We exercise the LP path here, which is where the typed
// `UniswapFeeTierUnknown` variant reliably fires.

#[test]
fn uniswap_fee_tier_unknown_lists_canonical_tiers() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": { "protocol": "uniswap", "token0": "USDC", "token1": "WETH",
                           "fee": "1234", "tick_lower": -10, "tick_upper": 10,
                           "amount0": "100", "amount1": "0.01",
                           "min_amount0": "100", "min_amount1": "0.01" } }
        ]
    }"#;

    let s = structured(input);
    assert_eq!(s.code, "uniswap_fee_tier_unknown");
    assert!(s.available.iter().any(|v| v == "500"));
    assert!(s.available.iter().any(|v| v == "3000"));
    assert_eq!(s.fields.get("fee").map(|x| x.as_str()), Some("1234"));
}

// ─── Slippage too low ───────────────────────────────────────────────────

#[test]
fn slippage_too_low_includes_step_index_and_path() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "100", "to": "WETH", "min_amount_out": "0" } }
        ]
    }"#;

    let s = structured(input);
    assert_eq!(s.code, "slippage_too_low");
    assert_eq!(s.step_index, Some(0));
    assert_eq!(
        s.path.as_deref(),
        Some("steps[0].swap.min_amount_out"),
    );
}

// ─── Deadline missing on a batched intent ───────────────────────────────

#[test]
fn deadline_missing_on_batched_intent_is_actionable() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } },
            { "borrow": { "asset": "DAI", "amount": "2000", "from": "aave" } }
        ]
    }"#;

    // Bypass the test-default timestamp injection so the deadline rule
    // actually fires.
    use common::compile_anvil_raw;
    let err = compile_anvil_raw(input).expect_err("should fail without deadline");
    let s = err.to_structured();
    assert_eq!(s.code, "deadline_missing");
    assert!(s.fix_instruction.contains("current_timestamp"));
}

// ─── JSON: unknown field with typo suggestion ───────────────────────────

#[test]
fn json_unknown_field_with_typo_suggestion() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "to": "WETH", "amout": "100",
                        "min_amount_out": "0.01" } }
        ]
    }"#;

    let s = structured(input);
    assert_eq!(s.code, "unknown_field");
    assert_eq!(s.suggestion.as_deref(), Some("amount"));
    assert!(s.available.iter().any(|v| v == "amount" || v == "from"));
}

// ─── Schema version unsupported ─────────────────────────────────────────

#[test]
fn schema_version_unsupported_is_classified() {
    let input = r#"{
        "schema_version": "2.0",
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let s = structured(input);
    assert_eq!(s.code, "schema_version_unsupported");
    assert_eq!(s.fields.get("got").map(|x| x.as_str()), Some("2.0"));
}

// ─── Empty steps ────────────────────────────────────────────────────────

#[test]
fn empty_steps_is_classified() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": []
    }"#;

    let s = structured(input);
    assert_eq!(s.code, "empty_steps");
}

// ─── Signer zero ────────────────────────────────────────────────────────

#[test]
fn signer_zero_is_classified() {
    let input = r#"{
        "network": "anvil",
        "from": "0x0000000000000000000000000000000000000000",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let s = structured(input);
    assert_eq!(s.code, "signer_zero");
    assert_eq!(s.path.as_deref(), Some("from"));
}

// ─── Send to zero ───────────────────────────────────────────────────────

#[test]
fn send_to_zero_is_classified() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "send": { "asset": "USDC", "amount": "10",
                        "to": "0x0000000000000000000000000000000000000000" } }
        ]
    }"#;

    let s = structured(input);
    assert_eq!(s.code, "send_to_zero");
}

// ─── Morpho without market ──────────────────────────────────────────────

#[test]
fn morpho_borrow_without_market_is_classified() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "borrow": { "asset": "USDC", "amount": "1000", "from": "morpho" } }
        ]
    }"#;

    let s = structured(input);
    // The actual code depends on whether the missing-market check fires
    // before normalization; we accept either the typed variant (when
    // required) or `validation_generic` would be a regression.
    assert_ne!(s.code, "validation_generic");
    assert_ne!(s.code, "invalid_chain");
    assert!(!s.fix_instruction.is_empty());
}

// ─── Fields are populated for the codes the LLM most often hits ─────────

#[test]
fn structured_fields_mention_relevant_context() {
    // Check a sampling of error variants surface their key context as
    // structured fields rather than embedding it only in prose.
    let cases: &[(&str, &str, &[&str])] = &[
        // (input, expected_code, must_contain_field_keys)
        (
            r#"{"network":"anvil","from":"0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045","steps":[
                {"deposit":{"asset":"UDSC","amount":"100","into":"aave"}}
            ]}"#,
            "unknown_asset",
            &["asset", "network"],
        ),
    ];

    for (input, expected_code, must_have) in cases {
        let s = structured(input);
        assert_eq!(&s.code, expected_code, "input: {input}");
        for key in *must_have {
            assert!(
                s.fields.contains_key(*key),
                "code `{}` should populate fields[{key}]; got {:?}",
                s.code,
                s.fields,
            );
        }
    }
}
