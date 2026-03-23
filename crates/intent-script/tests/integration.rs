//! End-to-end integration tests for the intent-script compiler.

use std::path::{Path, PathBuf};

use intent_script::output::CompileOutputJson;
use intent_script::{CompileOutput, compile};

/// Get the path to the config directory at the workspace root.
fn config_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR points to crates/intent-script/
    // config/ is at the workspace root (two levels up)
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent() // crates/
        .unwrap()
        .parent() // workspace root
        .unwrap()
        .join("config")
}

#[test]
fn test_wrap_eth_to_weth() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1.5" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    // Wrap is a single tx
    match &output {
        CompileOutput::SingleTx(tx) => {
            // Target should be WETH contract
            assert_eq!(
                format!("{}", tx.to),
                "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
            );
            // Value should be 1.5 ETH in wei
            assert_eq!(tx.value.to_string(), "1500000000000000000");
            // Chain ID should be 1 (ethereum mainnet)
            assert_eq!(tx.chain_id, 1);
            // Calldata should be deposit() selector: 0xd0e30db0
            assert_eq!(tx.data.len(), 4);
            assert_eq!(&tx.data[..4], &[0xd0, 0xe3, 0x0d, 0xb0]);
        }
        other => panic!("Expected SingleTx, got {:?}", other),
    }

    // Verify JSON serialization works
    let json_output = CompileOutputJson::from(&output);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Wrap ETH output:\n{json_str}");

    assert!(json_str.contains("single_tx"));
    assert!(json_str.contains("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"));
}

#[test]
fn test_aave_deposit_usdc_batched_through_router() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    // Deposit produces 2 calls (approve + supply) which get batched
    // into a single router.execute() tx since a router is configured.
    match &output {
        CompileOutput::SingleTx(tx) => {
            // Target should be the IntentRouter
            assert_eq!(
                format!("{}", tx.to),
                "0x1111111254EEB25477B68fb85Ed929f73A960582"
            );
            // No direct ETH value (approve + supply are both 0-value)
            assert_eq!(tx.value.to_string(), "0");
            // Calldata should start with execute() selector
            // execute((address,bytes,uint256)[],address[])
            // The selector is the first 4 bytes of keccak256 of the signature
            assert!(
                tx.data.len() > 4,
                "Should have calldata for router.execute()"
            );
            // Description should mention batching
            assert!(
                tx.description.contains("Batched"),
                "Description should mention batching: {}",
                tx.description
            );
        }
        other => panic!("Expected SingleTx (batched via router), got {:?}", other),
    }

    // Verify JSON serialization
    let json_output = CompileOutputJson::from(&output);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Aave deposit (batched) output:\n{json_str}");

    assert!(json_str.contains("single_tx"));
    assert!(json_str.contains("0x1111111254EEB25477B68fb85Ed929f73A960582"));
}

#[test]
fn test_unwrap_weth() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "unwrap": { "asset": "WETH", "amount": "2.0" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    match &output {
        CompileOutput::SingleTx(tx) => {
            // Target should be WETH contract
            assert_eq!(
                format!("{}", tx.to),
                "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
            );
            // No ETH value sent for unwrap
            assert_eq!(tx.value.to_string(), "0");
            // withdraw(uint256) selector: 0x2e1a7d4d
            assert_eq!(&tx.data[..4], &[0x2e, 0x1a, 0x7d, 0x4d]);
        }
        other => panic!("Expected SingleTx, got {:?}", other),
    }
}

#[test]
fn test_unknown_network_fails() {
    let input = r#"{
        "network": "solana",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1.0" } }
        ]
    }"#;

    let result = compile(input, &config_dir());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("solana"),
        "Error should mention the unknown network: {err}"
    );
}

#[test]
fn test_unknown_asset_fails() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "SHIB", "amount": "1000" } }
        ]
    }"#;

    let result = compile(input, &config_dir());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("SHIB"),
        "Error should mention the unknown asset: {err}"
    );
}

#[test]
fn test_empty_steps_fails() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": []
    }"#;

    let result = compile(input, &config_dir());
    assert!(result.is_err());
}
