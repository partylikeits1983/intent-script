//! Lido adapter.
//!
//! Generates calldata for:
//! - `lido.submit(address _referral)` — stake ETH and receive stETH
//! - `wstETH.wrap(uint256 _stETHAmount)` — wrap stETH into wstETH

use alloc::format;
use alloc::string::ToString;
use alloc::vec;

use alloy_primitives::{Bytes, U256};
use alloy_sol_types::SolCall;

use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, ResolvedStep};

alloy_sol_types::sol! {
    function submit(address _referral) external payable returns (uint256);
    function wrap(uint256 _stETHAmount) external returns (uint256);
}

/// Lower a LidoStake step to a concrete lido.submit() call.
pub fn lower_stake(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::LidoStake {
        lido,
        amount,
        referral,
    } = step
    else {
        return Err(CompileError::Adapter("Expected LidoStake step".to_string()));
    };

    let calldata = submitCall {
        _referral: *referral,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *lido,
        calldata: Bytes::from(calldata),
        value: *amount, // ETH sent as msg.value
        description: format!("Stake {} wei ETH in Lido for stETH", amount),
    }])
}

/// Lower a WstETHWrap step to a concrete wstETH.wrap() call.
pub fn lower_wsteth_wrap(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::WstETHWrap {
        wsteth,
        steth: _,
        amount,
    } = step
    else {
        return Err(CompileError::Adapter(
            "Expected WstETHWrap step".to_string(),
        ));
    };

    let calldata = wrapCall {
        _stETHAmount: *amount,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *wsteth,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!("Wrap {} wei stETH into wstETH", amount),
    }])
}
