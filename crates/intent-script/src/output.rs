//! Compile output types — the final result of the compiler pipeline.

use alloy_primitives::{Address, Bytes, U256};
use serde::Serialize;

/// The result of compiling an intent script.
#[derive(Debug, Clone)]
pub enum CompileOutput {
    /// A single unsigned transaction
    SingleTx(UnsignedTx),
    /// An ordered sequence of unsigned transactions
    TxSequence(Vec<UnsignedTx>),
    /// Cannot be executed as plain EOA txs — needs an executor contract
    RequiresExecutor { reason: String },
}

/// An unsigned EVM transaction ready for signing.
#[derive(Debug, Clone)]
pub struct UnsignedTx {
    pub to: Address,
    pub data: Bytes,
    pub value: U256,
    pub chain_id: u64,
    pub from: Address,
    pub description: String,
}

// --- JSON serialization ---

/// Serializable wrapper for CompileOutput
#[derive(Debug, Serialize)]
pub struct CompileOutputJson {
    #[serde(rename = "type")]
    pub output_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transactions: Option<Vec<UnsignedTxJson>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UnsignedTxJson {
    pub to: String,
    pub data: String,
    pub value: String,
    pub chain_id: u64,
    pub from: String,
    pub description: String,
}

impl From<&UnsignedTx> for UnsignedTxJson {
    fn from(tx: &UnsignedTx) -> Self {
        UnsignedTxJson {
            to: format!("{}", tx.to),
            data: format!("0x{}", hex::encode(&tx.data)),
            value: tx.value.to_string(),
            chain_id: tx.chain_id,
            from: format!("{}", tx.from),
            description: tx.description.clone(),
        }
    }
}

impl From<&CompileOutput> for CompileOutputJson {
    fn from(output: &CompileOutput) -> Self {
        match output {
            CompileOutput::SingleTx(tx) => CompileOutputJson {
                output_type: "single_tx".to_string(),
                transactions: Some(vec![UnsignedTxJson::from(tx)]),
                reason: None,
            },
            CompileOutput::TxSequence(txs) => CompileOutputJson {
                output_type: "tx_sequence".to_string(),
                transactions: Some(txs.iter().map(UnsignedTxJson::from).collect()),
                reason: None,
            },
            CompileOutput::RequiresExecutor { reason } => CompileOutputJson {
                output_type: "requires_executor".to_string(),
                transactions: None,
                reason: Some(reason.clone()),
            },
        }
    }
}

/// We need hex encoding for Bytes. Using a simple inline implementation
/// since alloy_primitives::Bytes Display already includes 0x prefix.
mod hex {
    pub fn encode(data: &[u8]) -> String {
        data.iter().map(|b| format!("{b:02x}")).collect()
    }
}
