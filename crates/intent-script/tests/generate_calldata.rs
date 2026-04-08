//! Generate calldata fixture files for Foundry tests.
//!
//! These tests compile intent JSON and write the resulting calldata
//! to files that Foundry tests can read and execute against the router.
//!
//! Run with:
//!   cargo test -p intent-script --test generate_calldata -- --nocapture

use std::path::{Path, PathBuf};

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

/// Write calldata to a fixture file as a hex string (with 0x prefix).
/// Also writes a companion {name}_value.txt with the ETH value in wei,
/// and {name}_to.txt with the target address.
fn write_calldata(name: &str, result: &CompileResult) {
    let dir = fixtures_dir();
    std::fs::create_dir_all(&dir).expect("create fixtures dir");

    // Extract the transaction to write — for Eip712Intent, use the direct_tx
    let tx = match &result.output {
        CompileOutput::SingleTx(tx) => tx,
        CompileOutput::Eip712Intent(intent) => &intent.direct_tx,
        CompileOutput::TxSequence(txs) => {
            for (i, tx) in txs.iter().enumerate() {
                let hex = format!("0x{}", hex_encode(&tx.data));
                let path = dir.join(format!("{name}_{i}.txt"));
                std::fs::write(&path, &hex).expect("write calldata file");
                println!(
                    "Wrote calldata to {}: {} bytes",
                    path.display(),
                    tx.data.len()
                );
            }
            return;
        }
        CompileOutput::RequiresExecutor { reason } => {
            panic!("Cannot generate calldata: {reason}");
        }
    };

    let hex = format!("0x{}", hex_encode(&tx.data));
    let path = dir.join(format!("{name}.txt"));
    std::fs::write(&path, &hex).expect("write calldata file");

    // Write value file
    let value_path = dir.join(format!("{name}_value.txt"));
    std::fs::write(&value_path, tx.value.to_string()).expect("write value file");

    // Write target address file
    let to_path = dir.join(format!("{name}_to.txt"));
    std::fs::write(&to_path, format!("{}", tx.to)).expect("write to file");

    println!(
        "Wrote calldata to {}: {} bytes",
        path.display(),
        tx.data.len()
    );
    println!("  to: {}", tx.to);
    println!("  value: {}", tx.value);
    println!("  calldata: {hex}");
}

fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn generate_wrap_eth_calldata() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1.0" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");
    write_calldata("wrap_eth", &output);

    println!("Generated calldata for wrap ETH: {:?}", output);
}

#[test]
fn generate_aave_deposit_calldata() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");
    write_calldata("aave_deposit_usdc", &output);
}

#[test]
fn generate_swap_usdc_weth_calldata() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");
    write_calldata("swap_usdc_weth", &output);
}

#[test]
fn generate_deposit_borrow_calldata() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } },
            { "borrow": { "asset": "DAI", "amount": "2000", "from": "aave" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");
    write_calldata("deposit_borrow", &output);
}

#[test]
fn generate_swap_deposit_borrow_calldata() {
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
    write_calldata("swap_deposit_borrow", &output);
}

#[test]
fn generate_stake_eth_lido_calldata() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "stake": { "asset": "ETH", "amount": "10.0", "into": "lido" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");
    write_calldata("stake_eth_lido", &output);
}

#[test]
fn generate_aave_withdraw_calldata() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "withdraw": { "asset": "USDC", "amount": "5000", "from": "aave" } }
        ]
    }"#;

    let output = compile(input, &config_dir()).expect("compile should succeed");
    write_calldata("aave_withdraw_usdc", &output);
}

#[test]
fn generate_complex_defi_calldata() {
    // Read from the actual example file — the canonical complex DeFi intent
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path = std::path::Path::new(manifest_dir).join("examples/complex_defi.json");
    let input =
        std::fs::read_to_string(&example_path).expect("should read complex_defi.json example file");

    let output = compile(&input, &config_dir()).expect("compile should succeed");
    write_calldata("complex_defi", &output);
}

#[test]
fn generate_stake_lido_wsteth_calldata() {
    // Read from the actual example file
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path = std::path::Path::new(manifest_dir).join("examples/stake_lido_wsteth.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read stake_lido_wsteth.json example file");

    let output = compile(&input, &config_dir()).expect("compile should succeed");
    write_calldata("stake_lido_wsteth", &output);
}
