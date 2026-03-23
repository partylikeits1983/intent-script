//! Generate calldata fixture files for Foundry tests.
//!
//! These tests compile intent JSON and write the resulting calldata
//! to files that Foundry tests can read and execute against the router.
//!
//! Run with:
//!   cargo test -p intent-script --test generate_calldata -- --nocapture

use std::path::{Path, PathBuf};

use intent_script::{CompileOutput, compile};

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
fn write_calldata(name: &str, output: &CompileOutput) {
    let dir = fixtures_dir();
    std::fs::create_dir_all(&dir).expect("create fixtures dir");

    match output {
        CompileOutput::SingleTx(tx) => {
            let hex = format!("0x{}", hex_encode(&tx.data));
            let path = dir.join(format!("{name}.txt"));
            std::fs::write(&path, &hex).expect("write calldata file");
            println!(
                "Wrote calldata to {}: {} bytes",
                path.display(),
                tx.data.len()
            );
            println!("  to: {}", tx.to);
            println!("  value: {}", tx.value);
            println!("  calldata: {hex}");
        }
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
        }
        CompileOutput::RequiresExecutor { reason } => {
            panic!("Cannot generate calldata: {reason}");
        }
    }
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
