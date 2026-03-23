//! Lido adapter.
//!
//! Generates calldata for:
//! - `lido.submit(address _referral)` — stake ETH and receive stETH

use alloy_primitives::Bytes;
use alloy_sol_types::SolCall;

use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, ResolvedStep};

alloy_sol_types::sol! {
    function submit(address _referral) external payable returns (uint256);
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
