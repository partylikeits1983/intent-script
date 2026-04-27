//! B2: Absolute slippage floor tests.
//!
//! The previous cap was 100% — slippage_bps >= 10_000 was rejected, but
//! anything below that was accepted. A hallucinated "slippage: 50" (50%!)
//! would have compiled and handed the trade to a sandwicher. B2 lowers
//! the ceiling to 500 bps (5%) matching the leverage-sugar cap.

mod common;

use common::compile_anvil;

#[test]
fn slippage_at_cap_compiles() {
    // Exactly 5% — edge case allowed.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "swap": { "from": "USDC", "amount": "100", "to": "WETH",
                        "price": "0.0005", "slippage": "5" } }
        ]
    }"#;
    compile_anvil(input).expect("5% slippage should be within the cap");
}

#[test]
fn slippage_just_over_cap_is_rejected() {
    // 5.01% — just over.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "swap": { "from": "USDC", "amount": "100", "to": "WETH",
                        "price": "0.0005", "slippage": "5.01" } }
        ]
    }"#;
    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(
        err.contains("Slippage") && err.contains("cap"),
        "expected slippage-cap rejection, got: {err}"
    );
}

#[test]
fn hallucinated_50_percent_slippage_is_rejected() {
    // The canonical hallucination: "50" → 50% slippage. Without the
    // absolute cap this silently handed the trade to a sandwicher.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "swap": { "from": "USDC", "amount": "100", "to": "WETH",
                        "price": "0.0005", "slippage": "50" } }
        ]
    }"#;
    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(
        err.contains("Slippage") && err.contains("cap"),
        "expected slippage-cap rejection, got: {err}"
    );
}

#[test]
fn default_slippage_still_works() {
    // When slippage is absent, the compiler defaults to 0.5%; that must
    // still be well under the cap.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "swap": { "from": "USDC", "amount": "100", "to": "WETH",
                        "price": "0.0005" } }
        ]
    }"#;
    compile_anvil(input).expect("default slippage should compile");
}
