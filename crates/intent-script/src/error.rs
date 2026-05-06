use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use serde::Serialize;

#[derive(Debug)]
pub enum CompileError {
    UnknownNetwork(String),
    UnknownAsset {
        asset: String,
        network: String,
        suggestion: Option<String>,
    },
    UnknownProtocol {
        protocol: String,
        network: String,
        available: Vec<String>,
    },
    InvalidAmount(String),
    InvalidAddress(String),
    Config(String),
    UnsupportedStep(String),
    Validation(String),
    InsufficientBalance {
        token: String,
        required: String,
        available: String,
    },
    SlippageTooLow {
        step_index: usize,
        current: String,
    },
    HealthFactorRisk {
        current: f64,
        threshold: f64,
    },
    InvalidChain(String),
    /// Catch-all adapter / lowering failure with a free-form message.
    /// Prefer the typed variants below when the failure mode is known.
    Adapter(String),
    /// The IR step the adapter received does not match the variant the
    /// adapter handles (e.g. lowering called the Aave-supply adapter with
    /// an Aave-borrow IR node). This is a compiler-internal bug, never
    /// the user's fault — the LLM should NOT retry the same intent.
    AdapterStepMismatch {
        adapter: &'static str,
        expected: &'static str,
    },
    /// A required protocol contract is missing from the registry config
    /// (e.g. `protocols/ethereum.json` declares `morpho` but its
    /// `contracts.pool` is absent). Compiler/config bug, not user-fixable.
    ProtocolContractMissing {
        protocol: String,
        contract: String,
    },
    /// A Morpho step did not specify a `market` and there's no default in
    /// the registry. User-fixable: add `"market": "<id>"` to the step.
    MorphoMarketRequired,
    /// A Uniswap V3 swap or LP step specified a fee tier that the
    /// compiler doesn't recognize as a canonical tier (500, 3000, 10000).
    UniswapFeeTierUnknown {
        fee: String,
    },
    Json(String),
    /// A batched intent was compiled without any source for an EIP-712
    /// deadline. The on-chain router's `executeSigned` rejects
    /// `deadline == 0`, so we refuse to emit such an intent at compile
    /// time instead of producing a tx that is guaranteed to revert.
    DeadlineMissing,
    /// An explicit `deadline` is at or before the supplied
    /// `current_timestamp`. The signed intent would be rejected by the
    /// router; fail fast at compile time.
    DeadlineInPast {
        deadline: u64,
        current_timestamp: u64,
    },
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::UnknownNetwork(s) => write!(f, "Unknown network: {s}"),
            CompileError::UnknownAsset {
                asset,
                network,
                suggestion,
            } => match suggestion {
                Some(s) => write!(
                    f,
                    "Unknown asset '{asset}' on network '{network}'. Did you mean '{s}'?"
                ),
                None => write!(f, "Unknown asset '{asset}' on network '{network}'"),
            },
            CompileError::UnknownProtocol {
                protocol,
                network,
                available,
            } => {
                if available.is_empty() {
                    write!(f, "Unknown protocol '{protocol}' on network '{network}'")
                } else {
                    write!(
                        f,
                        "Unknown protocol '{protocol}' on network '{network}'. Available: {}",
                        available.join(", ")
                    )
                }
            }
            CompileError::InvalidAmount(s) => write!(f, "Invalid amount: {s}"),
            CompileError::InvalidAddress(s) => write!(f, "Invalid address: {s}"),
            CompileError::Config(s) => write!(f, "Config error: {s}"),
            CompileError::UnsupportedStep(s) => write!(f, "Unsupported step: {s}"),
            CompileError::Validation(s) => write!(f, "Validation error: {s}"),
            CompileError::InsufficientBalance {
                token,
                required,
                available,
            } => write!(
                f,
                "Insufficient {token} balance: need {required}, have {available}"
            ),
            CompileError::SlippageTooLow {
                step_index,
                current,
            } => write!(
                f,
                "Step {step_index}: slippage protection too low (current minimum: {current})"
            ),
            CompileError::HealthFactorRisk { current, threshold } => write!(
                f,
                "Aave health factor {current:.2} is below minimum {threshold:.1}; borrow rejected to prevent liquidation"
            ),
            CompileError::InvalidChain(s) => write!(f, "Invalid intent chain: {s}"),
            CompileError::Adapter(s) => write!(f, "Adapter error: {s}"),
            CompileError::AdapterStepMismatch { adapter, expected } => write!(
                f,
                "Adapter '{adapter}' received the wrong IR step shape (expected '{expected}'). \
                 This is a compiler-internal bug, not a user error."
            ),
            CompileError::ProtocolContractMissing { protocol, contract } => write!(
                f,
                "Protocol '{protocol}' has no '{contract}' contract configured in the \
                 registry; the compiler cannot lower steps that target this protocol \
                 until the registry is fixed."
            ),
            CompileError::MorphoMarketRequired => write!(
                f,
                "Morpho steps require an explicit `market` field naming an active market. \
                 Add `\"market\": \"<id>\"` to the step."
            ),
            CompileError::UniswapFeeTierUnknown { fee } => write!(
                f,
                "Invalid Uniswap V3 fee tier '{fee}' (must be one of: 500, 3000, 10000)."
            ),
            CompileError::Json(s) => write!(f, "JSON parse error: {s}"),
            CompileError::DeadlineMissing => write!(
                f,
                "Batched intent has no deadline: neither 'deadline' nor 'current_timestamp' \
                 was provided. The router's executeSigned rejects deadline == 0; set \
                 'current_timestamp' to the current Unix timestamp to auto-compute a \
                 30-minute deadline, or pass an explicit 'deadline' > current_timestamp."
            ),
            CompileError::DeadlineInPast {
                deadline,
                current_timestamp,
            } => write!(
                f,
                "Intent 'deadline' {deadline} is at or before 'current_timestamp' \
                 {current_timestamp}; the router would reject this signed intent. \
                 Use a 'deadline' strictly greater than the current timestamp."
            ),
        }
    }
}

/// Machine-readable structured error payload. Serializes to a JSON object
/// that crosses the WASM boundary alongside the human-readable Display
/// string, so the UI and LLM can branch on a stable `code` and read
/// targeted `suggestion` / `available` / `fix_instruction` fields instead
/// of regex-parsing prose.
///
/// Field names are camelCased on the wire to match the TypeScript side.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredError {
    /// Always `"compile"` when emitted by this crate. The simulation
    /// pipeline (UI-side) uses `"simulate"` on its own equivalent type.
    pub pipeline: &'static str,
    /// Stable enum code, see the registry in `to_structured` below.
    pub code: &'static str,
    /// Human-readable message — same string the Display impl produces.
    pub message: String,
    /// Compiler pipeline stage that rejected the intent, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<&'static str>,
    /// 0-based step index inside `intent.steps`, when recoverable from
    /// the error message or structured fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_index: Option<usize>,
    /// JSON-pointer-ish path to the offending field, e.g.
    /// `steps[2].swap.min_amount_out`. Best-effort.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Extra structured fields (token, required, available, expected, got, ...).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
    /// Closest typo match / canonical template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    /// Valid alternatives for enum-like failures.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub available: Vec<String>,
    /// 1-sentence explanation of WHY the rule exists.
    pub hint: String,
    /// Imperative: "Replace 'UDSC' with 'USDC' in steps[0].swap.from."
    pub fix_instruction: String,
}

/// All stable compile-error codes emitted by `to_structured()`. The
/// system-prompt's compile-error codes reference table is sourced from
/// this list; the `code_registry_in_sync` unit test asserts every
/// `to_structured()` arm returns one of these codes (catches drift when
/// new variants are added without updating the prompt-side reference).
///
/// Keep alphabetized so diffs are easy to read.
pub const ALL_COMPILE_ERROR_CODES: &[&str] = &[
    "adapter_error",
    "adapter_step_mismatch",
    "borrow_without_collateral",
    "call_budget_exceeded",
    "config_error",
    "deadline_in_past",
    "deadline_missing",
    "empty_steps",
    "flashloan_assets_malformed",
    "flashloan_balance_drift",
    "flashloan_inner_steps_exceeded",
    "flashloan_nested",
    "flashloan_not_repayable",
    "health_factor_risk",
    "insufficient_balance",
    "insufficient_running_balance",
    "invalid_address",
    "invalid_amount",
    "invalid_chain",
    "invalid_type",
    "json_parse_error",
    "lp_invalid",
    "lp_tick_misaligned",
    "max_spend_exceeded",
    "missing_field",
    "morpho_market_required",
    "native_eth_into_aave",
    "protocol_contract_missing",
    "recipient_pinning_violation",
    "schema_version_unsupported",
    "send_to_zero",
    "signer_zero",
    "slippage_too_low",
    "swap_to_self",
    "too_many_steps",
    "uniswap_fee_tier_unknown",
    "unknown_asset",
    "unknown_field",
    "unknown_network",
    "unknown_protocol",
    "unknown_step_kind",
    "validation_generic",
    "withdraw_without_position",
    "withdrawal_claim_invalid",
    "withdrawal_request_invalid",
    "zero_amount",
];

/// Recognized user-facing step kinds. Used to produce typo suggestions
/// for `UnsupportedStep` errors. Kept in sync with `schema::public_ast::Step`.
pub const ALL_STEP_KINDS: &[&str] = &[
    "swap",
    "deposit",
    "borrow",
    "withdraw",
    "wrap",
    "unwrap",
    "stake",
    "send",
    "request_withdrawal",
    "claim_withdrawal",
    "lp_mint",
    "lp_increase",
    "lp_decrease",
    "lp_collect",
    "flashloan",
    "long",
    "short",
    "close_position",
    "bridge",
];

impl CompileError {
    /// Emit a structured, LLM-actionable view of this error. Keeps the
    /// Display impl unchanged — the structured payload rides alongside
    /// for callers that want more than prose.
    pub fn to_structured(&self) -> StructuredError {
        let message = self.to_string();
        match self {
            CompileError::UnknownNetwork(s) => StructuredError {
                pipeline: "compile",
                code: "unknown_network",
                message,
                stage: Some("registry"),
                step_index: None,
                path: Some("network".to_string()),
                fields: pairs(&[("network", s)]),
                suggestion: None,
                available: Vec::new(),
                hint: "The compiler only loads asset/protocol config for networks that are \
                     explicitly configured. Use a supported network alias."
                    .to_string(),
                fix_instruction: format!(
                    "Replace '{s}' in the intent's top-level `network` field with a \
                     supported network (e.g. 'anvil', 'ethereum', 'sepolia')."
                ),
            },
            CompileError::UnknownAsset {
                asset,
                network,
                suggestion,
            } => StructuredError {
                pipeline: "compile",
                code: "unknown_asset",
                message,
                stage: Some("normalize"),
                step_index: None,
                path: None,
                fields: pairs(&[("asset", asset), ("network", network)]),
                suggestion: suggestion.clone(),
                available: Vec::new(),
                hint: "Asset aliases are drawn from the per-network asset registry; typos are \
                       rejected to avoid silently picking the wrong token."
                    .to_string(),
                fix_instruction: match suggestion {
                    Some(s) => format!(
                        "Replace asset '{asset}' with '{s}' in the step that references it."
                    ),
                    None => format!(
                        "Replace asset '{asset}' with a supported alias on network \
                         '{network}' (see the supported-assets list in the system prompt)."
                    ),
                },
            },
            CompileError::UnknownProtocol {
                protocol,
                network,
                available,
            } => StructuredError {
                pipeline: "compile",
                code: "unknown_protocol",
                message,
                stage: Some("normalize"),
                step_index: None,
                path: None,
                fields: pairs(&[("protocol", protocol), ("network", network)]),
                suggestion: closest_match(protocol, available.iter().map(|s| s.as_str())),
                available: available.clone(),
                hint: "Protocol names are drawn from the per-network protocol registry; \
                       typos are rejected to avoid routing to the wrong adapter."
                    .to_string(),
                fix_instruction: format!(
                    "Replace protocol '{protocol}' with one of: {}.",
                    if available.is_empty() {
                        "(none configured for this network)".to_string()
                    } else {
                        available.join(", ")
                    }
                ),
            },
            CompileError::InvalidAmount(s) => StructuredError {
                pipeline: "compile",
                code: "invalid_amount",
                message,
                stage: Some("normalize"),
                step_index: None,
                path: None,
                fields: pairs(&[("amount", s)]),
                suggestion: None,
                available: Vec::new(),
                hint: "Amounts are decimal strings in natural human units; the compiler \
                       scales by the asset's decimals. Do not pre-multiply by 10^decimals, \
                       use scientific notation, or pass wei."
                    .to_string(),
                fix_instruction: format!(
                    "Replace '{s}' with a plain decimal string in human units (e.g. \
                     \"5000\" for 5,000 USDC, \"0.5\" for half a WBTC)."
                ),
            },
            CompileError::InvalidAddress(s) => StructuredError {
                pipeline: "compile",
                code: "invalid_address",
                message,
                stage: Some("normalize"),
                step_index: None,
                path: None,
                fields: pairs(&[("address", s)]),
                suggestion: None,
                available: Vec::new(),
                hint: "Addresses must be 0x-prefixed 20-byte hex strings.".to_string(),
                fix_instruction: format!(
                    "Replace '{s}' with a 0x-prefixed 40-hex-character Ethereum address."
                ),
            },
            CompileError::Config(s) => StructuredError {
                pipeline: "compile",
                code: "config_error",
                message,
                stage: Some("registry"),
                step_index: None,
                path: None,
                fields: pairs(&[("detail", s)]),
                suggestion: None,
                available: Vec::new(),
                hint: "The compiler's registry config is malformed or missing a required \
                       entry. This is a compiler/config bug, not a bad intent — regenerating \
                       the same JSON will not fix it."
                    .to_string(),
                fix_instruction: "Ask the user to report this as a compiler issue; do not \
                                  retry the same intent."
                    .to_string(),
            },
            CompileError::UnsupportedStep(s) => {
                let suggestion = closest_match(s, ALL_STEP_KINDS.iter().copied());
                let mut available: Vec<String> =
                    ALL_STEP_KINDS.iter().map(|k| (*k).to_string()).collect();
                available.sort();
                StructuredError {
                    pipeline: "compile",
                    code: "unknown_step_kind",
                    message,
                    stage: Some("normalize"),
                    step_index: None,
                    path: None,
                    fields: pairs(&[("step", s)]),
                    suggestion: suggestion.clone(),
                    available,
                    hint: "Step kinds are a closed enum; the compiler has no adapter for \
                           unrecognized kinds. Do not invent new step shapes."
                        .to_string(),
                    fix_instruction: match suggestion {
                        Some(s2) => format!(
                            "Replace the unrecognized step key '{s}' with '{s2}', or rewrite \
                             the step using one of the supported kinds."
                        ),
                        None => format!(
                            "Replace the unrecognized step key '{s}' with one of the supported \
                             step kinds: {}.",
                            ALL_STEP_KINDS.join(", ")
                        ),
                    },
                }
            }
            CompileError::Validation(s) => classify_validation_message(s, &message),
            CompileError::InsufficientBalance {
                token,
                required,
                available,
            } => StructuredError {
                pipeline: "compile",
                code: "insufficient_balance",
                message,
                stage: Some("validate"),
                step_index: None,
                path: None,
                fields: pairs(&[
                    ("token", token),
                    ("required", required),
                    ("available", available),
                ]),
                suggestion: None,
                available: Vec::new(),
                hint: "The wallet does not hold enough of this token to cover the step. \
                       Lower the step's amount or route from a token the wallet does hold."
                    .to_string(),
                fix_instruction: format!(
                    "Lower the step's amount for {token} to at most {available}, or change \
                     the input token to one the wallet holds."
                ),
            },
            CompileError::SlippageTooLow {
                step_index,
                current,
            } => StructuredError {
                pipeline: "compile",
                code: "slippage_too_low",
                message,
                stage: Some("validate"),
                step_index: Some(*step_index),
                path: Some(format!("steps[{step_index}].swap.min_amount_out")),
                fields: pairs(&[
                    ("current", current),
                    ("step_index", &step_index.to_string()),
                ]),
                suggestion: None,
                available: Vec::new(),
                hint: "Swaps must carry real slippage protection (non-zero min_amount_out). \
                       A zero min_amount_out allows arbitrary sandwiching."
                    .to_string(),
                fix_instruction: format!(
                    "Set `min_amount_out` on steps[{step_index}].swap to a positive value \
                     derived from current market price with 0.5–2% headroom, or use \
                     `price` + `slippage` instead."
                ),
            },
            CompileError::HealthFactorRisk { current, threshold } => StructuredError {
                pipeline: "compile",
                code: "health_factor_risk",
                message,
                stage: Some("validate"),
                step_index: None,
                path: None,
                fields: pairs(&[
                    ("current", &format!("{current:.4}")),
                    ("threshold", &format!("{threshold:.4}")),
                ]),
                suggestion: None,
                available: Vec::new(),
                hint: "Aave borrows are rejected below the health-factor threshold to \
                       prevent immediate liquidation."
                    .to_string(),
                fix_instruction: format!(
                    "Lower the borrow amount, or deposit more collateral first so the \
                     resulting health factor is at or above {threshold:.2}."
                ),
            },
            CompileError::InvalidChain(s) => classify_invalid_chain_message(s, &message),
            CompileError::Adapter(s) => StructuredError {
                pipeline: "compile",
                code: "adapter_error",
                message,
                stage: Some("lower"),
                step_index: None,
                path: None,
                fields: pairs(&[("detail", s)]),
                suggestion: None,
                available: Vec::new(),
                hint: "A protocol adapter failed to produce calldata. Usually means the \
                       registry entry is incomplete for the requested action."
                    .to_string(),
                fix_instruction: "Ask the user to report this as a compiler issue; the same \
                                  intent will keep failing until the adapter config is fixed."
                    .to_string(),
            },
            CompileError::AdapterStepMismatch { adapter, expected } => StructuredError {
                pipeline: "compile",
                code: "adapter_step_mismatch",
                message,
                stage: Some("lower"),
                step_index: None,
                path: None,
                fields: pairs(&[("adapter", adapter), ("expected", expected)]),
                suggestion: None,
                available: Vec::new(),
                hint: "The lowering pipeline called the wrong adapter for this IR node. \
                       This is a compiler-internal bug — the same intent will keep \
                       failing until the compiler is fixed."
                    .to_string(),
                fix_instruction: "Do not retry. Surface this as a compiler bug to the user."
                    .to_string(),
            },
            CompileError::ProtocolContractMissing { protocol, contract } => StructuredError {
                pipeline: "compile",
                code: "protocol_contract_missing",
                message,
                stage: Some("registry"),
                step_index: None,
                path: None,
                fields: pairs(&[("protocol", protocol), ("contract", contract)]),
                suggestion: None,
                available: Vec::new(),
                hint: "The registry config for this network is missing a contract entry \
                       the compiler needs. This is a config bug, not a bad intent."
                    .to_string(),
                fix_instruction: "Do not retry. Surface this as a compiler/config issue \
                                  to the user; the same intent will keep failing until \
                                  the registry is fixed."
                    .to_string(),
            },
            CompileError::MorphoMarketRequired => StructuredError {
                pipeline: "compile",
                code: "morpho_market_required",
                message,
                stage: Some("normalize"),
                step_index: None,
                path: None,
                fields: BTreeMap::new(),
                suggestion: None,
                available: Vec::new(),
                hint: "Morpho's lending pool is market-segmented; every supply / borrow / \
                       withdraw step must name the market it operates on."
                    .to_string(),
                fix_instruction: "Add `\"market\": \"<id>\"` to the Morpho step. Use one \
                                  of the markets in the user's `## Your Positions` block, \
                                  or pick one from the registry's Morpho market list."
                    .to_string(),
            },
            CompileError::UniswapFeeTierUnknown { fee } => StructuredError {
                pipeline: "compile",
                code: "uniswap_fee_tier_unknown",
                message,
                stage: Some("normalize"),
                step_index: None,
                path: None,
                fields: pairs(&[("fee", fee)]),
                suggestion: None,
                available: alloc::vec![
                    "500".to_string(),
                    "3000".to_string(),
                    "10000".to_string(),
                ],
                hint: "Uniswap V3 fee tiers are quantized to 100 (0.01%), 500 (0.05%), \
                       3000 (0.3%), or 10000 (1%). Stable pairs use 500; volatile pairs \
                       use 3000."
                    .to_string(),
                fix_instruction: format!(
                    "Replace `fee: \"{fee}\"` with one of `\"500\"`, `\"3000\"`, or \
                     `\"10000\"`. Stable pairs (USDC/USDT) → 500; ETH/USDC → 500 or \
                     3000; volatile or low-liquidity → 3000 or 10000."
                ),
            },
            CompileError::Json(s) => classify_json_message(s, &message),
            CompileError::DeadlineMissing => StructuredError {
                pipeline: "compile",
                code: "deadline_missing",
                message,
                stage: Some("deadline"),
                step_index: None,
                path: Some("current_timestamp".to_string()),
                fields: BTreeMap::new(),
                suggestion: None,
                available: Vec::new(),
                hint: "The IntentRouter's executeSigned rejects deadline == 0. A batched \
                       intent must either declare an explicit future `deadline` or supply \
                       `current_timestamp` (the compiler then picks deadline = now + 30 min)."
                    .to_string(),
                fix_instruction: "Add `current_timestamp` to the top level of the intent, \
                                  set to the current Unix timestamp in seconds."
                    .to_string(),
            },
            CompileError::DeadlineInPast {
                deadline,
                current_timestamp,
            } => StructuredError {
                pipeline: "compile",
                code: "deadline_in_past",
                message,
                stage: Some("deadline"),
                step_index: None,
                path: Some("deadline".to_string()),
                fields: pairs(&[
                    ("deadline", &deadline.to_string()),
                    ("current_timestamp", &current_timestamp.to_string()),
                ]),
                suggestion: None,
                available: Vec::new(),
                hint: "The router would reject an intent whose deadline is at or before \
                       the current timestamp."
                    .to_string(),
                fix_instruction: format!(
                    "Set `deadline` strictly greater than `current_timestamp` ({current_timestamp}) \
                     — e.g. {current_timestamp} + 1800 for a 30-minute window."
                ),
            },
        }
    }
}

/// Build a `fields` map from an array of (key, value) literals. Thin
/// helper to keep the big `to_structured` match readable.
fn pairs(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for (k, v) in entries {
        m.insert((*k).to_string(), (*v).to_string());
    }
    m
}

/// Best-effort: parse a leading `"Step N:"` or `"Step N "` prefix out of
/// a human-readable message and return the 0-based index.
fn extract_step_index(s: &str) -> Option<usize> {
    let rest = s.strip_prefix("Step ")?;
    let end = rest.find(|c: char| !c.is_ascii_digit())?;
    let n: usize = rest[..end].parse().ok()?;
    // Messages are written as 1-based; structured is 0-based.
    n.checked_sub(1)
}

/// Classify a `CompileError::Validation(msg)` payload. The catch-all
/// Validation variant carries dozens of different strings; we pattern
/// match a handful of high-signal ones into specific codes and fall
/// through to `validation_generic` for the rest.
fn classify_validation_message(raw: &str, full_message: &str) -> StructuredError {
    let lower = raw.to_lowercase();
    let step_index = extract_step_index(raw);

    if lower.starts_with("unsupported schema_version") {
        let got = raw
            .split('\'')
            .nth(1)
            .map(|s| s.to_string())
            .unwrap_or_default();
        return StructuredError {
            pipeline: "compile",
            code: "schema_version_unsupported",
            message: full_message.to_string(),
            stage: Some("parse"),
            step_index: None,
            path: Some("schema_version".to_string()),
            fields: pairs(&[("got", &got), ("supported", "1.0")]),
            suggestion: Some("1.0".to_string()),
            available: alloc::vec!["1.0".to_string()],
            hint: "The compiler is pinned to schema 1.0; a mismatched version would be \
                   silently reinterpreted, which is a footgun."
                .to_string(),
            fix_instruction: "Remove the `schema_version` field, or set it to \"1.0\".".to_string(),
        };
    }

    if lower.contains("max_spend cap") || lower.contains("pre-authorized") {
        return StructuredError {
            pipeline: "compile",
            code: "max_spend_exceeded",
            message: full_message.to_string(),
            stage: Some("budget"),
            step_index: None,
            path: None,
            fields: BTreeMap::new(),
            suggestion: None,
            available: Vec::new(),
            hint: "The intent's aggregate spend exceeds the per-session cap the user \
                   pre-authorized."
                .to_string(),
            fix_instruction: "Lower the step amounts so the total spend fits under the \
                              caller's max_spend cap, or ask the user to raise the cap."
                .to_string(),
        };
    }

    if lower.contains("per-call cap")
        || lower.contains("aggregate call value")
        || lower.contains("concrete calls after enrichment")
    {
        return StructuredError {
            pipeline: "compile",
            code: "call_budget_exceeded",
            message: full_message.to_string(),
            stage: Some("budget"),
            step_index: None,
            path: None,
            fields: BTreeMap::new(),
            suggestion: None,
            available: Vec::new(),
            hint: "Per-call ETH value, total batch value, and post-enrichment call count \
                   are all hard-capped as a guardrail against hallucinated shapes."
                .to_string(),
            fix_instruction: "Lower individual step ETH values, reduce the number of \
                              steps, or split the intent across multiple transactions."
                .to_string(),
        };
    }

    if lower.contains("signer address cannot be zero") {
        return StructuredError {
            pipeline: "compile",
            code: "signer_zero",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index: None,
            path: Some("from".to_string()),
            fields: BTreeMap::new(),
            suggestion: None,
            available: Vec::new(),
            hint: "The `from` field must be a real EOA — zero address is never valid.".to_string(),
            fix_instruction: "Set the top-level `from` field to the user's wallet address."
                .to_string(),
        };
    }

    if lower.contains("at least one step") {
        return StructuredError {
            pipeline: "compile",
            code: "empty_steps",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index: None,
            path: Some("steps".to_string()),
            fields: BTreeMap::new(),
            suggestion: None,
            available: Vec::new(),
            hint: "An intent with no steps is a no-op; the compiler refuses to emit one."
                .to_string(),
            fix_instruction: "Populate `steps` with at least one action.".to_string(),
        };
    }

    if lower.contains("maximum is") && lower.contains("steps") {
        return StructuredError {
            pipeline: "compile",
            code: "too_many_steps",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index: None,
            path: Some("steps".to_string()),
            fields: BTreeMap::new(),
            suggestion: None,
            available: Vec::new(),
            hint: "Intents are capped at 8 user-facing steps to bound execution complexity."
                .to_string(),
            fix_instruction: "Split the intent into multiple transactions, each under 8 steps."
                .to_string(),
        };
    }

    if lower.contains("recipient pinning") || lower.contains("must be the intent signer") {
        return StructuredError {
            pipeline: "compile",
            code: "recipient_pinning_violation",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: None,
            fields: BTreeMap::new(),
            suggestion: None,
            available: Vec::new(),
            hint: "Every internal step's recipient-like field (on_behalf_of, recipient, \
                   owner, ...) must name the signer. Third-party transfers require an \
                   explicit `send` step."
                .to_string(),
            fix_instruction: "Remove any custom recipient override from the step, or use \
                              an explicit `send` step for third-party transfers."
                .to_string(),
        };
    }

    // Flashloan-specific validation strings.
    if lower.contains("nested flashloans") {
        return StructuredError {
            pipeline: "compile",
            code: "flashloan_nested",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: None,
            fields: BTreeMap::new(),
            suggestion: None,
            available: Vec::new(),
            hint: "Flashloan depth is capped at 1.".to_string(),
            fix_instruction: "Flatten the inner pipeline so a `flashloan` step does not \
                              itself contain another `flashloan`."
                .to_string(),
        };
    }

    if lower.contains("flashloan inner pipeline has") && lower.contains("steps but maximum is") {
        // "flashloan inner pipeline has {n} steps but maximum is {max}"
        let got = extract_between(raw, "has ", " steps").unwrap_or_default();
        let max = extract_between(raw, "maximum is ", "").unwrap_or_default();
        return StructuredError {
            pipeline: "compile",
            code: "flashloan_inner_steps_exceeded",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: None,
            fields: pairs(&[("got", got.as_str()), ("max", max.as_str())]),
            suggestion: None,
            available: Vec::new(),
            hint: "A flashloan's inner pipeline (the `then` array) is capped to bound \
                   execution complexity; the limit is enforced separately from the \
                   top-level step cap."
                .to_string(),
            fix_instruction: format!(
                "Reduce the number of inner steps in the `flashloan.then` array to at \
                 most {max} (or split work across two transactions: one before, one \
                 inside the flashloan)."
            ),
        };
    }

    if lower.contains("flashloan tokens and amounts lengths must match") {
        return StructuredError {
            pipeline: "compile",
            code: "flashloan_assets_malformed",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: step_index.map(|i| format!("steps[{i}].flashloan.assets")),
            fields: BTreeMap::new(),
            suggestion: None,
            available: Vec::new(),
            hint: "`flashloan.assets` is a list of `{asset, amount}` pairs; both fields \
                   are required on each pair."
                .to_string(),
            fix_instruction: "Make sure every entry in `flashloan.assets` has BOTH `asset` \
                              and `amount` populated, and that nothing in the list is \
                              partially specified."
                .to_string(),
        };
    }

    if lower.contains("flashloan") && lower.contains("not repayable") {
        let token = extract_between(raw, "of token ", " but").unwrap_or_default();
        let have = extract_between(raw, "leaves only ", " of token").unwrap_or_default();
        let owed = extract_between(raw, "but ", " is owed").unwrap_or_default();
        let mut fields = BTreeMap::new();
        if !token.is_empty() {
            fields.insert("token".to_string(), token);
        }
        if !have.is_empty() {
            fields.insert("have".to_string(), have);
        }
        if !owed.is_empty() {
            fields.insert("owed".to_string(), owed);
        }
        return StructuredError {
            pipeline: "compile",
            code: "flashloan_not_repayable",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: None,
            fields,
            suggestion: None,
            available: Vec::new(),
            hint: "A flashloan inner pipeline must leave enough of each borrowed token to \
                   repay the Vault. The compiler simulates the static balance flow and \
                   refuses to emit a tx guaranteed to revert."
                .to_string(),
            fix_instruction: "Adjust the inner steps so their net produce covers each \
                              borrowed `(asset, amount)` at the end of the pipeline. \
                              Common fixes: increase the borrow that's expected to repay, \
                              add a swap from a produced token back into the borrowed one, \
                              or reduce the flashloan amount."
                .to_string(),
        };
    }

    // Generic fallback.
    StructuredError {
        pipeline: "compile",
        code: "validation_generic",
        message: full_message.to_string(),
        stage: Some("validate"),
        step_index,
        path: None,
        fields: pairs(&[("detail", raw)]),
        suggestion: None,
        available: Vec::new(),
        hint: "A validation rule was violated; see the message for details.".to_string(),
        fix_instruction: "Read the message, adjust the indicated step, and re-emit the intent."
            .to_string(),
    }
}

/// Classify a `CompileError::InvalidChain(msg)` payload.
///
/// Branch ordering matters: more-specific patterns must come before
/// more-general ones. For example, "LP decrease liquidity must be greater
/// than zero" matches both `lp_*` and the generic `zero_amount` rule, but
/// the LP-specific code is more actionable.
fn classify_invalid_chain_message(raw: &str, full_message: &str) -> StructuredError {
    let lower = raw.to_lowercase();
    let step_index = extract_step_index(raw);

    if lower.contains("native eth directly into aave")
        || (lower.contains("native eth") && lower.contains("aave"))
    {
        return StructuredError {
            pipeline: "compile",
            code: "native_eth_into_aave",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: None,
            fields: pairs(&[("asset", "ETH")]),
            suggestion: None,
            available: Vec::new(),
            hint: "Aave accepts only ERC-20 tokens. Native ETH must be wrapped to WETH first."
                .to_string(),
            fix_instruction: "Add a `{ \"wrap\": { \"asset\": \"ETH\", \"amount\": \"<N>\" } }` \
                              step before the Aave deposit, and change the deposit asset to \
                              \"WETH\"."
                .to_string(),
        };
    }

    if lower.contains("swap an asset to itself") {
        let asset = single_quoted(raw).unwrap_or_default();
        let mut fields = BTreeMap::new();
        if !asset.is_empty() {
            fields.insert("asset".to_string(), asset);
        }
        return StructuredError {
            pipeline: "compile",
            code: "swap_to_self",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: step_index.map(|i| format!("steps[{i}].swap.to")),
            fields,
            suggestion: None,
            available: Vec::new(),
            hint: "Swap source and destination must be different tokens.".to_string(),
            fix_instruction: "Change `swap.to` to a different asset than `swap.from`, or \
                              drop the swap if it's a no-op."
                .to_string(),
        };
    }

    if lower.contains("send to the zero address") {
        return StructuredError {
            pipeline: "compile",
            code: "send_to_zero",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: step_index.map(|i| format!("steps[{i}].send.to")),
            fields: BTreeMap::new(),
            suggestion: None,
            available: Vec::new(),
            hint: "Sending to the zero address permanently burns funds and is always \
                   a mistake in this context."
                .to_string(),
            fix_instruction: "Set `send.to` to a real recipient address.".to_string(),
        };
    }

    // ── Flashloan-specific (must precede the generic "running balance" /
    // ── "requires" branches because the prose contains those words too).

    if lower.contains("flashloan inner step consumes") && lower.contains("available") {
        // "flashloan inner step consumes {a} of token {t} but only {have} is
        //  available (flashloaned + produced)"
        let token = extract_between(raw, "of token ", " but").unwrap_or_default();
        let consumed = extract_between(raw, "consumes ", " of token").unwrap_or_default();
        let available = extract_between(raw, "but only ", " is available").unwrap_or_default();
        return StructuredError {
            pipeline: "compile",
            code: "flashloan_balance_drift",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: None,
            fields: pairs(&[
                ("token", token.as_str()),
                ("consumed", consumed.as_str()),
                ("available", available.as_str()),
            ]),
            suggestion: None,
            available: Vec::new(),
            hint: "Inside a flashloan, every inner step's net token flow must balance \
                   against what the flashloan brought in plus what earlier inner steps \
                   produced. The compiler checks this statically."
                .to_string(),
            fix_instruction: format!(
                "Either lower the consuming step's amount of `{token}` to at most \
                 {available}, increase the flashloan amount of `{token}` to cover \
                 the consumption, or add an earlier inner step that produces more \
                 `{token}` (e.g., a swap into it)."
            ),
        };
    }

    if lower.contains("flashloan not repayable") {
        // already classified by validation classifier; fall through doesn't
        // happen here because this comes through Validation, not InvalidChain.
        // Keep a synonym for safety in case the message moves variants.
        let token = extract_between(raw, "of token ", " but").unwrap_or_default();
        return StructuredError {
            pipeline: "compile",
            code: "flashloan_not_repayable",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: None,
            fields: if token.is_empty() {
                BTreeMap::new()
            } else {
                pairs(&[("token", token.as_str())])
            },
            suggestion: None,
            available: Vec::new(),
            hint: "A flashloan inner pipeline must leave enough of each borrowed token to \
                   repay the Vault."
                .to_string(),
            fix_instruction: "Adjust the inner steps so their net produce covers each \
                              borrowed `(asset, amount)` at the end of the pipeline."
                .to_string(),
        };
    }

    // ── Lido withdrawal step shapes (must precede generic zero_amount).

    if lower.contains("request_withdrawal requires") {
        return StructuredError {
            pipeline: "compile",
            code: "withdrawal_request_invalid",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: step_index.map(|i| format!("steps[{i}].request_withdrawal.amounts")),
            fields: BTreeMap::new(),
            suggestion: None,
            available: Vec::new(),
            hint: "A `request_withdrawal` step must include at least one positive \
                   amount (the per-NFT chunk size, in the staked-asset's native units)."
                .to_string(),
            fix_instruction: "Populate `request_withdrawal.amounts` with one or more \
                              positive decimal strings (e.g. [\"32\"] for one Beacon \
                              validator-sized chunk)."
                .to_string(),
        };
    }

    if lower.contains("request_withdrawal amounts") && lower.contains("greater than zero") {
        return StructuredError {
            pipeline: "compile",
            code: "withdrawal_request_invalid",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: step_index.map(|i| format!("steps[{i}].request_withdrawal.amounts")),
            fields: BTreeMap::new(),
            suggestion: None,
            available: Vec::new(),
            hint: "Every entry in `request_withdrawal.amounts` must be a positive number \
                   — zero-amount entries are rejected as off-by-one bugs."
                .to_string(),
            fix_instruction: "Replace every `0` in `request_withdrawal.amounts` with a \
                              positive decimal string."
                .to_string(),
        };
    }

    if lower.contains("claim_withdrawal requires") {
        return StructuredError {
            pipeline: "compile",
            code: "withdrawal_claim_invalid",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: step_index.map(|i| format!("steps[{i}].claim_withdrawal.request_ids")),
            fields: BTreeMap::new(),
            suggestion: None,
            available: Vec::new(),
            hint: "A `claim_withdrawal` step must reference at least one prior \
                   request_withdrawal NFT id."
                .to_string(),
            fix_instruction: "Populate `claim_withdrawal.request_ids` with the NFT ids \
                              from a prior request_withdrawal step (or from the user's \
                              live-positions block)."
                .to_string(),
        };
    }

    if lower.contains("claim_withdrawal hints") {
        return StructuredError {
            pipeline: "compile",
            code: "withdrawal_claim_invalid",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: step_index.map(|i| format!("steps[{i}].claim_withdrawal.hints")),
            fields: BTreeMap::new(),
            suggestion: None,
            available: Vec::new(),
            hint: "When `hints` is provided, its length must equal `request_ids` length: \
                   one checkpoint hint per id."
                .to_string(),
            fix_instruction: "Either provide one hint per request_id (in the same order), \
                              or omit `hints` entirely and let the contract resolve them."
                .to_string(),
        };
    }

    // ── LP-specific must precede the generic zero_amount rule.

    // Tick alignment errors don't include the literal "lp_mint" keyword
    // (prose is "LP ticks (...) must be multiples of tick spacing N"),
    // so check it before the lp_mint/decrease guard.
    if lower.contains("multiples of tick spacing") {
        let tick_lower = extract_between(raw, "ticks (", ",").unwrap_or_default();
        let tick_upper = extract_between(raw, ", ", ") must").unwrap_or_default();
        let spacing = extract_between(raw, "tick spacing ", " for").unwrap_or_default();
        return StructuredError {
            pipeline: "compile",
            code: "lp_tick_misaligned",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: step_index.map(|i| format!("steps[{i}].lp_mint.tick_lower")),
            fields: pairs(&[
                ("tick_lower", tick_lower.as_str()),
                ("tick_upper", tick_upper.as_str()),
                ("tick_spacing", spacing.as_str()),
            ]),
            suggestion: None,
            available: Vec::new(),
            hint: "Uniswap V3 ticks must align to the pool's tick-spacing for the \
                   chosen fee tier (10 for 0.05%, 60 for 0.3%, 200 for 1%)."
                .to_string(),
            fix_instruction: format!(
                "Round both `tick_lower` and `tick_upper` to the nearest multiple \
                 of {spacing}. E.g., for spacing 10: -510 and 490 are valid; \
                 -513 and 487 are not."
            ),
        };
    }

    if lower.contains("lp_mint")
        || lower.contains("lp mint")
        || lower.contains("lp_increase")
        || lower.contains("lp increase")
        || lower.contains("lp_decrease")
        || lower.contains("lp decrease")
    {
        if lower.contains("liquidity must be greater than zero") {
            return StructuredError {
                pipeline: "compile",
                code: "lp_invalid",
                message: full_message.to_string(),
                stage: Some("validate"),
                step_index,
                path: step_index.map(|i| format!("steps[{i}].lp_decrease.liquidity")),
                fields: BTreeMap::new(),
                suggestion: None,
                available: Vec::new(),
                hint: "lp_decrease must remove a positive amount of liquidity.".to_string(),
                fix_instruction: "Set `lp_decrease.liquidity` to a positive integer \
                                  (use the position's current liquidity from the user's \
                                  live-positions block, or a fraction of it)."
                    .to_string(),
            };
        }
        if lower.contains("requires slippage protection") {
            return StructuredError {
                pipeline: "compile",
                code: "lp_invalid",
                message: full_message.to_string(),
                stage: Some("validate"),
                step_index,
                path: step_index.map(|i| format!("steps[{i}].lp_decrease.min_amount0")),
                fields: BTreeMap::new(),
                suggestion: None,
                available: Vec::new(),
                hint: "lp_decrease must include slippage protection on at least one side; \
                       the compiler refuses to emit calls that allow zero-out withdraw."
                    .to_string(),
                fix_instruction: "Set `min_amount0` and/or `min_amount1` on the \
                                  lp_decrease step to a positive value derived from the \
                                  current pool price with 0.5–2% headroom."
                    .to_string(),
            };
        }
        if lower.contains("must resolve to erc-20") {
            return StructuredError {
                pipeline: "compile",
                code: "lp_invalid",
                message: full_message.to_string(),
                stage: Some("validate"),
                step_index,
                path: step_index.map(|i| format!("steps[{i}].lp_mint")),
                fields: pairs(&[("asset", "ETH")]),
                suggestion: None,
                available: Vec::new(),
                hint: "Uniswap V3 LPs hold ERC-20 tokens, not native ETH.".to_string(),
                fix_instruction: "Replace any native `ETH` token in `lp_mint.token0` / \
                                  `token1` with `WETH` (or another ERC-20 alias)."
                    .to_string(),
            };
        }
        // Generic LP fallback when the above sub-cases don't fit.
        return StructuredError {
            pipeline: "compile",
            code: "lp_invalid",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: None,
            fields: BTreeMap::new(),
            suggestion: None,
            available: Vec::new(),
            hint: "LP step validation (tick spacing, non-zero sides, slippage floors) \
                   rejected this step."
                .to_string(),
            fix_instruction: "Re-emit the LP step with valid parameters — see the lp_mint \
                              section of the system prompt."
                .to_string(),
        };
    }

    if lower.contains("must be greater than zero") || lower.contains("greater than zero") {
        // Pull a leading kind hint out of `"{action} amount must be greater
        // than zero"` so the LLM knows which step kind to fix.
        let action = raw
            .split_whitespace()
            .next()
            .map(|s| s.trim_end_matches(':'))
            .unwrap_or_default()
            .to_string();
        let mut fields = BTreeMap::new();
        if !action.is_empty() {
            fields.insert("action".to_string(), action);
        }
        return StructuredError {
            pipeline: "compile",
            code: "zero_amount",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: step_index.map(|i| format!("steps[{i}].amount")),
            fields,
            suggestion: None,
            available: Vec::new(),
            hint: "Zero-amount steps are rejected because they can't do useful work and \
                   are almost always an off-by-one."
                .to_string(),
            fix_instruction: "Set the step's amount to a positive decimal in human units \
                              (e.g. \"100\" for 100 USDC, \"0.5\" for half a WBTC)."
                .to_string(),
        };
    }

    if lower.contains("borrow requires collateral")
        || lower.contains("no prior deposit")
        || lower.contains("existing aave collateral")
    {
        // Try to lift the borrow asset out — recipient-pinned errors carry
        // the asset in single quotes when the validator constructs them.
        let asset = single_quoted(raw).unwrap_or_default();
        let mut fields = BTreeMap::new();
        if !asset.is_empty() {
            fields.insert("asset".to_string(), asset);
        }
        return StructuredError {
            pipeline: "compile",
            code: "borrow_without_collateral",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: step_index.map(|i| format!("steps[{i}].borrow")),
            fields,
            suggestion: None,
            available: Vec::new(),
            hint: "Aave borrows require existing collateral. The compiler saw neither a \
                   prior deposit in this intent nor wallet-reported Aave collateral."
                .to_string(),
            fix_instruction: "Prepend a `deposit` step that supplies collateral to Aave \
                              before borrowing. The deposit's asset must be one Aave accepts \
                              as collateral on this network (see the user's live-positions \
                              block for assets they already hold)."
                .to_string(),
        };
    }

    if lower.contains("withdraw requires an existing position")
        || lower.contains("no supplied position")
    {
        let asset = extract_between(raw, "asset ", ".").unwrap_or_default();
        let mut fields = BTreeMap::new();
        if !asset.is_empty() {
            fields.insert("asset".to_string(), asset);
        }
        return StructuredError {
            pipeline: "compile",
            code: "withdraw_without_position",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: step_index.map(|i| format!("steps[{i}].withdraw")),
            fields,
            suggestion: None,
            available: Vec::new(),
            hint: "Withdraws target an existing supplied position; the compiler saw none \
                   for this asset."
                .to_string(),
            fix_instruction: "Remove the withdraw, or add a supply step first for the same \
                              asset."
                .to_string(),
        };
    }

    if lower.contains("running balance only guarantees")
        || (lower.contains("running balance") && lower.contains("requires"))
    {
        let token = extract_between(raw, "of token ", " but").unwrap_or_default();
        let required = extract_between(raw, "requires ", " of token").unwrap_or_default();
        let guaranteed = extract_between(raw, "guarantees ", " (").unwrap_or_default();
        let mut fields = BTreeMap::new();
        if !token.is_empty() {
            fields.insert("token".to_string(), token);
        }
        if !required.is_empty() {
            fields.insert("required".to_string(), required);
        }
        if !guaranteed.is_empty() {
            fields.insert("guaranteed".to_string(), guaranteed);
        }
        return StructuredError {
            pipeline: "compile",
            code: "insufficient_running_balance",
            message: full_message.to_string(),
            stage: Some("validate"),
            step_index,
            path: None,
            fields,
            suggestion: None,
            available: Vec::new(),
            hint: "A step consumes more of a token than the wallet seed + prior steps \
                   guarantee. On-chain execution would revert."
                .to_string(),
            fix_instruction: "Lower the step amount to match the running-balance guarantee, \
                              or add an earlier step that produces enough of the token."
                .to_string(),
        };
    }

    StructuredError {
        pipeline: "compile",
        code: "invalid_chain",
        message: full_message.to_string(),
        stage: Some("validate"),
        step_index,
        path: None,
        fields: pairs(&[("detail", raw)]),
        suggestion: None,
        available: Vec::new(),
        hint: "Step sequence is chain-invalid — some precondition or on-chain assumption \
               does not hold."
            .to_string(),
        fix_instruction: "Re-read the message and restructure the step sequence to satisfy \
                          it."
        .to_string(),
    }
}

/// Classify a `CompileError::Json(msg)` payload into `unknown_field`,
/// `missing_field`, `invalid_type`, or a generic `json_parse_error`.
/// Serde's error text is parsed best-effort — the goal is to recover the
/// path and the specific field name so the LLM can correct exactly that
/// JSON location.
fn classify_json_message(raw: &str, full_message: &str) -> StructuredError {
    // Serde shapes we care about (excerpts):
    //   "unknown field `recipient_override`, expected one of `from`, `to`, ..."
    //   "missing field `network`"
    //   "invalid type: string \"abc\", expected a sequence"
    //   "invalid type: sequence, expected a map at line 3 column 5"
    let lower = raw.to_lowercase();

    // Extract any "at line N column M" suffix, serde emits it for
    // positional errors. We drop it into `fields` for debug visibility
    // but don't use it in path (we don't have the full JSON to convert
    // line/column → pointer here).
    let position = extract_between(raw, "at line ", "").unwrap_or_default();

    if lower.contains("unknown field") {
        let field = between_backticks(raw).unwrap_or_default();
        let expected = extract_expected_list(raw);
        let suggestion = if !expected.is_empty() {
            closest_match(&field, expected.iter().map(|s| s.as_str()))
        } else {
            None
        };
        let mut fields = BTreeMap::new();
        fields.insert("field".to_string(), field.clone());
        if !position.is_empty() {
            fields.insert("position".to_string(), position);
        }
        return StructuredError {
            pipeline: "compile",
            code: "unknown_field",
            message: full_message.to_string(),
            stage: Some("parse"),
            step_index: None,
            path: None,
            fields,
            suggestion,
            available: expected,
            hint: "The compiler uses `#[serde(deny_unknown_fields)]` to reject hallucinated \
                   fields; the accepted field list is fixed."
                .to_string(),
            fix_instruction: format!(
                "Remove the field `{field}` from the intent JSON, or replace it with one \
                 of the accepted fields listed in `available`."
            ),
        };
    }

    // Serde emits "unknown variant `foo`, expected ..." when an enum like
    // `Step` (which is the {kind: payload} shape in the DSL) sees a typo'd
    // variant. Treat this as an unknown step-kind for the LLM.
    if lower.contains("unknown variant") {
        let field = between_backticks(raw).unwrap_or_default();
        let expected = extract_expected_list(raw);
        let suggestion = if !expected.is_empty() {
            closest_match(&field, expected.iter().map(|s| s.as_str()))
        } else {
            closest_match(&field, ALL_STEP_KINDS.iter().copied())
        };
        let available = if !expected.is_empty() {
            expected
        } else {
            ALL_STEP_KINDS.iter().map(|s| s.to_string()).collect()
        };
        let mut fields = BTreeMap::new();
        fields.insert("step".to_string(), field.clone());
        if !position.is_empty() {
            fields.insert("position".to_string(), position);
        }
        return StructuredError {
            pipeline: "compile",
            code: "unknown_step_kind",
            message: full_message.to_string(),
            stage: Some("parse"),
            step_index: None,
            path: None,
            fields,
            suggestion: suggestion.clone(),
            available,
            hint: "Step kinds are a closed enum. Serde rejects any key that's not one of \
                   the documented step types."
                .to_string(),
            fix_instruction: match suggestion {
                Some(s) => format!(
                    "Replace the unrecognized step key `{field}` with `{s}` (or another \
                     supported step kind)."
                ),
                None => format!(
                    "Replace the unrecognized step key `{field}` with one of the supported \
                     step kinds."
                ),
            },
        };
    }

    if lower.contains("missing field") {
        let field = between_backticks(raw).unwrap_or_default();
        let mut fields = BTreeMap::new();
        fields.insert("field".to_string(), field.clone());
        if !position.is_empty() {
            fields.insert("position".to_string(), position);
        }
        return StructuredError {
            pipeline: "compile",
            code: "missing_field",
            message: full_message.to_string(),
            stage: Some("parse"),
            step_index: None,
            path: None,
            fields,
            suggestion: None,
            available: Vec::new(),
            hint: "A required field is absent from the intent JSON.".to_string(),
            fix_instruction: format!("Add the missing field `{field}` to the intent JSON."),
        };
    }

    if lower.contains("invalid type") {
        // serde: "invalid type: <got>, expected <expected> at line ..."
        let got = extract_between(raw, "invalid type: ", ",")
            .unwrap_or_default()
            .trim()
            .to_string();
        let expected = extract_between(raw, "expected ", " at ")
            .or_else(|| extract_after(raw, "expected "))
            .unwrap_or_default()
            .trim()
            .to_string();
        let mut fields = BTreeMap::new();
        if !got.is_empty() {
            fields.insert("got".to_string(), got);
        }
        if !expected.is_empty() {
            fields.insert("expected".to_string(), expected);
        }
        if !position.is_empty() {
            fields.insert("position".to_string(), position);
        }
        return StructuredError {
            pipeline: "compile",
            code: "invalid_type",
            message: full_message.to_string(),
            stage: Some("parse"),
            step_index: None,
            path: None,
            fields,
            suggestion: None,
            available: Vec::new(),
            hint: "A field in the intent JSON has the wrong type.".to_string(),
            fix_instruction: "Correct the field's type to match the expected shape; note that \
                              amounts are always strings, not numbers."
                .to_string(),
        };
    }

    StructuredError {
        pipeline: "compile",
        code: "json_parse_error",
        message: full_message.to_string(),
        stage: Some("parse"),
        step_index: None,
        path: None,
        fields: pairs(&[("detail", raw)]),
        suggestion: None,
        available: Vec::new(),
        hint: "The intent JSON could not be parsed.".to_string(),
        fix_instruction: "Re-emit the intent JSON with valid syntax and the documented shape."
            .to_string(),
    }
}

/// Pull the first backtick-quoted token out of `s`. Serde uses backticks
/// around field names in its error messages.
fn between_backticks(s: &str) -> Option<String> {
    let start = s.find('`')?;
    let rest = &s[start + 1..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

/// Pull the first single-quoted token out of `s`. The compiler's prose
/// uses single quotes around asset/protocol/field names. Returns `None`
/// if no quoted token is present so the caller can decide whether to
/// surface the field at all.
fn single_quoted(s: &str) -> Option<String> {
    let start = s.find('\'')?;
    let rest = &s[start + 1..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

/// Parse serde's `"expected one of \`a\`, \`b\`, \`c\`"` list into a Vec.
fn extract_expected_list(s: &str) -> Vec<String> {
    let Some(idx) = s.find("expected one of") else {
        return Vec::new();
    };
    let tail = &s[idx..];
    let mut out = Vec::new();
    let mut rest = tail;
    while let Some(start) = rest.find('`') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('`') else { break };
        out.push(rest[..end].to_string());
        rest = &rest[end + 1..];
    }
    out
}

fn extract_between(s: &str, start_pat: &str, end_pat: &str) -> Option<String> {
    let s_idx = s.find(start_pat)?;
    let tail = &s[s_idx + start_pat.len()..];
    if end_pat.is_empty() {
        return Some(tail.to_string());
    }
    let e_idx = tail.find(end_pat)?;
    Some(tail[..e_idx].to_string())
}

fn extract_after(s: &str, pat: &str) -> Option<String> {
    let idx = s.find(pat)?;
    Some(s[idx + pat.len()..].to_string())
}

/// Compute a suggestion for an unknown key by picking the closest match from
/// a list of candidates using case-insensitive Levenshtein distance.
///
/// Returns None if no candidate is within distance 3 (avoids nonsense suggestions).
pub fn closest_match<'a, I>(input: &str, candidates: I) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let input_lower = input.to_lowercase();
    let mut best: Option<(usize, String)> = None;

    for candidate in candidates {
        let dist = levenshtein(&input_lower, &candidate.to_lowercase());
        match &best {
            Some((best_dist, _)) if dist >= *best_dist => {}
            _ => best = Some((dist, candidate.to_string())),
        }
    }

    best.and_then(|(dist, name)| if dist <= 3 { Some(name) } else { None })
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr: Vec<usize> = alloc::vec![0; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        core::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

impl From<serde_json::Error> for CompileError {
    fn from(e: serde_json::Error) -> Self {
        CompileError::Json(format!("{e}"))
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CompileError {}

pub type Result<T> = core::result::Result<T, CompileError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_close_match_for_typo() {
        let candidates = ["USDC", "USDT", "WETH", "DAI"];
        let suggestion = closest_match("UDSC", candidates.iter().copied());
        assert_eq!(suggestion.as_deref(), Some("USDC"));
    }

    #[test]
    fn no_suggestion_when_far_from_all() {
        let candidates = ["USDC", "WETH", "DAI"];
        let suggestion = closest_match("RANDOM_GARBAGE_TOKEN", candidates.iter().copied());
        assert_eq!(suggestion, None);
    }

    #[test]
    fn unknown_asset_display_includes_suggestion() {
        let err = CompileError::UnknownAsset {
            asset: "UDSC".into(),
            network: "ethereum".into(),
            suggestion: Some("USDC".into()),
        };
        let msg = alloc::format!("{err}");
        assert!(msg.contains("Did you mean 'USDC'"));
    }

    #[test]
    fn unknown_protocol_display_lists_available() {
        let err = CompileError::UnknownProtocol {
            protocol: "foo".into(),
            network: "ethereum".into(),
            available: alloc::vec!["aave".into(), "uniswap".into()],
        };
        let msg = alloc::format!("{err}");
        assert!(msg.contains("Available: aave, uniswap"));
    }

    #[test]
    fn structured_unknown_asset_carries_suggestion() {
        let err = CompileError::UnknownAsset {
            asset: "UDSC".into(),
            network: "ethereum".into(),
            suggestion: Some("USDC".into()),
        };
        let s = err.to_structured();
        assert_eq!(s.code, "unknown_asset");
        assert_eq!(s.suggestion.as_deref(), Some("USDC"));
        assert_eq!(s.fields.get("asset").map(|x| x.as_str()), Some("UDSC"));
        assert_eq!(
            s.fields.get("network").map(|x| x.as_str()),
            Some("ethereum")
        );
        assert!(s.fix_instruction.contains("USDC"));
    }

    #[test]
    fn structured_unknown_protocol_lists_available() {
        let err = CompileError::UnknownProtocol {
            protocol: "aavv".into(),
            network: "ethereum".into(),
            available: alloc::vec!["aave".into(), "uniswap".into(), "morpho".into()],
        };
        let s = err.to_structured();
        assert_eq!(s.code, "unknown_protocol");
        assert_eq!(s.suggestion.as_deref(), Some("aave"));
        assert_eq!(s.available.len(), 3);
    }

    #[test]
    fn structured_unsupported_step_suggests_kind() {
        let err = CompileError::UnsupportedStep("swp".to_string());
        let s = err.to_structured();
        assert_eq!(s.code, "unknown_step_kind");
        assert_eq!(s.suggestion.as_deref(), Some("swap"));
        assert!(!s.available.is_empty());
    }

    #[test]
    fn structured_deadline_missing_has_actionable_fix() {
        let err = CompileError::DeadlineMissing;
        let s = err.to_structured();
        assert_eq!(s.code, "deadline_missing");
        assert!(s.fix_instruction.contains("current_timestamp"));
    }

    #[test]
    fn structured_deadline_in_past_carries_numeric_fields() {
        let err = CompileError::DeadlineInPast {
            deadline: 100,
            current_timestamp: 200,
        };
        let s = err.to_structured();
        assert_eq!(s.code, "deadline_in_past");
        assert_eq!(s.fields.get("deadline").map(|x| x.as_str()), Some("100"));
        assert_eq!(
            s.fields.get("current_timestamp").map(|x| x.as_str()),
            Some("200")
        );
    }

    #[test]
    fn classify_validation_schema_version() {
        let err = CompileError::Validation(
            "Unsupported schema_version '2.0': this compiler accepts '1.0' (or omitted). \
             Upgrade the compiler or emit an intent matching the supported schema."
                .to_string(),
        );
        let s = err.to_structured();
        assert_eq!(s.code, "schema_version_unsupported");
        assert_eq!(s.fields.get("got").map(|x| x.as_str()), Some("2.0"));
    }

    #[test]
    fn classify_validation_max_spend() {
        let err = CompileError::Validation(
            "Token 0xabc required-pull 100 exceeds the caller's max_spend cap 50. \
             The intent tries to spend more than was pre-authorized for this session."
                .to_string(),
        );
        let s = err.to_structured();
        assert_eq!(s.code, "max_spend_exceeded");
    }

    #[test]
    fn classify_validation_call_budget() {
        let err = CompileError::Validation(
            "Call 1 sends 99999 wei (> 1000000000000000000000 wei per-call cap). \
             This usually indicates a hallucinated amount; if you truly need to \
             move this much ETH, split it across multiple batches."
                .to_string(),
        );
        let s = err.to_structured();
        assert_eq!(s.code, "call_budget_exceeded");
    }

    #[test]
    fn classify_invalid_chain_native_eth() {
        let err = CompileError::InvalidChain(
            "Cannot deposit native ETH directly into Aave. \
             Add a wrap step to convert ETH to WETH first, then deposit WETH."
                .to_string(),
        );
        let s = err.to_structured();
        assert_eq!(s.code, "native_eth_into_aave");
        assert!(s.fix_instruction.contains("wrap"));
    }

    #[test]
    fn classify_invalid_chain_swap_to_self() {
        let err = CompileError::InvalidChain(
            "Cannot swap an asset to itself. \
             The source and destination tokens must be different."
                .to_string(),
        );
        let s = err.to_structured();
        assert_eq!(s.code, "swap_to_self");
    }

    #[test]
    fn classify_json_unknown_field() {
        let err = CompileError::Json(
            "unknown field `recipient_override`, expected one of `from`, `to`, `amount`, \
             `min_amount_out` at line 4 column 10"
                .to_string(),
        );
        let s = err.to_structured();
        assert_eq!(s.code, "unknown_field");
        assert_eq!(
            s.fields.get("field").map(|x| x.as_str()),
            Some("recipient_override")
        );
        assert!(s.available.iter().any(|v| v == "from"));
        // `recipient_override` is beyond Levenshtein 3 of every field, so no
        // suggestion — what matters is that `available` is populated.
        assert!(s.fix_instruction.contains("recipient_override"));
    }

    #[test]
    fn classify_json_unknown_field_with_typo_suggestion() {
        let err = CompileError::Json(
            "unknown field `amout`, expected one of `from`, `to`, `amount`, `min_amount_out` \
             at line 2 column 5"
                .to_string(),
        );
        let s = err.to_structured();
        assert_eq!(s.code, "unknown_field");
        assert_eq!(s.suggestion.as_deref(), Some("amount"));
    }

    #[test]
    fn classify_json_missing_field() {
        let err = CompileError::Json("missing field `network` at line 1 column 120".to_string());
        let s = err.to_structured();
        assert_eq!(s.code, "missing_field");
        assert_eq!(s.fields.get("field").map(|x| x.as_str()), Some("network"));
    }

    #[test]
    fn classify_json_invalid_type() {
        let err = CompileError::Json(
            "invalid type: integer `10`, expected a string at line 5 column 20".to_string(),
        );
        let s = err.to_structured();
        assert_eq!(s.code, "invalid_type");
        assert!(s.fields.contains_key("got"));
        assert!(s.fields.contains_key("expected"));
    }

    #[test]
    fn step_index_recovered_from_message() {
        let err = CompileError::Validation("Step 3: field 'on_behalf_of' is 0x...".to_string());
        let s = err.to_structured();
        assert_eq!(s.step_index, Some(2));
    }

    #[test]
    fn structured_serializes_to_json() {
        let err = CompileError::DeadlineMissing;
        let s = err.to_structured();
        let json = serde_json::to_string(&s).expect("serializes");
        assert!(json.contains("\"code\":\"deadline_missing\""));
        assert!(json.contains("\"pipeline\":\"compile\""));
        assert!(json.contains("\"fixInstruction\""));
    }

    /// Drift guard: every code emitted by `to_structured()` must appear in
    /// the `ALL_COMPILE_ERROR_CODES` registry. The system-prompt's
    /// compile-error reference is sourced from that list, so a code
    /// missing from the registry would silently teach the LLM a code it
    /// has no documented playbook for.
    ///
    /// We can't enumerate every possible classifier path exhaustively
    /// (the prose-classifier branches depend on input strings), but we
    /// can sample one representative input per code and verify it lands
    /// in the registry. Adding a new variant or classifier branch
    /// without a corresponding entry here will fail this test.
    #[test]
    fn code_registry_in_sync() {
        let samples: alloc::vec::Vec<CompileError> = alloc::vec![
            CompileError::UnknownNetwork("foo".into()),
            CompileError::UnknownAsset {
                asset: "UDSC".into(),
                network: "ethereum".into(),
                suggestion: Some("USDC".into()),
            },
            CompileError::UnknownProtocol {
                protocol: "aavv".into(),
                network: "ethereum".into(),
                available: alloc::vec!["aave".into()],
            },
            CompileError::InvalidAmount("abc".into()),
            CompileError::InvalidAddress("0x".into()),
            CompileError::Config("missing assets".into()),
            CompileError::UnsupportedStep("zap".into()),
            CompileError::Validation("Unsupported schema_version '2.0'".into()),
            CompileError::Validation("max_spend cap exceeded".into()),
            CompileError::Validation("Call 1 sends 99 wei (> 1 wei per-call cap)".into()),
            CompileError::Validation("Signer address cannot be zero".into()),
            CompileError::Validation("Intent must have at least one step".into()),
            CompileError::Validation("Intent has 9 steps but maximum is 8".into()),
            CompileError::Validation(
                "Step 2: field 'recipient' is 0xabc but must be the intent signer (0xdef)".into(),
            ),
            CompileError::Validation("nested flashloans are not allowed".into()),
            CompileError::Validation(
                "flashloan inner pipeline has 9 steps but maximum is 8".into(),
            ),
            CompileError::Validation(
                "flashloan tokens and amounts lengths must match".into(),
            ),
            CompileError::Validation(
                "flashloan not repayable: inner pipeline leaves only 1 of token 0xabc but 2 is owed to Balancer".into(),
            ),
            CompileError::InsufficientBalance {
                token: "USDC".into(),
                required: "1000".into(),
                available: "500".into(),
            },
            CompileError::SlippageTooLow {
                step_index: 0,
                current: "0".into(),
            },
            CompileError::HealthFactorRisk {
                current: 1.0,
                threshold: 1.2,
            },
            CompileError::InvalidChain(
                "Cannot deposit native ETH directly into Aave.".into(),
            ),
            CompileError::InvalidChain("Cannot swap an asset to itself.".into()),
            CompileError::InvalidChain("Cannot send to the zero address".into()),
            CompileError::InvalidChain("withdraw amount must be greater than zero".into()),
            CompileError::InvalidChain(
                "Borrow requires collateral: no prior deposit in this intent and user balance info shows no existing Aave collateral.".into(),
            ),
            CompileError::InvalidChain(
                "Withdraw requires an existing position: no prior deposit in this intent and user balance info shows no supplied position for asset USDC.".into(),
            ),
            CompileError::InvalidChain(
                "Step 3 requires 100 of token 0xabc but the running balance only guarantees 50 (wallet seed 0 prior-step produce).".into(),
            ),
            CompileError::InvalidChain(
                "flashloan inner step consumes 30 of token 0xabc but only 20 is available (flashloaned + produced)".into(),
            ),
            CompileError::InvalidChain(
                "request_withdrawal requires at least one amount".into(),
            ),
            CompileError::InvalidChain(
                "claim_withdrawal requires at least one request_id".into(),
            ),
            CompileError::InvalidChain(
                "claim_withdrawal hints length 1 does not match request_ids length 2".into(),
            ),
            CompileError::InvalidChain(
                "LP mint/increase requires a non-zero amount on at least one side".into(),
            ),
            CompileError::InvalidChain(
                "LP decrease liquidity must be greater than zero".into(),
            ),
            CompileError::InvalidChain(
                "LP decrease requires slippage protection: min_amount0 or min_amount1 must be > 0".into(),
            ),
            CompileError::InvalidChain(
                "lp_mint token addresses must resolve to ERC-20 contracts. Use WETH instead of native ETH.".into(),
            ),
            CompileError::InvalidChain(
                "LP ticks (-513, 487) must be multiples of tick spacing 10 for this fee tier".into(),
            ),
            CompileError::Adapter("free-form".into()),
            CompileError::AdapterStepMismatch {
                adapter: "aave_v3",
                expected: "AaveV3Supply",
            },
            CompileError::ProtocolContractMissing {
                protocol: "morpho".into(),
                contract: "pool".into(),
            },
            CompileError::MorphoMarketRequired,
            CompileError::UniswapFeeTierUnknown {
                fee: "1234".into(),
            },
            CompileError::Json("unknown field `oops` at line 1 column 1".into()),
            CompileError::Json("missing field `network` at line 1 column 1".into()),
            CompileError::Json(
                "invalid type: integer `1`, expected a string at line 1 column 1".into(),
            ),
            CompileError::Json("expected colon at line 1 column 1".into()),
            CompileError::DeadlineMissing,
            CompileError::DeadlineInPast {
                deadline: 100,
                current_timestamp: 200,
            },
        ];

        for err in &samples {
            let s = err.to_structured();
            assert!(
                ALL_COMPILE_ERROR_CODES.contains(&s.code),
                "code `{}` (from {err:?}) is not in ALL_COMPILE_ERROR_CODES; add it to the \
                 registry and to the system-prompt reference table",
                s.code,
            );
            assert!(
                !s.fix_instruction.is_empty(),
                "code `{}` has an empty fix_instruction; the LLM has nothing actionable",
                s.code,
            );
            assert!(
                !s.hint.is_empty(),
                "code `{}` has an empty hint; the LLM gets no rationale",
                s.code,
            );
        }
    }

    #[test]
    fn classify_flashloan_balance_drift_extracts_fields() {
        let err = CompileError::InvalidChain(
            "flashloan inner step consumes 30000000000 of token 0xabc but only 20000000000 \
             is available (flashloaned + produced)"
                .into(),
        );
        let s = err.to_structured();
        assert_eq!(s.code, "flashloan_balance_drift");
        assert_eq!(s.fields.get("token").map(|x| x.as_str()), Some("0xabc"));
        assert_eq!(
            s.fields.get("consumed").map(|x| x.as_str()),
            Some("30000000000")
        );
        assert_eq!(
            s.fields.get("available").map(|x| x.as_str()),
            Some("20000000000")
        );
        assert!(s.fix_instruction.contains("0xabc"));
    }

    #[test]
    fn classify_lp_decrease_zero_is_lp_invalid_not_zero_amount() {
        // Regression: "LP decrease liquidity must be greater than zero"
        // matches both `lp_*` and the generic `zero_amount` rule. The LP
        // branch must come first so the LLM gets the more-specific code.
        let err = CompileError::InvalidChain(
            "LP decrease liquidity must be greater than zero".into(),
        );
        let s = err.to_structured();
        assert_eq!(s.code, "lp_invalid");
    }

    #[test]
    fn classify_claim_withdrawal_is_not_running_balance() {
        // Regression: "claim_withdrawal requires at least one request_id"
        // contains "requires" which used to misclassify as
        // insufficient_running_balance.
        let err = CompileError::InvalidChain(
            "claim_withdrawal requires at least one request_id".into(),
        );
        let s = err.to_structured();
        assert_eq!(s.code, "withdrawal_claim_invalid");
    }

    #[test]
    fn classify_lp_tick_misaligned_extracts_spacing() {
        let err = CompileError::InvalidChain(
            "LP ticks (-513, 487) must be multiples of tick spacing 10 for this fee tier"
                .into(),
        );
        let s = err.to_structured();
        assert_eq!(s.code, "lp_tick_misaligned");
        assert_eq!(s.fields.get("tick_spacing").map(|x| x.as_str()), Some("10"));
    }

    #[test]
    fn classify_uniswap_fee_tier_unknown_lists_alternatives() {
        let err = CompileError::UniswapFeeTierUnknown {
            fee: "1234".into(),
        };
        let s = err.to_structured();
        assert_eq!(s.code, "uniswap_fee_tier_unknown");
        assert!(s.available.iter().any(|v| v == "500"));
        assert!(s.available.iter().any(|v| v == "3000"));
    }
}
