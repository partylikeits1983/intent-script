mod common;

use common::{compile_anvil, compile_anvil_without_router};
use intent_script::CompileOutput;

#[test]
fn single_call_plans_to_single_tx() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1" } }
        ]
    }"#;

    let result = compile_anvil(input).expect("compile should succeed");
    assert!(matches!(result.output, CompileOutput::SingleTx(_)));
}

#[test]
fn multi_call_with_router_plans_to_batched_eip712_intent() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let result = compile_anvil(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(intent.direct_tx.to, intent.domain.verifying_contract);
            assert!(!intent.intent_batch.calls.is_empty());
        }
        other => panic!("expected Eip712Intent, got {other:?}"),
    }
}

#[test]
fn multi_call_without_router_plans_to_tx_sequence() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let result = compile_anvil_without_router(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::TxSequence(txs) => {
            assert_eq!(txs.len(), 2, "deposit should lower to approve + supply");
        }
        other => panic!("expected TxSequence, got {other:?}"),
    }
}

#[test]
fn requires_router_step_stays_batched_even_with_single_user_step() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            {
                "flashloan": {
                    "via": "balancer",
                    "assets": [{ "asset": "WETH", "amount": "2.0" }],
                    "then": [
                        { "deposit": { "asset": "WETH", "amount": "2.0", "into": "aave" } },
                        { "borrow": { "asset": "USDC", "amount": "4000", "from": "aave" } },
                        { "swap": { "from": "USDC", "amount": "4000", "to": "WETH", "min_amount_out": "2.0" } }
                    ]
                }
            }
        ]
    }"#;

    let result = compile_anvil(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(!intent.intent_batch.calls.is_empty());
        }
        other => panic!("expected Eip712Intent, got {other:?}"),
    }
}

#[test]
fn requires_router_step_without_router_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            {
                "flashloan": {
                    "via": "balancer",
                    "assets": [{ "asset": "WETH", "amount": "2.0" }],
                    "then": [
                        { "deposit": { "asset": "WETH", "amount": "2.0", "into": "aave" } },
                        { "borrow": { "asset": "USDC", "amount": "4000", "from": "aave" } },
                        { "swap": { "from": "USDC", "amount": "4000", "to": "WETH", "min_amount_out": "2.0" } }
                    ]
                }
            }
        ]
    }"#;

    let err = compile_anvil_without_router(input).unwrap_err().to_string();
    // The intent-router config gap surfaces as a typed
    // ProtocolContractMissing variant; verify the user-visible prose
    // mentions both the protocol and the missing contract.
    assert!(err.contains("intent_router"));
    assert!(err.contains("router"));
    assert!(err.contains("registry"));
}
