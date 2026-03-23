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

// ─── New primitive tests ───────────────────────────────────────────────

#[test]
fn test_swap_usdc_to_weth() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    // Swap produces 2 calls (approve + exactInputSingle) batched via router
    match &output {
        CompileOutput::SingleTx(tx) => {
            // Target should be the IntentRouter (batched)
            assert_eq!(
                format!("{}", tx.to),
                "0x1111111254EEB25477B68fb85Ed929f73A960582"
            );
            assert!(
                tx.data.len() > 4,
                "Should have calldata for router.execute()"
            );
            assert!(
                tx.description.contains("Batched"),
                "Description should mention batching: {}",
                tx.description
            );
            assert!(
                tx.description.contains("Uniswap V3"),
                "Description should mention Uniswap V3: {}",
                tx.description
            );
        }
        other => panic!("Expected SingleTx (batched via router), got {:?}", other),
    }

    // Verify JSON serialization
    let json_output = CompileOutputJson::from(&output);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Swap USDC→WETH output:\n{json_str}");
    assert!(json_str.contains("single_tx"));
}

#[test]
fn test_deposit_and_borrow_single_tx() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } },
            { "borrow": { "asset": "DAI", "amount": "2000", "from": "aave" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    // Deposit + borrow produces 3 calls (approve + supply + borrow) batched via router
    match &output {
        CompileOutput::SingleTx(tx) => {
            // Target should be the IntentRouter
            assert_eq!(
                format!("{}", tx.to),
                "0x1111111254EEB25477B68fb85Ed929f73A960582"
            );
            assert!(
                tx.data.len() > 4,
                "Should have calldata for router.execute()"
            );
            assert!(
                tx.description.contains("Batched"),
                "Description should mention batching: {}",
                tx.description
            );
            // Should contain both supply and borrow descriptions
            assert!(
                tx.description.contains("Supply"),
                "Description should mention Supply: {}",
                tx.description
            );
            assert!(
                tx.description.contains("Borrow"),
                "Description should mention Borrow: {}",
                tx.description
            );
        }
        other => panic!("Expected SingleTx (batched via router), got {:?}", other),
    }

    let json_output = CompileOutputJson::from(&output);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Deposit+Borrow output:\n{json_str}");
    assert!(json_str.contains("single_tx"));
}

#[test]
fn test_swap_deposit_borrow_chain() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "5000", "to": "WETH" } },
            { "deposit": { "asset": "WETH", "amount": "2.0", "into": "aave" } },
            { "borrow": { "asset": "DAI", "amount": "1000", "from": "aave" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    // Should produce batched tx: approve USDC, swap, approve WETH, supply, borrow
    match &output {
        CompileOutput::SingleTx(tx) => {
            assert_eq!(
                format!("{}", tx.to),
                "0x1111111254EEB25477B68fb85Ed929f73A960582"
            );
            assert!(
                tx.data.len() > 4,
                "Should have calldata for router.execute()"
            );
            assert!(
                tx.description.contains("Batched"),
                "Description should mention batching: {}",
                tx.description
            );
            // Should contain swap, supply, and borrow
            assert!(
                tx.description.contains("Swap"),
                "Description should mention Swap: {}",
                tx.description
            );
            assert!(
                tx.description.contains("Supply"),
                "Description should mention Supply: {}",
                tx.description
            );
            assert!(
                tx.description.contains("Borrow"),
                "Description should mention Borrow: {}",
                tx.description
            );
        }
        other => panic!("Expected SingleTx (batched via router), got {:?}", other),
    }

    let json_output = CompileOutputJson::from(&output);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Swap+Deposit+Borrow output:\n{json_str}");
    assert!(json_str.contains("single_tx"));
}

#[test]
fn test_stake_eth_in_lido() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "stake": { "asset": "ETH", "amount": "10.0", "into": "lido" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    // Stake is a single call (no approval needed for ETH)
    match &output {
        CompileOutput::SingleTx(tx) => {
            // Target should be Lido stETH contract
            assert_eq!(
                format!("{}", tx.to),
                "0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84"
            );
            // Value should be 10 ETH in wei
            assert_eq!(tx.value.to_string(), "10000000000000000000");
            // Calldata should be submit(address) selector
            assert!(tx.data.len() >= 4, "Should have calldata for submit()");
            // submit(address) selector: 0xa1903eab
            assert_eq!(&tx.data[..4], &[0xa1, 0x90, 0x3e, 0xab]);
        }
        other => panic!("Expected SingleTx, got {:?}", other),
    }

    let json_output = CompileOutputJson::from(&output);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Stake ETH in Lido output:\n{json_str}");
    assert!(json_str.contains("single_tx"));
    assert!(json_str.contains("0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84"));
}
