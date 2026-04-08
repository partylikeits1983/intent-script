//! Test helpers for submitting compiled transactions to Anvil.

use std::path::{Path, PathBuf};

use alloy::network::TransactionBuilder;
use alloy::rpc::types::TransactionRequest;

use intent_script::output::{CompileOutput, UnsignedTx};

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

/// Compile an intent JSON and return the output.
pub fn compile_intent(json: &str) -> intent_script::error::Result<CompileOutput> {
    intent_script::compile(json, &workspace_config_dir())
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
