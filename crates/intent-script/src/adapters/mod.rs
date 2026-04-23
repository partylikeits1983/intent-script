pub mod aave_v3;
pub mod across;
pub mod balancer;
pub mod erc20;
pub mod lido;
pub mod morpho;
pub mod oneinch;
pub mod send;
pub mod uniswap_v3;
pub mod uniswap_v3_lp;
pub mod wrap;

use crate::error::Result;
use crate::ir::{ConcreteCall, ResolvedStep};
use crate::registry::RegistryContext;

/// Lower a resolved step into concrete EVM calls using the appropriate adapter.
pub fn lower_step(step: &ResolvedStep, registry: &RegistryContext) -> Result<Vec<ConcreteCall>> {
    match step {
        ResolvedStep::Wrap { .. } => wrap::lower_wrap(step),
        ResolvedStep::Unwrap { .. } => wrap::lower_unwrap(step),
        ResolvedStep::Erc20Approve { .. } => erc20::lower_approve(step),
        ResolvedStep::Erc20TransferFrom { .. } => erc20::lower_transfer_from(step),
        ResolvedStep::AaveV3Supply { .. } => aave_v3::lower_supply(step),
        ResolvedStep::AaveV3Borrow { .. } => aave_v3::lower_borrow(step),
        ResolvedStep::AaveV3Withdraw { .. } => aave_v3::lower_withdraw(step),
        ResolvedStep::AaveV3Repay { .. } => aave_v3::lower_repay(step),
        ResolvedStep::MorphoSupply { .. } => morpho::lower_supply(step),
        ResolvedStep::MorphoSupplyCollat { .. } => morpho::lower_supply_collateral(step),
        ResolvedStep::MorphoBorrow { .. } => morpho::lower_borrow(step),
        ResolvedStep::MorphoWithdraw { .. } => morpho::lower_withdraw(step),
        ResolvedStep::MorphoWithdrawCollat { .. } => morpho::lower_withdraw_collateral(step),
        ResolvedStep::MorphoRepay { .. } => morpho::lower_repay(step),
        ResolvedStep::UniswapV3Swap { .. } => uniswap_v3::lower_swap(step),
        ResolvedStep::LidoStake { .. } => lido::lower_stake(step),
        ResolvedStep::WstETHWrap { .. } => lido::lower_wsteth_wrap(step),
        ResolvedStep::WstETHUnwrap { .. } => lido::lower_wsteth_unwrap(step),
        ResolvedStep::LidoRequestWithdrawal { .. } => lido::lower_request_withdrawal(step),
        ResolvedStep::LidoClaimWithdrawal { .. } => lido::lower_claim_withdrawal(step),
        ResolvedStep::UniswapV3LpMint { .. } => uniswap_v3_lp::lower_lp_mint(step),
        ResolvedStep::UniswapV3LpIncrease { .. } => uniswap_v3_lp::lower_lp_increase(step),
        ResolvedStep::UniswapV3LpDecrease { .. } => uniswap_v3_lp::lower_lp_decrease(step),
        ResolvedStep::UniswapV3LpCollect { .. } => uniswap_v3_lp::lower_lp_collect(step),
        ResolvedStep::OneInchSwap { .. } => oneinch::lower_oneinch_swap(step),
        ResolvedStep::Erc20Permit { .. } => erc20::lower_permit(step),
        ResolvedStep::SendErc20 { .. } => send::lower_send_erc20(step),
        ResolvedStep::SendEth { .. } => send::lower_send_eth(step),
        ResolvedStep::SendErc721 { .. } => send::lower_send_erc721(step),
        ResolvedStep::BalancerFlashloan { .. } => balancer::lower_flashloan(step, registry),
        ResolvedStep::AcrossDepositV3 { .. } => across::lower_deposit_v3(step),
    }
}
