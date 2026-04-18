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
    /// Uniswap V3 exactInputSingle swap
    UniswapV3Swap {
        router: Address,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee: u32,
        recipient: Address,
        deadline: U256,
        amount_out_minimum: U256,
    },
    /// Lido stETH staking via submit()
    LidoStake {
        lido: Address,
        amount: U256,
        referral: Address,
    },
    /// Wrap stETH → wstETH via wstETH.wrap(uint256)
    WstETHWrap {
        wsteth: Address,
        steth: Address,
        amount: U256,
    },
    /// 1inch swap with pre-fetched calldata passthrough
    OneInchSwap {
        router: Address,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        calldata: Bytes,
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
}

/// Helper: determine what token and amount a step consumes (if any).
///
/// Only returns consumption for user-facing steps (not auto-generated ones).
/// Used by cross-step amount flow validation and "all" amount resolution.
pub fn step_consumes(step: &ResolvedStep) -> Option<(Address, U256)> {
    match step {
        ResolvedStep::AaveV3Supply { asset, amount, .. } => Some((*asset, *amount)),
        ResolvedStep::WstETHWrap { steth, amount, .. } => Some((*steth, *amount)),
        ResolvedStep::UniswapV3Swap {
            token_in,
            amount_in,
            ..
        } => Some((*token_in, *amount_in)),
        ResolvedStep::OneInchSwap {
            token_in,
            amount_in,
            ..
        } => Some((*token_in, *amount_in)),
        ResolvedStep::Unwrap {
            wrapped_token,
            amount,
        } => Some((*wrapped_token, *amount)),
        ResolvedStep::SendErc20 { token, amount, .. } => Some((*token, *amount)),
        _ => None,
    }
}

/// Helper: determine what token and guaranteed amount a step produces (if any).
///
/// Used by cross-step amount flow validation and "all" amount resolution.
pub fn step_produces(step: &ResolvedStep) -> Option<(Address, U256)> {
    match step {
        ResolvedStep::UniswapV3Swap {
            token_out,
            amount_out_minimum,
            ..
        } => Some((*token_out, *amount_out_minimum)),
        // 1inch calldata is opaque — we use amount_in as a conservative lower bound
        // on production. The router's sweep will still return whatever arrives.
        // Downstream `"all"` consumers must not over-consume.
        ResolvedStep::OneInchSwap {
            token_out,
            amount_in,
            ..
        } => Some((*token_out, *amount_in)),
        ResolvedStep::AaveV3Borrow { asset, amount, .. } => Some((*asset, *amount)),
        ResolvedStep::AaveV3Withdraw { asset, amount, .. } => Some((*asset, *amount)),
        ResolvedStep::LidoStake { lido, amount, .. } => Some((*lido, *amount)),
        ResolvedStep::Wrap {
            wrapped_token,
            amount,
            ..
        } => Some((*wrapped_token, *amount)),
        ResolvedStep::WstETHWrap { wsteth, amount, .. } => Some((*wsteth, *amount)),
        _ => None,
    }
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
