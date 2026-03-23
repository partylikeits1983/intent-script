//! Canonical IR — the resolved, typed internal representation.
//!
//! All aliases have been resolved to addresses, all amounts to U256,
//! and all protocol references to concrete deployment addresses.

use alloy_primitives::{Address, Bytes, U256};

/// Fully resolved intent, ready for enrichment and lowering.
#[derive(Debug, Clone)]
pub struct ResolvedIntent {
    pub chain_id: u64,
    pub signer: Address,
    pub steps: Vec<ResolvedStep>,
    /// ERC-20 tokens that should be swept back to the signer after batched execution.
    /// Populated by the enricher when a router is available.
    pub tokens_to_sweep: Vec<Address>,
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
