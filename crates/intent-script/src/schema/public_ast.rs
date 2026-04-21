//! Public AST types — the serde representation of the LLM-facing JSON schema.
//!
//! These types are intentionally simple and string-based. The compiler
//! normalizes them into the canonical IR with resolved addresses and amounts.

use alloc::string::String;
use alloc::vec::Vec;

use hashbrown::HashMap;
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
    /// Current Unix timestamp in seconds. Required for deadline computation.
    /// The caller (CLI/frontend) provides this.
    #[serde(default)]
    pub current_timestamp: Option<u64>,
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

/// User's current ERC-20 allowances against the IntentRouter.
///
/// Assembled by the UI (never by the LLM) from a multicall of
/// `allowance(user, router)` across its top-tokens list and passed into the
/// compiler as a *separate* JSON argument (not a field on `IntentScript`).
/// When present, the compiler emits a prepended `approve(router, amount)`
/// UnsignedTx for any ERC-20 the user is spending whose current allowance is
/// below the aggregate amount pulled into the router.
#[derive(Debug, Deserialize, Default)]
pub struct AllowancesInput {
    /// Token alias → current allowance in base units as a decimal string
    /// (e.g. "USDT" → "0" or "USDC" → "115792...639935"). Missing keys are
    /// treated as 0 (no allowance). The spender is implicit — there is one
    /// router per chain, configured in the protocol registry.
    #[serde(default)]
    pub tokens: HashMap<String, String>,
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
    Send(SendStep),
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
    /// Explicit minimum output amount in human-readable output-token units.
    /// Takes precedence over slippage+price calculation.
    /// Example: "0.48" means at least 0.48 WETH out.
    #[serde(default)]
    pub min_amount_out: Option<String>,
    /// Current market price: output tokens per 1 input token.
    /// Required when slippage is specified without min_amount_out.
    /// Example: "0.0005" means 1 USDC → 0.0005 WETH.
    #[serde(default)]
    pub price: Option<String>,
    /// Max slippage tolerance as percentage (e.g., "0.5" = 0.5%).
    /// Default: 0.5% when price is provided but slippage is omitted.
    /// Requires the price field to be set.
    #[serde(default)]
    pub slippage: Option<String>,
    /// Optional swap-specific deadline as Unix timestamp.
    #[serde(default)]
    pub deadline: Option<u64>,
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

#[derive(Debug, Deserialize)]
pub struct SendStep {
    /// ERC20/ETH token alias (e.g. "USDC", "ETH")
    #[serde(default)]
    pub asset: Option<String>,
    /// Human-readable amount (e.g. "100.0")
    #[serde(default)]
    pub amount: Option<String>,
    /// Recipient address
    pub to: String,
    /// Asset type: "erc20" (default), "erc721"
    #[serde(default)]
    pub asset_type: Option<String>,
    /// NFT contract address (erc721 only)
    #[serde(default)]
    pub contract: Option<String>,
    /// NFT token ID (erc721 only)
    #[serde(default)]
    pub token_id: Option<String>,
}
