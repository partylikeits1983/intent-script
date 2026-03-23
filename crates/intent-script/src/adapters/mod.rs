pub mod aave_v3;
pub mod erc20;
pub mod lido;
pub mod uniswap_v3;
pub mod wrap;

use crate::error::Result;
use crate::ir::{ConcreteCall, ResolvedStep};
use crate::registry::RegistryContext;

/// Lower a resolved step into concrete EVM calls using the appropriate adapter.
pub fn lower_step(step: &ResolvedStep, _registry: &RegistryContext) -> Result<Vec<ConcreteCall>> {
    match step {
        ResolvedStep::Wrap { .. } => wrap::lower_wrap(step),
        ResolvedStep::Unwrap { .. } => wrap::lower_unwrap(step),
        ResolvedStep::Erc20Approve { .. } => erc20::lower_approve(step),
        ResolvedStep::AaveV3Supply { .. } => aave_v3::lower_supply(step),
        ResolvedStep::AaveV3Borrow { .. } => aave_v3::lower_borrow(step),
        ResolvedStep::AaveV3Withdraw { .. } => aave_v3::lower_withdraw(step),
        ResolvedStep::UniswapV3Swap { .. } => uniswap_v3::lower_swap(step),
        ResolvedStep::LidoStake { .. } => lido::lower_stake(step),
    }
}
