//! Deadline enforcement tests.
//!
//! The router's `executeSigned` rejects `deadline == 0`. Previously the
//! compiler emitted a warning when a batched intent had no deadline source;
//! that warning is now a hard error because a hallucinating LLM that drops
//! the deadline would otherwise produce a tx that the router is guaranteed
//! to reject. Similarly, an explicit deadline at or before the supplied
//! `current_timestamp` is rejected up front.

mod common;

use common::compile_anvil_raw;
use intent_script::CompileOutput;
use intent_script::error::CompileError;

#[test]
fn batched_intent_without_deadline_source_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let err = compile_anvil_raw(input).expect_err("must error — batched output needs deadline");
    assert!(
        matches!(err, CompileError::DeadlineMissing),
        "expected DeadlineMissing, got {err:?}"
    );
}

#[test]
fn batched_intent_with_current_timestamp_compiles_and_auto_computes_deadline() {
    let current = 1_712_344_000u64;
    let input = format!(
        r#"{{
            "network": "anvil",
            "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
            "current_timestamp": {current},
            "steps": [
                {{ "deposit": {{ "asset": "USDC", "amount": "100", "into": "aave" }} }}
            ]
        }}"#
    );

    let result = compile_anvil_raw(&input).expect("compile should succeed");
    let intent = match result.output {
        CompileOutput::Eip712Intent(i) => i,
        other => panic!("expected Eip712Intent, got {other:?}"),
    };
    // 30-minute auto-deadline contract.
    assert_eq!(
        intent.intent_batch.deadline,
        current + 30 * 60,
        "deadline must be current_timestamp + 30 minutes"
    );
}

#[test]
fn batched_intent_with_explicit_future_deadline_compiles() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "deadline": 1712345678,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let result = compile_anvil_raw(input).expect("compile should succeed");
    assert!(matches!(result.output, CompileOutput::Eip712Intent(_)));
}

#[test]
fn batched_intent_with_explicit_past_deadline_is_rejected() {
    // deadline in the past relative to current_timestamp — the router
    // would reject the signed intent; catch it at compile time.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712345000,
        "deadline": 1712344000,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let err = compile_anvil_raw(input).expect_err("past deadline must be rejected");
    match err {
        CompileError::DeadlineInPast {
            deadline,
            current_timestamp,
        } => {
            assert_eq!(deadline, 1_712_344_000);
            assert_eq!(current_timestamp, 1_712_345_000);
        }
        other => panic!("expected DeadlineInPast, got {other:?}"),
    }
}

#[test]
fn batched_intent_with_deadline_equal_to_current_timestamp_is_rejected() {
    // Boundary: deadline == current_timestamp. The router enforces
    // `block.timestamp <= deadline`, so an intent with deadline == now is
    // only safe at the single block it's mined in — effectively unusable
    // by the time it reaches a relayer.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "deadline": 1712344000,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let err = compile_anvil_raw(input).expect_err("boundary-equal deadline must be rejected");
    assert!(
        matches!(err, CompileError::DeadlineInPast { .. }),
        "expected DeadlineInPast, got {err:?}"
    );
}

#[test]
fn single_tx_without_deadline_source_still_compiles() {
    // The single-tx path uses `executeDirect`, which does not enforce a
    // deadline. No rejection, no warning.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1" } }
        ]
    }"#;

    let result = compile_anvil_raw(input).expect("single-tx without deadline is fine");
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
