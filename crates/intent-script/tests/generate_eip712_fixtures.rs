//! Generate EIP-712 fixture files for Foundry fork tests.
//!
//! These tests compile intent JSON and write the resulting EIP-712 batch data
//! to JSON files that Foundry tests can read, sign with vm.sign(), and execute
//! against the router via executeSigned().
//!
//! The fixture format is a JSON object containing:
//! - calls: array of {target, callData, value}
//! - tokensToSweep: array of addresses
//! - nonce: uint256
//! - deadline: uint256
//! - signer: address
//! - totalValue: total ETH value across all calls
//!
//! Run with:
//!   cargo test -p intent-script --test generate_eip712_fixtures -- --nocapture

use std::path::{Path, PathBuf};

use intent_script::output::CompileOutputJson;
use intent_script::{CompileOutput, CompileResult, compile};

/// Get the path to the config directory at the workspace root.
fn config_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("config")
}

fn load_config() -> (String, String, String) {
    let dir = config_dir();
    let chains = std::fs::read_to_string(dir.join("chains.json")).unwrap();
    let assets = std::fs::read_to_string(dir.join("assets/ethereum.json")).unwrap();
    let protocols = std::fs::read_to_string(dir.join("protocols/ethereum.json")).unwrap();
    (chains, assets, protocols)
}

fn do_compile(input: &str) -> Result<CompileResult, intent_script::error::CompileError> {
    let (c, a, p) = load_config();
    compile(input, &c, &a, &p)
}

/// Get the path to the Foundry fixtures directory.
fn fixtures_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("contracts")
        .join("test")
        .join("fixtures")
}

fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

/// Write EIP-712 batch data as a JSON fixture file for Foundry fork tests.
/// Also writes the full EIP-712 typed data JSON for reference.
fn write_eip712_fixture(name: &str, result: &CompileResult) {
    let dir = fixtures_dir();
    std::fs::create_dir_all(&dir).expect("create fixtures dir");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            // Write the full EIP-712 typed data JSON (for reference / wallet signing)
            let eip712_json = CompileOutputJson::from(result);
            let eip712_str =
                serde_json::to_string_pretty(&eip712_json).expect("serialize eip712 json");
            let eip712_path = dir.join(format!("{name}_eip712.json"));
            std::fs::write(&eip712_path, &eip712_str).expect("write eip712 json");
            println!("Wrote EIP-712 JSON to {}", eip712_path.display());

            // Write a simplified batch JSON that Foundry can easily parse
            // Format: { calls: [{target, callData, value}], tokensToSweep: [...], ... }
            let calls: Vec<serde_json::Value> = intent
                .intent_batch
                .calls
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "target": format!("{}", c.target),
                        "callData": format!("0x{}", hex_encode(&c.call_data)),
                        "value": c.value.to_string()
                    })
                })
                .collect();

            let tokens: Vec<String> = intent
                .intent_batch
                .tokens_to_sweep
                .iter()
                .map(|a| format!("{}", a))
                .collect();

            let batch_json = serde_json::json!({
                "signer": format!("{}", intent.intent_batch.signer),
                "calls": calls,
                "tokensToSweep": tokens,
                "nonce": intent.intent_batch.nonce,
                "deadline": intent.intent_batch.deadline,
                "totalValue": intent.direct_tx.value.to_string(),
                "routerAddress": format!("{}", intent.direct_tx.to),
                "chainId": intent.domain.chain_id,
                "description": &intent.description
            });

            let batch_str =
                serde_json::to_string_pretty(&batch_json).expect("serialize batch json");
            let batch_path = dir.join(format!("{name}_batch.json"));
            std::fs::write(&batch_path, &batch_str).expect("write batch json");
            println!("Wrote batch JSON to {}", batch_path.display());
            println!("  signer: {}", intent.intent_batch.signer);
            println!("  calls: {}", intent.intent_batch.calls.len());
            println!(
                "  tokensToSweep: {}",
                intent.intent_batch.tokens_to_sweep.len()
            );
            println!("  totalValue: {}", intent.direct_tx.value);
        }
        CompileOutput::SingleTx(_) => {
            println!("Skipping {name}: SingleTx (no EIP-712 batch)");
        }
        CompileOutput::TxSequence(_) => {
            println!("Skipping {name}: TxSequence (no EIP-712 batch)");
        }
        CompileOutput::RequiresExecutor { reason } => {
            panic!("Cannot generate fixture for {name}: {reason}");
        }
    }
}

#[test]
fn generate_complex_defi_eip712() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path = std::path::Path::new(manifest_dir).join("examples/complex_defi.json");
    let input =
        std::fs::read_to_string(&example_path).expect("should read complex_defi.json example file");

    let output = do_compile(&input).expect("compile should succeed");
    write_eip712_fixture("complex_defi", &output);
}

#[test]
fn generate_aave_deposit_eip712() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let output = do_compile(input).expect("compile should succeed");
    write_eip712_fixture("aave_deposit_usdc", &output);
}

#[test]
fn generate_swap_usdc_weth_eip712() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "min_amount_out": "0.1" } }
        ]
    }"#;

    let output = do_compile(input).expect("compile should succeed");
    write_eip712_fixture("swap_usdc_weth", &output);
}

#[test]
fn generate_deposit_borrow_eip712() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } },
            { "borrow": { "asset": "DAI", "amount": "2000", "from": "aave" } }
        ]
    }"#;

    let output = do_compile(input).expect("compile should succeed");
    write_eip712_fixture("deposit_borrow", &output);
}

#[test]
fn generate_stake_lido_wsteth_eip712() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path = std::path::Path::new(manifest_dir).join("examples/stake_lido_wsteth.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read stake_lido_wsteth.json example file");

    let output = do_compile(&input).expect("compile should succeed");
    write_eip712_fixture("stake_lido_wsteth", &output);
}

#[test]
fn generate_stake_eth_lido_eip712() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "stake": { "asset": "ETH", "amount": "10.0", "into": "lido" } }
        ]
    }"#;

    let output = do_compile(input).expect("compile should succeed");
    write_eip712_fixture("stake_eth_lido", &output);
}
