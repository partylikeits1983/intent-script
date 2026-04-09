//! WETH wrap/unwrap adapter.
//!
//! - Wrap: calls `WETH.deposit{value: amount}()` — function selector 0xd0e30db0
//! - Unwrap: calls `WETH.withdraw(uint256)` — function selector 0x2e1a7d4d

use alloc::format;
use alloc::string::ToString;
use alloc::vec;

use alloy_primitives::{Bytes, U256};
use alloy_sol_types::SolCall;

use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, ResolvedStep};

// Define the WETH ABI using alloy sol! macro
alloy_sol_types::sol! {
    function deposit() external payable;
    function withdraw(uint256 wad) external;
}

/// Lower a Wrap step to a concrete WETH.deposit() call.
pub fn lower_wrap(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::Wrap {
        wrapped_token,
        amount,
    } = step
    else {
        return Err(CompileError::Adapter("Expected Wrap step".to_string()));
    };

    let calldata = depositCall {}.abi_encode();

    Ok(vec![ConcreteCall {
        to: *wrapped_token,
        calldata: Bytes::from(calldata),
        value: *amount,
        description: format!("Wrap {} wei of native asset to wrapped token", amount),
    }])
}

/// Lower an Unwrap step to a concrete WETH.withdraw() call.
pub fn lower_unwrap(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::Unwrap {
        wrapped_token,
        amount,
    } = step
    else {
        return Err(CompileError::Adapter("Expected Unwrap step".to_string()));
    };

    let calldata = withdrawCall { wad: *amount }.abi_encode();

    Ok(vec![ConcreteCall {
        to: *wrapped_token,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!("Unwrap {} wei of wrapped token to native asset", amount),
    }])
}
