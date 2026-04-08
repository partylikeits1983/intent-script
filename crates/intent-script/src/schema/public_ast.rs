//! Public AST types — the serde representation of the LLM-facing JSON schema.
//!
//! These types are intentionally simple and string-based. The compiler
//! normalizes them into the canonical IR with resolved addresses and amounts.

use serde::Deserialize;

/// Top-level intent script document.
#[derive(Debug, Deserialize)]
pub struct IntentScript {
    /// Network alias, e.g. "ethereum", "base", "arbitrum"
    pub network: String,
    /// Signer EOA address as hex string
    pub from: String,
    /// Ordered list of action steps
    pub steps: Vec<Step>,
    /// Optional nonce for EIP-712 replay protection (default: 0)
    #[serde(default)]
    pub nonce: Option<u64>,
    /// Optional deadline timestamp for EIP-712 expiry (default: 0 = no expiry)
    #[serde(default)]
    pub deadline: Option<u64>,
}

/// A single action step. Each variant wraps a minimal payload.
///
/// The JSON uses the action name as the key:
/// ```json
/// { "wrap": { "asset": "ETH", "amount": "1.5" } }
/// ```
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Swap(SwapStep),
    Deposit(DepositStep),
    Borrow(BorrowStep),
    Withdraw(WithdrawStep),
    Wrap(WrapStep),
    Unwrap(UnwrapStep),
    Stake(StakeStep),
    Custom(serde_json::Value),
}

#[derive(Debug, Deserialize)]
pub struct SwapStep {
    pub from: String,
    pub amount: String,
    pub to: String,
    /// Optional fee tier for Uniswap V3 (default: 3000 = 0.3%)
    #[serde(default)]
    pub fee: Option<String>,
    /// Optional routing provider: "uniswap" (default), "1inch"
    #[serde(default)]
    pub via: Option<String>,
    /// Pre-fetched calldata for aggregator swaps (required when via = "1inch")
    #[serde(default)]
    pub calldata: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DepositStep {
    pub asset: String,
    pub amount: String,
    pub into: String,
}

#[derive(Debug, Deserialize)]
pub struct BorrowStep {
    pub asset: String,
    pub amount: String,
    pub from: String,
}

#[derive(Debug, Deserialize)]
pub struct WithdrawStep {
    pub asset: String,
    pub amount: String,
    pub from: String,
}

#[derive(Debug, Deserialize)]
pub struct WrapStep {
    pub asset: String,
    pub amount: String,
}

#[derive(Debug, Deserialize)]
pub struct UnwrapStep {
    pub asset: String,
    pub amount: String,
}

#[derive(Debug, Deserialize)]
pub struct StakeStep {
    pub asset: String,
    pub amount: String,
    pub into: String,
}
