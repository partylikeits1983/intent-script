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
fn test_aave_deposit_usdc() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    // Deposit should produce a TxSequence: approve + supply
    match &output {
        CompileOutput::TxSequence(txs) => {
            assert_eq!(txs.len(), 2, "Expected 2 txs (approve + supply)");

            // First tx: ERC-20 approve
            let approve_tx = &txs[0];
            // Target should be USDC contract
            assert_eq!(
                format!("{}", approve_tx.to),
                "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
            );
            assert_eq!(approve_tx.value.to_string(), "0");
            // approve(address,uint256) selector: 0x095ea7b3
            assert_eq!(&approve_tx.data[..4], &[0x09, 0x5e, 0xa7, 0xb3]);

            // Second tx: Aave V3 supply
            let supply_tx = &txs[1];
            // Target should be Aave V3 Pool
            assert_eq!(
                format!("{}", supply_tx.to),
                "0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2"
            );
            assert_eq!(supply_tx.value.to_string(), "0");
            // supply(address,uint256,address,uint16) selector: 0x617ba037
            assert_eq!(&supply_tx.data[..4], &[0x61, 0x7b, 0xa0, 0x37]);
        }
        other => panic!("Expected TxSequence, got {:?}", other),
    }

    // Verify JSON serialization
    let json_output = CompileOutputJson::from(&output);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Aave deposit output:\n{json_str}");

    assert!(json_str.contains("tx_sequence"));
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
