//! B12: strict JSON schema + version gate tests.
//!
//! Every step and the top-level IntentScript carry
//! `#[serde(deny_unknown_fields)]`, and the optional `schema_version` must
//! be either absent or exactly "1.0". A hallucinated field like
//! `recipient_override` or `authorize_anyone` inside a step used to be
//! silently ignored (and the step would lower with its defaults). It now
//! rejects at the parser boundary.

mod common;

use common::compile_anvil;

#[test]
fn unknown_field_at_top_level_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "inject_me": "nope",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1" } }
        ]
    }"#;

    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(
        err.contains("unknown field") || err.contains("inject_me"),
        "expected unknown-field rejection, got: {err}"
    );
}

#[test]
fn unknown_field_inside_swap_step_is_rejected() {
    // Exactly the kind of hallucination B1 would otherwise catch at the IR
    // level — rejecting it at parse time is cheaper and surfaces a clearer
    // error to the UI.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            {
                "swap": {
                    "from": "USDC",
                    "amount": "100",
                    "to": "WETH",
                    "min_amount_out": "0.04",
                    "recipient_override": "0xBAD000BAd000BAD000baD000Bad000BAD000bAd0"
                }
            }
        ]
    }"#;

    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(
        err.contains("unknown field") || err.contains("recipient_override"),
        "expected unknown-field rejection inside swap step, got: {err}"
    );
}

#[test]
fn unknown_field_inside_deposit_step_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            {
                "deposit": {
                    "asset": "USDC",
                    "amount": "100",
                    "into": "aave",
                    "deposit_for": "0xBAD000BAd000BAD000baD000Bad000BAD000bAd0"
                }
            }
        ]
    }"#;

    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(
        err.contains("unknown field") || err.contains("deposit_for"),
        "expected unknown-field rejection inside deposit step, got: {err}"
    );
}

#[test]
fn unknown_field_inside_send_step_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            {
                "send": {
                    "asset": "USDC",
                    "amount": "100",
                    "to": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
                    "stealth": true
                }
            }
        ]
    }"#;

    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(
        err.contains("unknown field") || err.contains("stealth"),
        "expected unknown-field rejection inside send step, got: {err}"
    );
}

#[test]
fn unknown_field_in_balances_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "balances": {
            "tokens": { "USDC": "100.0" },
            "secret_mode": "treasury"
        },
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "10", "into": "aave" } }
        ]
    }"#;

    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(
        err.contains("unknown field") || err.contains("secret_mode"),
        "expected unknown-field rejection inside balances, got: {err}"
    );
}

#[test]
fn schema_version_1_0_compiles() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "schema_version": "1.0",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1" } }
        ]
    }"#;

    compile_anvil(input).expect("schema_version 1.0 must compile");
}

#[test]
fn schema_version_2_0_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "schema_version": "2.0",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1" } }
        ]
    }"#;

    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(
        err.contains("schema_version") && err.contains("1.0"),
        "expected version-mismatch rejection, got: {err}"
    );
}

#[test]
fn absent_schema_version_still_compiles_for_backcompat() {
    // v1 JSON without `schema_version` keeps working until we flip the
    // field to required in a future major release.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1" } }
        ]
    }"#;

    compile_anvil(input).expect("absent schema_version must still compile");
}
