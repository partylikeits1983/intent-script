//! Public AST types — the serde representation of the LLM-facing JSON schema.
//!
//! These types are intentionally simple and string-based. The compiler
//! normalizes them into the canonical IR with resolved addresses and amounts.

use std::collections::HashMap;

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
    /// Optional user balance information for enhanced validation.
    /// When provided, the compiler can check feasibility and produce better warnings.
    #[serde(default)]
    pub balances: Option<UserBalances>,
}

/// User's on-chain balance information, provided optionally by the frontend.
#[derive(Debug, Deserialize, Default)]
pub struct UserBalances {
    /// Token alias → human-readable balance (e.g. "USDC" → "10000.0")
    #[serde(default)]
    pub tokens: HashMap<String, String>,
    /// Aave V3 position information
    #[serde(default)]
    pub aave_positions: Option<AavePositions>,
}

/// Aave V3 position information for balance-aware validation.
#[derive(Debug, Deserialize, Default)]
pub struct AavePositions {
    /// Token alias → supplied amount (e.g. "USDC" → "50000.0")
    #[serde(default)]
    pub supplied: HashMap<String, String>,
    /// Token alias → borrowed amount (e.g. "DAI" → "5000.0")
    #[serde(default)]
    pub borrowed: HashMap<String, String>,
    /// Current health factor as string (e.g. "1.85")
    #[serde(default)]
    pub health_factor: Option<String>,
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
