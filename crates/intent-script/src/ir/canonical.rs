//! Canonical IR — the resolved, typed internal representation.
//!
//! All aliases have been resolved to addresses, all amounts to U256,
//! and all protocol references to concrete deployment addresses.

use alloc::string::String;
use alloc::vec::Vec;

use alloy_primitives::{Address, Bytes, U256};
use hashbrown::HashMap;

/// Fully resolved intent, ready for enrichment and lowering.
#[derive(Debug, Clone)]
pub struct ResolvedIntent {
    pub chain_id: u64,
    pub signer: Address,
    pub steps: Vec<ResolvedStep>,
    /// ERC-20 tokens that should be swept back to the signer after batched execution.
    /// Populated by the enricher when a router is available.
    pub tokens_to_sweep: Vec<Address>,
    /// Nonce for EIP-712 replay protection.
    pub nonce: u64,
    /// Deadline timestamp for EIP-712 expiry (0 = no expiry).
    pub deadline: u64,
    /// Optional user balance information for enhanced validation.
    pub user_balances: Option<ResolvedBalances>,
    /// Aggregate ERC-20 amounts that will be pulled from the signer into the
    /// router during batched execution, keyed by token address. Populated by
    /// the enricher each time it emits a `Erc20TransferFrom { from: signer, ... }`
    /// pulling user-held tokens into the router. Used by the builder to decide
    /// which `approve(router, amount)` prerequisite txs to emit when the caller
    /// supplied `current_allowances`. Order is stable (sorted by address).
    pub required_pulls: Vec<(Address, U256)>,
    /// Router fee in basis points applied at sweep/refund time on-chain.
    /// Populated from `registry.fee_bps()` during normalize so that
    /// `step_produces` can return post-fee floors for downstream `"all"`
    /// consumers.
    pub fee_bps: u16,
    /// Forces the planner to emit a Batched (router-executed) output even
    /// when the enriched pipeline lowers to a single concrete call. Set by
    /// normalize when a step needs router context — e.g. Balancer flashloans,
    /// whose `receiveFlashLoan` callback requires the router's transient
    /// sentinel to have been armed by `_executeCalls`.
    pub requires_router: bool,
}

/// Fully resolved Morpho Blue market parameters ready for calldata encoding.
///
/// The market id (`keccak256(abi.encode(MarketParams))`) is stored alongside
/// the constituent fields so adapters can either reconstruct the struct for
/// `supply/borrow/…` calls or reference the id directly when Morpho's ABI
/// takes the id instead of the struct.
#[derive(Debug, Clone)]
pub struct MorphoMarketParams {
    pub loan_token: Address,
    pub collateral_token: Address,
    pub oracle: Address,
    pub irm: Address,
    pub lltv: U256,
    pub id: [u8; 32],
}

/// Resolved user balance information with concrete addresses and U256 amounts.
#[derive(Debug, Clone, Default)]
pub struct ResolvedBalances {
    /// Token address → balance in smallest unit (wei, etc.)
    pub tokens: HashMap<Address, U256>,
    /// Aave V3 supplied: token address → supplied amount in smallest unit
    pub aave_supplied: HashMap<Address, U256>,
    /// Aave V3 borrowed: token address → borrowed amount in smallest unit
    pub aave_borrowed: HashMap<Address, U256>,
    /// Aave V3 health factor (parsed from string)
    pub aave_health_factor: Option<f64>,
}

/// A resolved action step with concrete types.
#[derive(Debug, Clone)]
pub enum ResolvedStep {
    /// Wrap native asset (e.g. ETH → WETH) via WETH.deposit()
    Wrap {
        wrapped_token: Address,
        amount: U256,
    },
    /// Unwrap wrapped native (e.g. WETH → ETH) via WETH.withdraw()
    Unwrap {
        wrapped_token: Address,
        amount: U256,
    },
    /// ERC-20 approve (auto-inserted by enricher)
    Erc20Approve {
        token: Address,
        spender: Address,
        amount: U256,
    },
    /// ERC-20 transferFrom (auto-inserted by enricher for router batching)
    Erc20TransferFrom {
        token: Address,
        from: Address,
        to: Address,
        amount: U256,
    },
    /// Aave V3 supply
    AaveV3Supply {
        pool: Address,
        asset: Address,
        amount: U256,
        on_behalf_of: Address,
    },
    /// Aave V3 borrow
    AaveV3Borrow {
        pool: Address,
        asset: Address,
        amount: U256,
        rate_mode: u8,
        on_behalf_of: Address,
    },
    /// Aave V3 withdraw
    AaveV3Withdraw {
        pool: Address,
        asset: Address,
        amount: U256,
        to: Address,
    },
    /// Aave V3 repay — pay back borrowed assets.
    AaveV3Repay {
        pool: Address,
        asset: Address,
        amount: U256,
        rate_mode: u8,
        on_behalf_of: Address,
    },
    /// Morpho Blue: supply loan asset (earns interest).
    MorphoSupply {
        pool: Address,
        market_params: MorphoMarketParams,
        amount: U256,
        on_behalf: Address,
    },
    /// Morpho Blue: supply collateral (no interest; enables borrowing).
    MorphoSupplyCollat {
        pool: Address,
        market_params: MorphoMarketParams,
        amount: U256,
        on_behalf: Address,
    },
    /// Morpho Blue: borrow loan asset against collateral.
    MorphoBorrow {
        pool: Address,
        market_params: MorphoMarketParams,
        amount: U256,
        on_behalf: Address,
        receiver: Address,
    },
    /// Morpho Blue: withdraw supplied loan asset.
    MorphoWithdraw {
        pool: Address,
        market_params: MorphoMarketParams,
        amount: U256,
        on_behalf: Address,
        receiver: Address,
    },
    /// Morpho Blue: withdraw collateral (only when no borrow held or within HF).
    MorphoWithdrawCollat {
        pool: Address,
        market_params: MorphoMarketParams,
        amount: U256,
        on_behalf: Address,
        receiver: Address,
    },
    /// Morpho Blue: repay borrowed loan asset.
    MorphoRepay {
        pool: Address,
        market_params: MorphoMarketParams,
        amount: U256,
        on_behalf: Address,
    },
    /// Uniswap V3 exactInputSingle swap.
    ///
    /// `native_input` is true when the user's intent specified the chain's
    /// native asset (e.g. "ETH") as the input: `token_in` still holds the
    /// wrapped-native address because that's what the SwapRouter expects in
    /// the calldata, but the call must carry `amount_in` as msg.value so the
    /// router auto-wraps via its internal `pay()` path. Downstream code must
    /// NOT insert an ERC-20 transferFrom/approve for a native-input swap.
    UniswapV3Swap {
        router: Address,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee: u32,
        recipient: Address,
        deadline: U256,
        amount_out_minimum: U256,
        native_input: bool,
    },
    /// Lido stETH staking via submit()
    ///
    /// `steth` is the stETH token contract address — Lido's `submit()` is
    /// called directly on the stETH contract (it mints stETH 1:1 with ETH in).
    LidoStake {
        steth: Address,
        amount: U256,
        referral: Address,
    },
    /// Wrap stETH → wstETH via wstETH.wrap(uint256)
    WstETHWrap {
        wsteth: Address,
        steth: Address,
        amount: U256,
    },
    /// Unwrap wstETH → stETH via wstETH.unwrap(uint256)
    WstETHUnwrap {
        wsteth: Address,
        steth: Address,
        amount: U256,
    },
    /// Lido withdrawal queue: request one or more NFTs redeemable later for ETH.
    ///
    /// `token` is either stETH or wstETH (distinguished by `is_wsteth`), and
    /// `owner` — the NFT recipient — is always the signer in v1, so the router
    /// never custodies the withdrawal NFTs.
    LidoRequestWithdrawal {
        queue: Address,
        token: Address,
        is_wsteth: bool,
        amounts: Vec<U256>,
        owner: Address,
    },
    /// Lido withdrawal queue: claim ETH for previously-minted withdrawal NFTs.
    ///
    /// `hints` must be the same length as `request_ids` and are the caller-
    /// supplied `findCheckpointHints(...)` results from the queue.
    LidoClaimWithdrawal {
        queue: Address,
        request_ids: Vec<U256>,
        hints: Vec<U256>,
    },
    /// Uniswap V3 NonfungiblePositionManager: mint a new LP position NFT.
    ///
    /// `token0` must be the lexicographically-smaller address of the pair
    /// (NPM invariant). The `recipient` is the signer so the NFT bypasses
    /// the router; dust-refunded amounts from slippage land in the router
    /// and must be swept.
    UniswapV3LpMint {
        npm: Address,
        token0: Address,
        token1: Address,
        fee: u32,
        tick_lower: i32,
        tick_upper: i32,
        amount0: U256,
        amount1: U256,
        amount0_min: U256,
        amount1_min: U256,
        recipient: Address,
        deadline: U256,
    },
    /// Uniswap V3: add liquidity to an existing position NFT.
    UniswapV3LpIncrease {
        npm: Address,
        token0: Address,
        token1: Address,
        token_id: U256,
        amount0: U256,
        amount1: U256,
        amount0_min: U256,
        amount1_min: U256,
        deadline: U256,
    },
    /// Uniswap V3: remove liquidity from an existing position.
    ///
    /// Decrease alone leaves the withdrawn tokens as uncollected fees inside
    /// the position; a subsequent `UniswapV3LpCollect` is required to sweep
    /// them to the router / signer.
    UniswapV3LpDecrease {
        npm: Address,
        token_id: U256,
        liquidity: u128,
        amount0_min: U256,
        amount1_min: U256,
        deadline: U256,
    },
    /// Uniswap V3: collect tokens (fees + any freshly-decreased liquidity)
    /// from a position NFT to `recipient` (= router under batching).
    UniswapV3LpCollect {
        npm: Address,
        token0: Address,
        token1: Address,
        token_id: U256,
        recipient: Address,
        amount0_max: u128,
        amount1_max: u128,
    },
    /// ERC-20 permit (v, r, s are placeholder zeros — frontend fills after signing)
    Erc20Permit {
        token: Address,
        owner: Address,
        spender: Address,
        value: U256,
        deadline: U256,
    },
    /// Send ERC-20 tokens to a recipient
    SendErc20 {
        token: Address,
        to: Address,
        amount: U256,
    },
    /// Send native ETH to a recipient
    SendEth { to: Address, amount: U256 },
    /// Send ERC-721 NFT to a recipient
    SendErc721 {
        contract: Address,
        from: Address,
        to: Address,
        token_id: U256,
    },
    /// Across V3 `depositV3` — single-sided cross-chain transfer.
    /// `step_produces` returns `None` because produced tokens land on the
    /// destination chain, not the router.
    AcrossDepositV3 {
        spoke_pool: Address,
        depositor: Address,
        recipient: Address,
        input_token: Address,
        output_token: Address,
        input_amount: U256,
        output_amount: U256,
        destination_chain_id: U256,
        exclusive_relayer: Address,
        quote_timestamp: u32,
        fill_deadline: u32,
        exclusivity_deadline: u32,
        message: Bytes,
    },
    /// Balancer V2 flashloan wrapping an inner pipeline.
    ///
    /// The inner pipeline is stored as `Vec<ResolvedStep>` (not lowered yet)
    /// so the enricher can walk it recursively with a seeded context and
    /// auto-insert approvals/transferFroms as needed. Lowering happens last:
    /// the whole inner tree is rendered to `ConcreteCall[]`, ABI-encoded as
    /// `userData`, and passed to `vault.flashLoan(recipient=router,…)`.
    BalancerFlashloan {
        vault: Address,
        tokens: Vec<Address>,
        amounts: Vec<U256>,
        inner_steps: Vec<ResolvedStep>,
    },
}

/// Helper: determine what token and amount a step consumes (if any).
///
/// Only returns consumption for user-facing steps (not auto-generated ones).
/// Used by cross-step amount flow validation and "all" amount resolution.
pub fn step_consumes(step: &ResolvedStep) -> Option<(Address, U256)> {
    match step {
        ResolvedStep::AaveV3Supply { asset, amount, .. } => Some((*asset, *amount)),
        ResolvedStep::AaveV3Repay { asset, amount, .. } => Some((*asset, *amount)),
        ResolvedStep::MorphoSupply {
            market_params,
            amount,
            ..
        } => Some((market_params.loan_token, *amount)),
        ResolvedStep::MorphoSupplyCollat {
            market_params,
            amount,
            ..
        } => Some((market_params.collateral_token, *amount)),
        ResolvedStep::MorphoRepay {
            market_params,
            amount,
            ..
        } => Some((market_params.loan_token, *amount)),
        // Wrap (ETH→WETH) consumes native ETH; the produced WETH is surfaced
        // via `step_produces`. Without this case, the preview builder treats
        // wrap as a net WETH inflow and then mis-aggregates wrap+deposit flows
        // as a tiny WETH *input* (fee_bps mismatch on the intermediate).
        ResolvedStep::Wrap { amount, .. } => Some((Address::ZERO, *amount)),
        // LidoStake consumes native ETH and produces stETH — same rationale.
        ResolvedStep::LidoStake { amount, .. } => Some((Address::ZERO, *amount)),
        ResolvedStep::WstETHWrap { steth, amount, .. } => Some((*steth, *amount)),
        ResolvedStep::WstETHUnwrap { wsteth, amount, .. } => Some((*wsteth, *amount)),
        ResolvedStep::LidoRequestWithdrawal { token, amounts, .. } => {
            let total = amounts
                .iter()
                .copied()
                .fold(U256::ZERO, |acc, a| acc.saturating_add(a));
            Some((*token, total))
        }
        ResolvedStep::UniswapV3Swap {
            token_in,
            amount_in,
            native_input,
            ..
        } => {
            // Native-input swaps spend the chain's native asset, not WETH.
            // Surface that to the preview/flow code so users see "ETH in".
            let consumed = if *native_input {
                Address::ZERO
            } else {
                *token_in
            };
            Some((consumed, *amount_in))
        }
        ResolvedStep::Unwrap {
            wrapped_token,
            amount,
        } => Some((*wrapped_token, *amount)),
        ResolvedStep::SendErc20 { token, amount, .. } => Some((*token, *amount)),
        ResolvedStep::AcrossDepositV3 {
            input_token,
            input_amount,
            ..
        } => Some((*input_token, *input_amount)),
        _ => None,
    }
}

/// Helper: determine what token and guaranteed amount a step produces (if any).
///
/// Used by cross-step amount flow validation and "all" amount resolution.
///
/// `fee_bps` is the router's sweep-time skim in basis points. The returned
/// amount is reduced to `raw * (10_000 - fee_bps) / 10_000` so downstream
/// `"all"` consumers see the floor that will actually be in the router after
/// sweep. Pass `0` when the produced tokens do NOT flow through sweep
/// (e.g. inside a flashloan's inner pipeline where tokens are transferred
/// back to the flashloan provider, not swept to the user).
pub fn step_produces(step: &ResolvedStep, fee_bps: u16) -> Option<(Address, U256)> {
    let (token, amount) = match step {
        // A transferFrom into the router brings tokens INTO the router's
        // balance sheet — important for flashloan repayability accounting
        // where the leverage-sugar expander emits an explicit transferFrom
        // as the first inner step to represent the user's equity contribution.
        // Auto-inserted transferFroms don't appear in pre-enrich IR, so this
        // doesn't perturb the normal "all" / amount-flow paths.
        ResolvedStep::Erc20TransferFrom { token, amount, .. } => (*token, *amount),
        ResolvedStep::UniswapV3Swap {
            token_out,
            amount_out_minimum,
            ..
        } => (*token_out, *amount_out_minimum),
        ResolvedStep::AaveV3Borrow { asset, amount, .. } => (*asset, *amount),
        ResolvedStep::AaveV3Withdraw { asset, amount, .. } => (*asset, *amount),
        ResolvedStep::MorphoBorrow {
            market_params,
            amount,
            ..
        } => (market_params.loan_token, *amount),
        ResolvedStep::MorphoWithdraw {
            market_params,
            amount,
            ..
        } => (market_params.loan_token, *amount),
        ResolvedStep::MorphoWithdrawCollat {
            market_params,
            amount,
            ..
        } => (market_params.collateral_token, *amount),
        ResolvedStep::LidoStake { steth, amount, .. } => (*steth, *amount),
        ResolvedStep::Wrap {
            wrapped_token,
            amount,
            ..
        } => (*wrapped_token, *amount),
        ResolvedStep::WstETHWrap { wsteth, amount, .. } => (*wsteth, *amount),
        ResolvedStep::WstETHUnwrap { steth, amount, .. } => (*steth, *amount),
        _ => return None,
    };

    debug_assert!(fee_bps <= 10_000, "fee_bps must be <= 10_000");
    let reduced = amount * U256::from(10_000u64 - fee_bps as u64) / U256::from(10_000u64);
    Some((token, reduced))
}

/// A concrete EVM call produced by an adapter.
#[derive(Debug, Clone)]
pub struct ConcreteCall {
    /// Target contract address
    pub to: Address,
    /// ABI-encoded calldata
    pub calldata: Bytes,
    /// ETH value to send with the call
    pub value: U256,
    /// Human-readable description of what this call does
    pub description: String,
}
