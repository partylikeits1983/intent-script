//! Test helpers for submitting compiled transactions to Anvil.

use std::path::{Path, PathBuf};

use alloy::network::TransactionBuilder;
use alloy::rpc::types::TransactionRequest;

use intent_script::output::{CompileOutput, CompileResult, UnsignedTx};

/// Get the path to the config directory at the workspace root.
pub fn workspace_config_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR points to crates/evm-testing/
    // config/ is at the workspace root (two levels up)
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent() // crates/
        .unwrap()
        .parent() // workspace root
        .unwrap()
        .join("config")
}

/// Convert an intent-script UnsignedTx to an Alloy TransactionRequest.
pub fn to_alloy_tx(tx: &UnsignedTx) -> TransactionRequest {
    let mut req = TransactionRequest::default();
    req.set_from(tx.from);
    req.set_to(tx.to);
    req.set_value(tx.value);
    req.set_input(tx.data.clone());
    req.set_chain_id(tx.chain_id);
    req
}

/// Load config files from the workspace config directory.
pub fn load_config() -> (String, String, String) {
    let dir = workspace_config_dir();
    let chains = std::fs::read_to_string(dir.join("chains.json")).unwrap();
    let assets = std::fs::read_to_string(dir.join("assets/ethereum.json")).unwrap();
    let protocols = std::fs::read_to_string(dir.join("protocols/ethereum.json")).unwrap();
    (chains, assets, protocols)
}

/// Compile an intent JSON and return the result (output + warnings).
pub fn compile_intent(json: &str) -> intent_script::error::Result<CompileResult> {
    let (c, a, p) = load_config();
    intent_script::compile(json, &c, &a, &p)
}

/// Extract all unsigned transactions from a CompileOutput.
pub fn extract_txs(output: &CompileOutput) -> Vec<&UnsignedTx> {
    match output {
        CompileOutput::SingleTx(tx) => vec![tx],
        CompileOutput::Eip712Intent(intent) => vec![&intent.direct_tx],
        CompileOutput::TxSequence(txs) => txs.iter().collect(),
        CompileOutput::RequiresExecutor { .. } => vec![],
    }
}
