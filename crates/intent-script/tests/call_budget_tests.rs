//! B5: Call budget tests.
//!
//! validate_call_budget runs after lower::lower and caps:
//!   - per-call ETH value at 1000 ETH
//!   - aggregate ETH value at 10,000 ETH across the batch
//!   - concrete-call count at 24 (generous headroom above MAX_STEPS=5)
//!
//! These are defensive bounds that catch hallucinated overflow shapes
//! (`value: 10^30`) without constraining any realistic intent.

mod common;

use common::compile_anvil;

#[test]
fn single_wrap_at_per_call_cap_compiles() {
    // Exactly 1000 ETH — edge case allowed.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1000" } }
        ]
    }"#;
    compile_anvil(input).expect("1000 ETH wrap should be within the cap");
}

#[test]
fn single_wrap_over_per_call_cap_is_rejected() {
    // 1001 ETH — just over the cap.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1001" } }
        ]
    }"#;
    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(
        err.contains("per-call cap") || err.contains("wei"),
        "expected call-budget rejection, got: {err}"
    );
}

#[test]
fn hallucinated_absurd_wrap_amount_is_rejected() {
    // The classic "extra zero" hallucination — 10^10 ETH.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "10000000000" } }
        ]
    }"#;
    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(
        err.contains("per-call cap") || err.contains("wei"),
        "expected call-budget rejection, got: {err}"
    );
}
