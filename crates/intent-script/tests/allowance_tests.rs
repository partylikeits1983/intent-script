mod common;

use common::{compile_anvil_with_allowances, max_allowance_decimal};
use intent_script::CompileOutput;

const USDC_DEPOSIT_INPUT: &str = r#"{
    "network": "anvil",
    "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
    "steps": [
        { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }
    ]
}"#;

#[test]
fn prerequisite_approval_emitted_when_allowance_is_below_required_pull() {
    let allowances = r#"{ "tokens": { "USDC": "4999999999" } }"#;
    let result =
        compile_anvil_with_allowances(USDC_DEPOSIT_INPUT, Some(allowances)).expect("compile ok");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(intent.prerequisite_approvals.len(), 1);
            assert_eq!(
                format!("{}", intent.prerequisite_approvals[0].to),
                "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
            );
        }
        other => panic!("expected Eip712Intent, got {other:?}"),
    }
}

#[test]
fn exact_allowance_match_emits_no_prerequisite_approval() {
    let allowances = r#"{ "tokens": { "USDC": "5000000000" } }"#;
    let result =
        compile_anvil_with_allowances(USDC_DEPOSIT_INPUT, Some(allowances)).expect("compile ok");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(intent.prerequisite_approvals.is_empty());
        }
        other => panic!("expected Eip712Intent, got {other:?}"),
    }
}

#[test]
fn larger_allowance_emits_no_prerequisite_approval() {
    let allowances = format!(
        r#"{{ "tokens": {{ "USDC": "{}" }} }}"#,
        max_allowance_decimal()
    );
    let result =
        compile_anvil_with_allowances(USDC_DEPOSIT_INPUT, Some(&allowances)).expect("compile ok");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(intent.prerequisite_approvals.is_empty());
        }
        other => panic!("expected Eip712Intent, got {other:?}"),
    }
}

#[test]
fn missing_token_entry_is_treated_as_zero_allowance() {
    let allowances = r#"{ "tokens": { "DAI": "1" } }"#;
    let result =
        compile_anvil_with_allowances(USDC_DEPOSIT_INPUT, Some(allowances)).expect("compile ok");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(intent.prerequisite_approvals.len(), 1);
        }
        other => panic!("expected Eip712Intent, got {other:?}"),
    }
}

#[test]
fn unknown_alias_in_allowances_emits_warning_and_does_not_break_compile() {
    let allowances = r#"{ "tokens": { "FAKE": "123", "USDC": "5000000000" } }"#;
    let result =
        compile_anvil_with_allowances(USDC_DEPOSIT_INPUT, Some(allowances)).expect("compile ok");

    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("Allowance entry for unknown asset 'FAKE' ignored")),
        "expected unknown-asset allowance warning, got {:?}",
        result.warnings
    );
}

#[test]
fn malformed_allowance_value_fails_compilation() {
    let allowances = r#"{ "tokens": { "USDC": "not-a-number" } }"#;
    let err = compile_anvil_with_allowances(USDC_DEPOSIT_INPUT, Some(allowances))
        .unwrap_err()
        .to_string();

    assert!(err.contains("Invalid amount"));
}

#[test]
fn native_asset_allowance_entry_is_ignored_with_warning() {
    let allowances = r#"{ "tokens": { "ETH": "1", "USDC": "5000000000" } }"#;
    let result =
        compile_anvil_with_allowances(USDC_DEPOSIT_INPUT, Some(allowances)).expect("compile ok");

    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("Allowance entry for unknown asset 'ETH' ignored")),
        "expected native-asset allowance warning, got {:?}",
        result.warnings
    );
}
