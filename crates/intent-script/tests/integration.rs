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
    // into an Eip712Intent since a router is configured.
    match &output {
        CompileOutput::Eip712Intent(intent) => {
            // Direct tx target should be the IntentRouter
            assert_eq!(
                format!("{}", intent.direct_tx.to),
                "0x1111111254EEB25477B68fb85Ed929f73A960582"
            );
            // No direct ETH value (approve + supply are both 0-value)
            assert_eq!(intent.direct_tx.value.to_string(), "0");
            // Calldata should start with executeDirect() selector
            assert!(
                intent.direct_tx.data.len() > 4,
                "Should have calldata for router.executeDirect()"
            );
            // Description should mention batching
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            // Should have EIP-712 domain
            assert_eq!(intent.domain.name, "IntentRouter");
            assert_eq!(intent.domain.chain_id, 1);
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }

    // Verify JSON serialization
    let json_output = CompileOutputJson::from(&output);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Aave deposit (batched) output:\n{json_str}");

    assert!(json_str.contains("eip712_intent"));
    assert!(json_str.contains("IntentRouter"));
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
        CompileOutput::Eip712Intent(intent) => {
            // Target should be the IntentRouter (batched)
            assert_eq!(
                format!("{}", intent.direct_tx.to),
                "0x1111111254EEB25477B68fb85Ed929f73A960582"
            );
            assert!(
                intent.direct_tx.data.len() > 4,
                "Should have calldata for router.executeDirect()"
            );
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Uniswap V3"),
                "Description should mention Uniswap V3: {}",
                intent.description
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }

    // Verify JSON serialization
    let json_output = CompileOutputJson::from(&output);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Swap USDC→WETH output:\n{json_str}");
    assert!(json_str.contains("eip712_intent"));
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
        CompileOutput::Eip712Intent(intent) => {
            // Target should be the IntentRouter
            assert_eq!(
                format!("{}", intent.direct_tx.to),
                "0x1111111254EEB25477B68fb85Ed929f73A960582"
            );
            assert!(
                intent.direct_tx.data.len() > 4,
                "Should have calldata for router.executeDirect()"
            );
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            // Should contain both supply and borrow descriptions
            assert!(
                intent.description.contains("Supply"),
                "Description should mention Supply: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Borrow"),
                "Description should mention Borrow: {}",
                intent.description
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }

    let json_output = CompileOutputJson::from(&output);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Deposit+Borrow output:\n{json_str}");
    assert!(json_str.contains("eip712_intent"));
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
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(
                format!("{}", intent.direct_tx.to),
                "0x1111111254EEB25477B68fb85Ed929f73A960582"
            );
            assert!(
                intent.direct_tx.data.len() > 4,
                "Should have calldata for router.executeDirect()"
            );
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            // Should contain swap, supply, and borrow
            assert!(
                intent.description.contains("Swap"),
                "Description should mention Swap: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Supply"),
                "Description should mention Supply: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Borrow"),
                "Description should mention Borrow: {}",
                intent.description
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }

    let json_output = CompileOutputJson::from(&output);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Swap+Deposit+Borrow output:\n{json_str}");
    assert!(json_str.contains("eip712_intent"));
}

#[test]
fn test_swap_with_custom_fee_tier() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "fee": "500" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    match &output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(
                intent.direct_tx.data.len() > 4,
                "Should have calldata for router.executeDirect()"
            );
            assert!(
                intent.description.contains("Uniswap V3"),
                "Description should mention Uniswap V3: {}",
                intent.description
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }
}

#[test]
fn test_swap_without_fee_defaults_to_3000() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    match &output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(intent.direct_tx.data.len() > 4);
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

#[test]
fn test_swap_via_1inch_with_calldata() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "via": "1inch", "calldata": "0xdeadbeef" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    match &output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(intent.direct_tx.data.len() > 4);
            assert!(
                intent.description.contains("1inch"),
                "Description should mention 1inch: {}",
                intent.description
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }
}

#[test]
fn test_swap_via_1inch_missing_calldata_fails() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "via": "1inch" } }
        ]
    }"#;

    let result = compile(input, &config_dir());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("calldata"),
        "Error should mention missing calldata: {err}"
    );
}

#[test]
fn test_swap_via_unsupported_fails() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "via": "paraswap" } }
        ]
    }"#;

    let result = compile(input, &config_dir());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("paraswap"),
        "Error should mention unsupported provider: {err}"
    );
}

#[test]
fn test_wrap_steth_to_wsteth() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "stETH", "amount": "10.0" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    // Wrap stETH produces 2 calls (approve + wrap) batched via router
    match &output {
        CompileOutput::Eip712Intent(intent) => {
            // Target should be the IntentRouter (batched)
            assert_eq!(
                format!("{}", intent.direct_tx.to),
                "0x1111111254EEB25477B68fb85Ed929f73A960582"
            );
            assert!(
                intent.direct_tx.data.len() > 4,
                "Should have calldata for router.executeDirect()"
            );
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            assert!(
                intent.description.contains("wstETH"),
                "Description should mention wstETH: {}",
                intent.description
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }
}

#[test]
fn test_stake_and_wrap_steth() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "stake": { "asset": "ETH", "amount": "10.0", "into": "lido" } },
            { "wrap": { "asset": "stETH", "amount": "10.0" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    // Stake + wrap produces batched calldata
    match &output {
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(
                format!("{}", intent.direct_tx.to),
                "0x1111111254EEB25477B68fb85Ed929f73A960582"
            );
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Stake"),
                "Description should mention Stake: {}",
                intent.description
            );
            assert!(
                intent.description.contains("wstETH"),
                "Description should mention wstETH: {}",
                intent.description
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }
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

#[test]
fn test_eip712_nonce_and_deadline() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "nonce": 5,
        "deadline": 1712345678,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    match &output {
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(intent.intent_batch.nonce, 5);
            assert_eq!(intent.intent_batch.deadline, 1712345678);
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

// ─── Additional coverage tests ─────────────────────────────────────────

#[test]
fn test_aave_withdraw() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "withdraw": { "asset": "USDC", "amount": "5000", "from": "aave" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");

    // Withdraw is a single call (no approval needed — user already has aTokens)
    match &output {
        CompileOutput::SingleTx(tx) => {
            // Target should be Aave V3 Pool
            assert_eq!(
                format!("{}", tx.to),
                "0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2"
            );
            // No ETH value
            assert_eq!(tx.value.to_string(), "0");
            // withdraw(address,uint256,address) selector: 0x69328dec
            assert_eq!(&tx.data[..4], &[0x69, 0x32, 0x8d, 0xec]);
        }
        other => panic!("Expected SingleTx for withdraw, got {:?}", other),
    }

    let json_output = CompileOutputJson::from(&output);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Aave withdraw output:\n{json_str}");
    assert!(json_str.contains("single_tx"));
}

#[test]
fn test_complex_defi_from_example_file() {
    // Read the actual example file — this is the canonical complex DeFi intent
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path = std::path::Path::new(manifest_dir).join("examples/complex_defi.json");
    let input =
        std::fs::read_to_string(&example_path).expect("should read complex_defi.json example file");

    let output = compile(&input, &config_dir()).expect("compile should succeed");

    // complex_defi.json: swap USDC→WETH + deposit WETH into Aave + borrow DAI
    // This produces multiple calls batched via router
    match &output {
        CompileOutput::Eip712Intent(intent) => {
            // Should be batched via router
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            // Should contain all three operations
            assert!(
                intent.description.contains("Swap") || intent.description.contains("swap"),
                "Description should mention Swap: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Supply") || intent.description.contains("supply"),
                "Description should mention Supply: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Borrow") || intent.description.contains("borrow"),
                "Description should mention Borrow: {}",
                intent.description
            );
            // Should have multiple calls in the batch
            assert!(
                intent.intent_batch.calls.len() >= 3,
                "Complex DeFi should produce at least 3 calls (approve+swap+approve+supply+borrow), got {}",
                intent.intent_batch.calls.len()
            );
            // EIP-712 domain should be set
            assert_eq!(intent.domain.name, "IntentRouter");
            assert_eq!(intent.domain.chain_id, 1);
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }

    // Verify JSON serialization round-trips
    let json_output = CompileOutputJson::from(&output);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Complex DeFi output:\n{json_str}");
    assert!(json_str.contains("eip712_intent"));
    assert!(json_str.contains("IntentRouter"));
}

#[test]
fn test_stake_lido_wsteth_from_example_file() {
    // Read the actual example file
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path = std::path::Path::new(manifest_dir).join("examples/stake_lido_wsteth.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read stake_lido_wsteth.json example file");

    let output = compile(&input, &config_dir()).expect("compile should succeed");

    // stake ETH in Lido + wrap stETH → wstETH produces batched calls
    match &output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Stake"),
                "Description should mention Stake: {}",
                intent.description
            );
            assert!(
                intent.description.contains("wstETH"),
                "Description should mention wstETH: {}",
                intent.description
            );
            // Should have calls for: stake + approve stETH + wrap to wstETH
            assert!(
                intent.intent_batch.calls.len() >= 2,
                "Stake+wrap should produce at least 2 calls, got {}",
                intent.intent_batch.calls.len()
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }

    let json_output = CompileOutputJson::from(&output);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Stake Lido wstETH output:\n{json_str}");
    assert!(json_str.contains("eip712_intent"));
}

#[test]
fn test_all_example_files_compile() {
    // Ensure every example JSON file in the examples directory compiles successfully
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let examples_dir = std::path::Path::new(manifest_dir).join("examples");

    let entries = std::fs::read_dir(&examples_dir).expect("should read examples directory");
    let mut count = 0;

    for entry in entries {
        let entry = entry.expect("should read dir entry");
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "json") {
            let input = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("should read {}", path.display()));
            let result = compile(&input, &config_dir());
            assert!(
                result.is_ok(),
                "Example {} should compile successfully, but got error: {:?}",
                path.file_name().unwrap().to_string_lossy(),
                result.unwrap_err()
            );
            count += 1;
            println!(
                "✅ {} compiled successfully",
                path.file_name().unwrap().to_string_lossy()
            );
        }
    }

    assert!(count > 0, "Should have found at least one example file");
    println!("All {count} example files compiled successfully");
}
