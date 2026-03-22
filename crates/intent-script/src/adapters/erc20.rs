//! ERC-20 approve adapter.
//!
//! Generates `token.approve(spender, amount)` calldata.

use alloy_primitives::{Bytes, U256};
use alloy_sol_types::SolCall;

use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, ResolvedStep};

alloy_sol_types::sol! {
    function approve(address spender, uint256 amount) external returns (bool);
}

/// Lower an Erc20Approve step to a concrete approve() call.
pub fn lower_approve(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::Erc20Approve {
        token,
        spender,
        amount,
    } = step
    else {
        return Err(CompileError::Adapter(
            "Expected Erc20Approve step".to_string(),
        ));
    };

    let calldata = approveCall {
        spender: *spender,
        amount: *amount,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *token,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Approve {} wei of token {} for spender {}",
            amount, token, spender
        ),
    }])
}
