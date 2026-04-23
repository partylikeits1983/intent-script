mod common;

use common::compile_anvil;
use intent_script::CompileOutput;

#[test]
fn batched_intent_without_deadline_source_emits_warning() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let result = compile_anvil(input).expect("compile should succeed");
    assert!(matches!(result.output, CompileOutput::Eip712Intent(_)));
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("Intent has no deadline") && w.contains("current_timestamp")),
        "expected missing-deadline warning, got {:?}",
        result.warnings
    );
}

#[test]
fn batched_intent_with_current_timestamp_emits_no_deadline_warning() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let result = compile_anvil(input).expect("compile should succeed");
    assert!(matches!(result.output, CompileOutput::Eip712Intent(_)));
    assert!(
        result
            .warnings
            .iter()
            .all(|w| !w.contains("Intent has no deadline")),
        "unexpected deadline warning: {:?}",
        result.warnings
    );
}

#[test]
fn batched_intent_with_explicit_deadline_emits_no_deadline_warning() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "deadline": 1712345678,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let result = compile_anvil(input).expect("compile should succeed");
    assert!(matches!(result.output, CompileOutput::Eip712Intent(_)));
    assert!(
        result
            .warnings
            .iter()
            .all(|w| !w.contains("Intent has no deadline")),
        "unexpected deadline warning: {:?}",
        result.warnings
    );
}

#[test]
fn single_tx_without_deadline_source_emits_no_deadline_warning() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1" } }
        ]
    }"#;

    let result = compile_anvil(input).expect("compile should succeed");
    assert!(matches!(result.output, CompileOutput::SingleTx(_)));
    assert!(
        result
            .warnings
            .iter()
            .all(|w| !w.contains("Intent has no deadline")),
        "single-tx path should not emit deadline warning: {:?}",
        result.warnings
    );
}
