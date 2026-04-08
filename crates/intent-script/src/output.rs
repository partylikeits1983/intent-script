//! Compile output types — the final result of the compiler pipeline.

use alloy_primitives::{Address, Bytes, U256};
use serde::Serialize;

/// The result of compiling an intent script.
#[derive(Debug, Clone)]
pub enum CompileOutput {
    /// A single unsigned transaction (for single-call intents)
    SingleTx(UnsignedTx),
    /// EIP-712 typed data for signing + direct tx for self-execution
    Eip712Intent(Eip712IntentOutput),
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

/// EIP-712 intent output — contains both typed data for signing and a direct tx.
#[derive(Debug, Clone)]
pub struct Eip712IntentOutput {
    /// The EIP-712 domain
    pub domain: Eip712Domain,
    /// The IntentBatch struct data
    pub intent_batch: IntentBatchData,
    /// Pre-computed EIP-712 typed data hash
    pub typed_data_hash: [u8; 32],
    /// Human-readable description of the batch
    pub description: String,
    /// The unsigned tx for self-execution (calls executeDirect)
    pub direct_tx: UnsignedTx,
}

/// EIP-712 domain parameters.
#[derive(Debug, Clone)]
pub struct Eip712Domain {
    pub name: String,
    pub version: String,
    pub chain_id: u64,
    pub verifying_contract: Address,
}

/// IntentBatch data for EIP-712 signing.
#[derive(Debug, Clone)]
pub struct IntentBatchData {
    pub signer: Address,
    pub calls: Vec<CallData>,
    pub tokens_to_sweep: Vec<Address>,
    pub nonce: u64,
    pub deadline: u64,
}

/// A single call in the intent batch.
#[derive(Debug, Clone)]
pub struct CallData {
    pub target: Address,
    pub call_data: Bytes,
    pub value: U256,
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
    pub eip712: Option<Eip712Json>,
    #[serde(rename = "directTx", skip_serializing_if = "Option::is_none")]
    pub direct_tx: Option<UnsignedTxJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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

#[derive(Debug, Serialize)]
pub struct Eip712Json {
    pub domain: Eip712DomainJson,
    #[serde(rename = "primaryType")]
    pub primary_type: String,
    pub types: Eip712TypesJson,
    pub message: Eip712MessageJson,
}

#[derive(Debug, Serialize)]
pub struct Eip712DomainJson {
    pub name: String,
    pub version: String,
    #[serde(rename = "chainId")]
    pub chain_id: u64,
    #[serde(rename = "verifyingContract")]
    pub verifying_contract: String,
}

#[derive(Debug, Serialize)]
pub struct Eip712TypesJson {
    #[serde(rename = "EIP712Domain")]
    pub eip712_domain: Vec<Eip712TypeField>,
    #[serde(rename = "Call")]
    pub call: Vec<Eip712TypeField>,
    #[serde(rename = "IntentBatch")]
    pub intent_batch: Vec<Eip712TypeField>,
}

#[derive(Debug, Serialize)]
pub struct Eip712TypeField {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
}

#[derive(Debug, Serialize)]
pub struct Eip712MessageJson {
    pub signer: String,
    pub calls: Vec<Eip712CallJson>,
    #[serde(rename = "tokensToSweep")]
    pub tokens_to_sweep: Vec<String>,
    pub nonce: String,
    pub deadline: String,
}

#[derive(Debug, Serialize)]
pub struct Eip712CallJson {
    pub target: String,
    #[serde(rename = "callData")]
    pub call_data: String,
    pub value: String,
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

impl From<&Eip712IntentOutput> for Eip712Json {
    fn from(output: &Eip712IntentOutput) -> Self {
        Eip712Json {
            domain: Eip712DomainJson {
                name: output.domain.name.clone(),
                version: output.domain.version.clone(),
                chain_id: output.domain.chain_id,
                verifying_contract: format!("{}", output.domain.verifying_contract),
            },
            primary_type: "IntentBatch".to_string(),
            types: Eip712TypesJson {
                eip712_domain: vec![
                    Eip712TypeField {
                        name: "name".to_string(),
                        type_name: "string".to_string(),
                    },
                    Eip712TypeField {
                        name: "version".to_string(),
                        type_name: "string".to_string(),
                    },
                    Eip712TypeField {
                        name: "chainId".to_string(),
                        type_name: "uint256".to_string(),
                    },
                    Eip712TypeField {
                        name: "verifyingContract".to_string(),
                        type_name: "address".to_string(),
                    },
                ],
                call: vec![
                    Eip712TypeField {
                        name: "target".to_string(),
                        type_name: "address".to_string(),
                    },
                    Eip712TypeField {
                        name: "callData".to_string(),
                        type_name: "bytes".to_string(),
                    },
                    Eip712TypeField {
                        name: "value".to_string(),
                        type_name: "uint256".to_string(),
                    },
                ],
                intent_batch: vec![
                    Eip712TypeField {
                        name: "signer".to_string(),
                        type_name: "address".to_string(),
                    },
                    Eip712TypeField {
                        name: "calls".to_string(),
                        type_name: "Call[]".to_string(),
                    },
                    Eip712TypeField {
                        name: "tokensToSweep".to_string(),
                        type_name: "address[]".to_string(),
                    },
                    Eip712TypeField {
                        name: "nonce".to_string(),
                        type_name: "uint256".to_string(),
                    },
                    Eip712TypeField {
                        name: "deadline".to_string(),
                        type_name: "uint256".to_string(),
                    },
                ],
            },
            message: Eip712MessageJson {
                signer: format!("{}", output.intent_batch.signer),
                calls: output
                    .intent_batch
                    .calls
                    .iter()
                    .map(|c| Eip712CallJson {
                        target: format!("{}", c.target),
                        call_data: format!("0x{}", hex::encode(&c.call_data)),
                        value: c.value.to_string(),
                    })
                    .collect(),
                tokens_to_sweep: output
                    .intent_batch
                    .tokens_to_sweep
                    .iter()
                    .map(|a| format!("{}", a))
                    .collect(),
                nonce: output.intent_batch.nonce.to_string(),
                deadline: output.intent_batch.deadline.to_string(),
            },
        }
    }
}

impl From<&CompileOutput> for CompileOutputJson {
    fn from(output: &CompileOutput) -> Self {
        match output {
            CompileOutput::SingleTx(tx) => CompileOutputJson {
                output_type: "single_tx".to_string(),
                transactions: Some(vec![UnsignedTxJson::from(tx)]),
                eip712: None,
                direct_tx: None,
                description: None,
                reason: None,
            },
            CompileOutput::Eip712Intent(intent) => CompileOutputJson {
                output_type: "eip712_intent".to_string(),
                transactions: None,
                eip712: Some(Eip712Json::from(intent)),
                direct_tx: Some(UnsignedTxJson::from(&intent.direct_tx)),
                description: Some(intent.description.clone()),
                reason: None,
            },
            CompileOutput::TxSequence(txs) => CompileOutputJson {
                output_type: "tx_sequence".to_string(),
                transactions: Some(txs.iter().map(UnsignedTxJson::from).collect()),
                eip712: None,
                direct_tx: None,
                description: None,
                reason: None,
            },
            CompileOutput::RequiresExecutor { reason } => CompileOutputJson {
                output_type: "requires_executor".to_string(),
                transactions: None,
                eip712: None,
                direct_tx: None,
                description: None,
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
