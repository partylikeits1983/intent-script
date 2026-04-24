//! B4: Per-token `max_spend` cap tests.
//!
//! AllowancesInput gains an optional `max_spend` map. When set, the
//! compiler rejects any intent whose aggregate required-pull for that
//! token exceeds the cap. Lets the UI bound what a single session can
//! consume regardless of what the LLM produces — the user pre-authorizes
//! an envelope and the compiler refuses to emit anything outside it.

mod common;

use common::compile_anvil_with_allowances;

#[test]
fn deposit_within_max_spend_compiles() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "500", "into": "aave" } }
        ]
    }"#;
    // 500 USDC pull, cap at 1000 USDC — should compile.
    let allowances = r#"{
        "tokens": { "USDC": "1000000000" },
        "max_spend": { "USDC": "1000000000" }
    }"#;

    compile_anvil_with_allowances(input, Some(allowances))
        .expect("pull within cap should compile");
}

#[test]
fn deposit_exceeds_max_spend_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }
        ]
    }"#;
    // Cap at 1000 USDC, intent wants 5000 — reject.
    let allowances = r#"{
        "tokens": { "USDC": "1000000000000" },
        "max_spend": { "USDC": "1000000000" }
    }"#;

    let err = compile_anvil_with_allowances(input, Some(allowances))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("max_spend") || err.contains("pre-authorized"),
        "expected max_spend rejection, got: {err}"
    );
}

#[test]
fn aggregate_required_pull_enforces_cap_across_steps() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "600", "into": "aave" } },
            { "deposit": { "asset": "USDC", "amount": "600", "into": "aave" } }
        ]
    }"#;
    // Cap 1000 USDC; two 600-USDC deposits = 1200 → reject.
    let allowances = r#"{
        "tokens": { "USDC": "1200000000" },
        "max_spend": { "USDC": "1000000000" }
    }"#;

    let err = compile_anvil_with_allowances(input, Some(allowances))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("max_spend") || err.contains("pre-authorized"),
        "expected aggregate-over-cap rejection, got: {err}"
    );
}

#[test]
fn missing_max_spend_entry_does_not_cap() {
    // max_spend covers USDC only; a WETH pull is uncapped.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "deposit": { "asset": "WETH", "amount": "1", "into": "aave" } }
        ]
    }"#;
    let allowances = r#"{
        "tokens": { "WETH": "10000000000000000000" },
        "max_spend": { "USDC": "1000000000" }
    }"#;

    compile_anvil_with_allowances(input, Some(allowances))
        .expect("missing max_spend entry means no cap for that token");
}
