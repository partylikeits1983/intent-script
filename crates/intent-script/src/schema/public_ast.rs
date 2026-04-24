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
#[serde(deny_unknown_fields)]
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
    /// Optional schema version. Compiler enforces `== "1.0"` when present;
    /// missing is permitted for backward compatibility but a future major
    /// release will make this required. Lets the UI pin the schema version
    /// it was built against so a divergent compiler rebuild doesn't
    /// silently reinterpret old intents.
    #[serde(default)]
    pub schema_version: Option<String>,
}

/// User's on-chain balance information, provided optionally by the frontend.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct AllowancesInput {
    /// Token alias → current allowance in base units as a decimal string
    /// (e.g. "USDT" → "0" or "USDC" → "115792...639935"). Missing keys are
    /// treated as 0 (no allowance). The spender is implicit — there is one
    /// router per chain, configured in the protocol registry.
    #[serde(default)]
    pub tokens: HashMap<String, String>,
    /// Optional per-token spend cap in base units as a decimal string.
    /// When set, the compiler rejects any intent whose aggregate
    /// required-pull for that token exceeds the cap. Lets the UI bound
    /// what a single session can consume regardless of what the LLM
    /// produces — the user pre-authorizes an envelope and the compiler
    /// refuses to emit anything outside it. Missing keys mean no cap.
    #[serde(default)]
    pub max_spend: HashMap<String, String>,
}

/// Aave V3 position information for balance-aware validation.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
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
    RequestWithdrawal(RequestWithdrawalStep),
    ClaimWithdrawal(ClaimWithdrawalStep),
    LpMint(LpMintStep),
    LpIncrease(LpIncreaseStep),
    LpDecrease(LpDecreaseStep),
    LpCollect(LpCollectStep),
    Send(SendStep),
    Flashloan(FlashloanStep),
    Long(LeverageStep),
    Short(LeverageStep),
    ClosePosition(ClosePositionStep),
    Bridge(BridgeStep),
    Custom(serde_json::Value),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwapStep {
    pub from: String,
    pub amount: String,
    pub to: String,
    /// Optional fee tier for Uniswap V3 (default: 3000 = 0.3%)
    #[serde(default)]
    pub fee: Option<String>,
    /// Optional routing provider: only "uniswap" is supported.
    /// Reserved for future multi-provider support; defaults to "uniswap".
    #[serde(default)]
    pub via: Option<String>,
    /// Deprecated. Pre-fetched calldata passthrough was removed with the
    /// 1inch adapter to close an untrusted-calldata path. Ignored.
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
#[serde(deny_unknown_fields)]
pub struct DepositStep {
    pub asset: String,
    pub amount: String,
    pub into: String,
    /// Market alias, required for protocols that key actions by market id
    /// (Morpho Blue). Absent for simple lending pools (Aave V3).
    #[serde(default)]
    pub market: Option<String>,
    /// Supply discriminator for markets with both loan and collateral sides.
    /// `"collateral"` supplies collateral (no interest), absent or `"loan"`
    /// supplies on the loan side (earns interest).
    #[serde(default, rename = "as")]
    pub r#as: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BorrowStep {
    pub asset: String,
    pub amount: String,
    pub from: String,
    /// Market alias (Morpho Blue only). Absent for Aave.
    #[serde(default)]
    pub market: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WithdrawStep {
    pub asset: String,
    pub amount: String,
    pub from: String,
    /// Market alias (Morpho Blue only). Absent for Aave.
    #[serde(default)]
    pub market: Option<String>,
    /// Withdraw discriminator: `"collateral"` withdraws from the collateral
    /// side of the market; absent or `"loan"` withdraws from the supply side.
    #[serde(default, rename = "as")]
    pub r#as: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WrapStep {
    pub asset: String,
    pub amount: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnwrapStep {
    pub asset: String,
    pub amount: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StakeStep {
    pub asset: String,
    pub amount: String,
    pub into: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestWithdrawalStep {
    /// "stETH" or "wstETH".
    pub asset: String,
    /// One amount per withdrawal NFT to mint (each bounded by protocol-side limits).
    pub amounts: Vec<String>,
    /// Protocol key; must be "lido".
    pub from: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimWithdrawalStep {
    /// Protocol key; must be "lido".
    pub protocol: String,
    /// NFT token ids returned by a prior `request_withdrawal`.
    pub request_ids: Vec<u64>,
    /// `findCheckpointHints(...)` results from the queue, one per request id.
    pub hints: Vec<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LpMintStep {
    /// Protocol key; must be "uniswap" in v1.
    pub protocol: String,
    pub token0: String,
    pub token1: String,
    /// Fee tier in hundredths of a bip: "500" | "3000" | "10000".
    pub fee: String,
    pub tick_lower: i32,
    pub tick_upper: i32,
    pub amount0: String,
    pub amount1: String,
    pub min_amount0: String,
    pub min_amount1: String,
    #[serde(default)]
    pub deadline: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LpIncreaseStep {
    /// NFT position token id as a decimal string (matches Solidity uint256).
    pub position_id: String,
    /// Pool's token0 alias (must match the position's actual token0).
    /// Required because the compiler has no RPC to introspect the NFT's
    /// metadata — the user must declare it so enrich can emit approvals
    /// for the correct ERC-20s.
    pub token0: String,
    pub token1: String,
    pub amount0: String,
    pub amount1: String,
    pub min_amount0: String,
    pub min_amount1: String,
    #[serde(default)]
    pub deadline: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LpDecreaseStep {
    pub position_id: String,
    /// Pool's token0 alias. Required so the compiler can parse
    /// `min_amount0` with the correct decimals.
    pub token0: String,
    pub token1: String,
    /// Liquidity amount to remove as a decimal string, or "all" for the
    /// full on-chain liquidity. Liquidity is a u128 in Uniswap V3.
    pub liquidity: String,
    pub min_amount0: String,
    pub min_amount1: String,
    #[serde(default)]
    pub deadline: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LpCollectStep {
    pub position_id: String,
    /// Pool's token0 alias (must match the position's actual token0).
    /// Required because the compiler has no RPC to introspect the NFT —
    /// enrich needs these to list the pair for sweep.
    pub token0: String,
    pub token1: String,
}

/// Flashloan step: borrow `assets` from a flashloan provider, run the `then`
/// pipeline (which must repay the flashloan), and return remaining balance to
/// the user. Only Balancer V2 is supported in v1 (`via: "balancer"`, 0% fee).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlashloanStep {
    /// Flashloan provider key; must be "balancer" in v1.
    pub via: String,
    /// Assets to borrow. Each entry specifies one token.
    pub assets: Vec<FlashloanAsset>,
    /// Inner pipeline to execute while the flashloaned funds are on this
    /// contract's balance. Bounded to 5 steps; nesting (flashloan inside
    /// flashloan) is rejected by validation.
    pub then: Vec<Step>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlashloanAsset {
    pub asset: String,
    pub amount: String,
}

/// Leverage sugar — `long` or `short`. Desugared during normalize into a
/// `Flashloan` (Balancer V2) wrapping Aave supply/borrow/swap. Both sides
/// share the same shape: `collateral` is what the user deposits into Aave;
/// `borrow` is what gets borrowed and swapped. A `long ETH` is `collateral=ETH,
/// borrow=USDC`; a `short ETH` is `collateral=USDC, borrow=ETH`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeverageStep {
    pub collateral: String,
    /// Borrow asset. Defaults: volatile collateral → "USDC"; stable → "WETH".
    #[serde(default)]
    pub borrow: Option<String>,
    pub amount: String,
    /// Target exposure multiplier. "1" = no leverage. Parsed as fixed-point
    /// with up to 4 fractional digits (e.g. "3.5" → 3.5x).
    pub leverage: String,
    /// Max swap slippage in bps. Default: "50" (= 0.5%). Capped at 500 (5%).
    #[serde(default)]
    pub slippage: Option<String>,
    /// Flashloan provider. Default: "balancer" (only v1-supported option).
    #[serde(default)]
    pub via: Option<String>,
    /// Current market price (borrow tokens per 1 collateral token) — required
    /// when `leverage > 1` until an on-chain quote primitive lands. Caller
    /// supplies from a price oracle or UI.
    #[serde(default)]
    pub price: Option<String>,
    /// Optional extra safety margin in bps subtracted from the effective max
    /// leverage cap. 0 by default (honor raw LTV floor).
    #[serde(default)]
    pub safety_margin_bps: Option<u16>,
    /// Uniswap V3 fee tier for the internal swap. Default "3000" (0.3%).
    /// Useful when the 0.3% pool has thin liquidity for the pair — e.g.
    /// USDC/wstETH typically routes better through the 0.05% or 0.01% tier.
    #[serde(default)]
    pub fee: Option<String>,
}

/// Bridge step — single-sided Across V3 deposit emitting only the source-chain
/// call (`depositV3` on the SpokePool). Receiving on the destination chain is
/// authored as a separate intent.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeStep {
    /// Bridge provider; must be "across" in v1.
    pub via: String,
    /// Asset alias (WETH, USDC, …). Native ETH is rejected — callers must
    /// pre-wrap to WETH.
    pub asset: String,
    pub amount: String,
    /// Destination chain alias from `chains.json` (e.g. "arbitrum").
    pub to_chain: String,
    /// Recipient address on the destination chain.
    pub recipient: String,
    /// Relayer fee in basis points — compiler caps at 50 (0.5%).
    pub relayer_fee_bps: String,
}

/// Close-position sugar — undoes a prior `long`/`short`. Requires the caller
/// to thread in the user's current Aave debt and collateral (read off-chain
/// via `getUserAccountData`) because the compiler has no RPC.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClosePositionStep {
    pub collateral: String,
    pub borrow: String,
    /// Current Aave debt in `borrow`-token units (human decimal string).
    pub current_debt: String,
    /// Current Aave collateral in `collateral`-token units.
    pub current_collateral: String,
    #[serde(default)]
    pub slippage: Option<String>,
    #[serde(default)]
    pub via: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
