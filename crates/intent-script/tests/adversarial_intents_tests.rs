//! Capstone adversarial-intent coverage.
//!
//! Each test represents a hallucination shape the security plan was
//! specifically designed to defeat. A passing run of this file means:
//! ten hand-crafted malicious intents, every one rejected at compile
//! time with a specific CompileError variant — zero produce
//! ConcreteCall output that could be signed and broadcast.
//!
//! Add new entries here whenever a new attack shape is identified or a
//! new guardrail lands.

mod common;

use common::{compile_anvil, compile_anvil_raw, compile_anvil_with_allowances};

#[test]
fn adversarial_1inch_calldata_is_rejected() {
    // Removed adapter: pre-fetched calldata passthrough was the single
    // largest hallucination-escape hatch. Any legacy "via: 1inch" intent
    // must now reject.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "swap": { "from": "USDC", "amount": "100", "to": "WETH",
                        "via": "1inch", "calldata": "0xdeadbeef" } }
        ]
    }"#;
    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(err.contains("1inch") && err.contains("uniswap"));
}

#[test]
fn adversarial_missing_deadline_is_rejected() {
    // Router's executeSigned requires deadline > 0. Batched intents
    // without a source now reject at compile time.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;
    compile_anvil_raw(input).expect_err("missing deadline must reject");
}

#[test]
fn adversarial_past_deadline_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712345000,
        "deadline": 1712344000,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;
    compile_anvil_raw(input).expect_err("past deadline must reject");
}

#[test]
fn adversarial_hallucinated_recipient_override_field_is_rejected() {
    // B12: unknown-field rejection at parse time. A hallucinated
    // recipient_override would otherwise be silently ignored.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "swap": { "from": "USDC", "amount": "100", "to": "WETH",
                        "min_amount_out": "0.04",
                        "recipient_override": "0xBAD000BAd000BAD000baD000Bad000BAD000bAd0" } }
        ]
    }"#;
    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(err.contains("unknown field") || err.contains("recipient_override"));
}

#[test]
fn adversarial_slippage_50_percent_is_rejected() {
    // B2: 5% absolute cap. A hallucinated 50% slippage would otherwise
    // have silently produced a signed trade any sandwicher could drain.
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
    assert!(err.contains("Slippage") && err.contains("cap"));
}

#[test]
fn adversarial_extra_zero_wrap_amount_is_rejected() {
    // B5: call budget. Canonical "extra zero" hallucination.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "10000000000" } }
        ]
    }"#;
    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(err.contains("per-call cap") || err.contains("wei"));
}

#[test]
fn adversarial_deposit_exceeds_wallet_balance_is_rejected() {
    // B3: wallet-aware amount flow.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "balances": { "tokens": { "USDC": "100.0" } },
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "500", "into": "aave" } }
        ]
    }"#;
    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(err.contains("running balance") || err.contains("guarantees"));
}

#[test]
fn adversarial_spend_exceeds_pre_authorized_session_cap_is_rejected() {
    // B4: per-token max_spend cap.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }
        ]
    }"#;
    let allowances = r#"{
        "tokens": { "USDC": "10000000000" },
        "max_spend": { "USDC": "1000000000" }
    }"#;
    let err = compile_anvil_with_allowances(input, Some(allowances))
        .unwrap_err()
        .to_string();
    assert!(err.contains("max_spend") || err.contains("pre-authorized"));
}

#[test]
fn adversarial_future_schema_version_is_rejected() {
    // B12: schema_version gate.
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
    assert!(err.contains("schema_version") && err.contains("1.0"));
}

#[test]
fn adversarial_unknown_network_is_rejected() {
    // Longstanding guard: unknown networks don't silently fall through.
    let input = r#"{
        "network": "fantasy",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1" } }
        ]
    }"#;
    compile_anvil(input).expect_err("unknown network must reject");
}
