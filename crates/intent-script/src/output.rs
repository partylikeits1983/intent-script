//! Compile output types — the final result of the compiler pipeline.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use alloy_primitives::{Address, Bytes, U256};
use serde::Serialize;

/// The full result of compilation: output + any warnings.
#[derive(Debug, Clone)]
pub struct CompileResult {
    /// The compiled output (transaction data)
    pub output: CompileOutput,
    /// Non-fatal warnings from validation (e.g., "borrow without prior deposit")
    pub warnings: Vec<String>,
    /// Structured preview of inputs/outputs/steps for user-facing display.
    pub preview: Option<Preview>,
}

/// Net user-facing preview: what the signer is about to send, receive, and do.
#[derive(Debug, Clone, Default)]
pub struct Preview {
    pub inputs: Vec<PreviewToken>,
    pub outputs: Vec<PreviewToken>,
    pub steps: Vec<PreviewStepInfo>,
}

#[derive(Debug, Clone)]
pub struct PreviewToken {
    pub symbol: String,
    pub amount: String,
    pub amount_raw: String,
    pub address: String,
}

#[derive(Debug, Clone)]
pub struct PreviewStepInfo {
    pub action: String,
    pub protocol: String,
    pub description: String,
}

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
    /// Non-fatal warnings from compilation (omitted when empty)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Structured user-facing preview (inputs/outputs/steps).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<PreviewJson>,
    /// Optional simulation results. The compiler always emits `None`; the frontend
    /// fills this in after running `eth_call` against the network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub simulation: Option<SimulationJson>,
}

#[derive(Debug, Serialize)]
pub struct PreviewJson {
    pub inputs: Vec<TokenAmountJson>,
    pub outputs: Vec<TokenAmountJson>,
    pub steps: Vec<PreviewStepJson>,
}

#[derive(Debug, Serialize)]
pub struct TokenAmountJson {
    pub token: String,
    pub amount: String,
    #[serde(rename = "amountRaw")]
    pub amount_raw: String,
    pub address: String,
}

#[derive(Debug, Serialize)]
pub struct PreviewStepJson {
    pub action: String,
    pub protocol: String,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct SimulationJson {
    pub success: bool,
    #[serde(rename = "gasEstimate", skip_serializing_if = "Option::is_none")]
    pub gas_estimate: Option<String>,
    #[serde(rename = "revertReason", skip_serializing_if = "Option::is_none")]
    pub revert_reason: Option<String>,
    #[serde(rename = "balanceChanges", skip_serializing_if = "Vec::is_empty")]
    pub balance_changes: Vec<BalanceChangeJson>,
}

#[derive(Debug, Serialize)]
pub struct BalanceChangeJson {
    pub token: String,
    pub before: String,
    pub after: String,
    pub change: String,
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
            value: format!("{}", tx.value),
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
            primary_type: "IntentBatch".into(),
            types: Eip712TypesJson {
                eip712_domain: vec![
                    Eip712TypeField {
                        name: "name".into(),
                        type_name: "string".into(),
                    },
                    Eip712TypeField {
                        name: "version".into(),
                        type_name: "string".into(),
                    },
                    Eip712TypeField {
                        name: "chainId".into(),
                        type_name: "uint256".into(),
                    },
                    Eip712TypeField {
                        name: "verifyingContract".into(),
                        type_name: "address".into(),
                    },
                ],
                call: vec![
                    Eip712TypeField {
                        name: "target".into(),
                        type_name: "address".into(),
                    },
                    Eip712TypeField {
                        name: "callData".into(),
                        type_name: "bytes".into(),
                    },
                    Eip712TypeField {
                        name: "value".into(),
                        type_name: "uint256".into(),
                    },
                ],
                intent_batch: vec![
                    Eip712TypeField {
                        name: "signer".into(),
                        type_name: "address".into(),
                    },
                    Eip712TypeField {
                        name: "calls".into(),
                        type_name: "Call[]".into(),
                    },
                    Eip712TypeField {
                        name: "tokensToSweep".into(),
                        type_name: "address[]".into(),
                    },
                    Eip712TypeField {
                        name: "nonce".into(),
                        type_name: "uint256".into(),
                    },
                    Eip712TypeField {
                        name: "deadline".into(),
                        type_name: "uint256".into(),
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
                        value: format!("{}", c.value),
                    })
                    .collect(),
                tokens_to_sweep: output
                    .intent_batch
                    .tokens_to_sweep
                    .iter()
                    .map(|a| format!("{}", a))
                    .collect(),
                nonce: format!("{}", output.intent_batch.nonce),
                deadline: format!("{}", output.intent_batch.deadline),
            },
        }
    }
}

impl From<&CompileOutput> for CompileOutputJson {
    fn from(output: &CompileOutput) -> Self {
        match output {
            CompileOutput::SingleTx(tx) => CompileOutputJson {
                output_type: "single_tx".into(),
                transactions: Some(vec![UnsignedTxJson::from(tx)]),
                eip712: None,
                direct_tx: None,
                description: None,
                reason: None,
                warnings: Vec::new(),
                preview: None,
                simulation: None,
            },
            CompileOutput::Eip712Intent(intent) => CompileOutputJson {
                output_type: "eip712_intent".into(),
                transactions: None,
                eip712: Some(Eip712Json::from(intent)),
                direct_tx: Some(UnsignedTxJson::from(&intent.direct_tx)),
                description: Some(intent.description.clone()),
                reason: None,
                warnings: Vec::new(),
                preview: None,
                simulation: None,
            },
            CompileOutput::TxSequence(txs) => CompileOutputJson {
                output_type: "tx_sequence".into(),
                transactions: Some(txs.iter().map(UnsignedTxJson::from).collect()),
                eip712: None,
                direct_tx: None,
                description: None,
                reason: None,
                warnings: Vec::new(),
                preview: None,
                simulation: None,
            },
            CompileOutput::RequiresExecutor { reason } => CompileOutputJson {
                output_type: "requires_executor".into(),
                transactions: None,
                eip712: None,
                direct_tx: None,
                description: None,
                reason: Some(reason.clone()),
                warnings: Vec::new(),
                preview: None,
                simulation: None,
            },
        }
    }
}

impl From<&CompileResult> for CompileOutputJson {
    fn from(result: &CompileResult) -> Self {
        let mut json = CompileOutputJson::from(&result.output);
        json.warnings = result.warnings.clone();
        json.preview = result.preview.as_ref().map(PreviewJson::from);
        json
    }
}

impl From<&Preview> for PreviewJson {
    fn from(p: &Preview) -> Self {
        PreviewJson {
            inputs: p.inputs.iter().map(TokenAmountJson::from).collect(),
            outputs: p.outputs.iter().map(TokenAmountJson::from).collect(),
            steps: p.steps.iter().map(PreviewStepJson::from).collect(),
        }
    }
}

impl From<&PreviewToken> for TokenAmountJson {
    fn from(t: &PreviewToken) -> Self {
        TokenAmountJson {
            token: t.symbol.clone(),
            amount: t.amount.clone(),
            amount_raw: t.amount_raw.clone(),
            address: t.address.clone(),
        }
    }
}

impl From<&PreviewStepInfo> for PreviewStepJson {
    fn from(s: &PreviewStepInfo) -> Self {
        PreviewStepJson {
            action: s.action.clone(),
            protocol: s.protocol.clone(),
            description: s.description.clone(),
        }
    }
}

impl CompileResult {
    /// Convert this compile result into its JSON-serializable form.
    ///
    /// Prefer this over constructing `CompileOutputJson` directly so callers
    /// (including the WASM binding) share a single conversion path.
    pub fn to_json(&self) -> CompileOutputJson {
        CompileOutputJson::from(self)
    }
}

/// We need hex encoding for Bytes. Using a simple inline implementation
/// since alloy_primitives::Bytes Display already includes 0x prefix.
mod hex {
    use alloc::string::String;

    pub fn encode(data: &[u8]) -> String {
        data.iter().map(|b| alloc::format!("{b:02x}")).collect()
    }
}
