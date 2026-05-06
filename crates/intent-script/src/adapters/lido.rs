//! Lido adapter.
//!
//! Generates calldata for:
//! - `lido.submit(address _referral)` — stake ETH and receive stETH
//! - `wstETH.wrap(uint256 _stETHAmount)` — wrap stETH into wstETH
//! - `wstETH.unwrap(uint256 _wstETHAmount)` — unwrap wstETH back into stETH
//! - `WithdrawalQueue.requestWithdrawals(uint256[], address)` / `requestWithdrawalsWstETH(...)`
//! - `WithdrawalQueue.claimWithdrawals(uint256[], uint256[])`

use alloc::format;
use alloc::vec;

use alloy_primitives::{Bytes, U256};
use alloy_sol_types::SolCall;

use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, ResolvedStep};

alloy_sol_types::sol! {
    function submit(address _referral) external payable returns (uint256);
    function wrap(uint256 _stETHAmount) external returns (uint256);
    function unwrap(uint256 _wstETHAmount) external returns (uint256);
    function requestWithdrawals(uint256[] _amounts, address _owner) external returns (uint256[]);
    function requestWithdrawalsWstETH(uint256[] _amounts, address _owner) external returns (uint256[]);
    function claimWithdrawals(uint256[] _requestIds, uint256[] _hints) external;
}

/// Lower a LidoStake step to a concrete stETH.submit() call.
///
/// `submit()` is a method on the stETH token contract itself — passing ETH
/// in as msg.value mints stETH 1:1 to the caller.
pub fn lower_stake(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::LidoStake {
        steth,
        amount,
        referral,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "lido",
            expected: "LidoStake",
        });
    };

    let calldata = submitCall {
        _referral: *referral,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *steth,
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
        return Err(CompileError::AdapterStepMismatch {
            adapter: "lido",
            expected: "WstETHWrap",
        });
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

/// Lower a WstETHUnwrap step to a concrete wstETH.unwrap() call.
pub fn lower_wsteth_unwrap(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::WstETHUnwrap {
        wsteth,
        steth: _,
        amount,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "lido",
            expected: "WstETHUnwrap",
        });
    };

    let calldata = unwrapCall {
        _wstETHAmount: *amount,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *wsteth,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!("Unwrap {} wei wstETH into stETH", amount),
    }])
}

/// Lower a LidoRequestWithdrawal step to the queue's `requestWithdrawals` or
/// `requestWithdrawalsWstETH` call depending on the underlying token.
pub fn lower_request_withdrawal(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::LidoRequestWithdrawal {
        queue,
        token: _,
        is_wsteth,
        amounts,
        owner,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "lido",
            expected: "LidoRequestWithdrawal",
        });
    };

    let calldata = if *is_wsteth {
        requestWithdrawalsWstETHCall {
            _amounts: amounts.clone(),
            _owner: *owner,
        }
        .abi_encode()
    } else {
        requestWithdrawalsCall {
            _amounts: amounts.clone(),
            _owner: *owner,
        }
        .abi_encode()
    };

    let label = if *is_wsteth { "wstETH" } else { "stETH" };
    Ok(vec![ConcreteCall {
        to: *queue,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Request Lido withdrawal of {} NFT(s) in {}",
            amounts.len(),
            label
        ),
    }])
}

/// Lower a LidoClaimWithdrawal step to the queue's `claimWithdrawals` call.
pub fn lower_claim_withdrawal(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::LidoClaimWithdrawal {
        queue,
        request_ids,
        hints,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "lido",
            expected: "LidoClaimWithdrawal",
        });
    };

    let calldata = claimWithdrawalsCall {
        _requestIds: request_ids.clone(),
        _hints: hints.clone(),
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *queue,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!("Claim {} Lido withdrawal NFT(s)", request_ids.len()),
    }])
}
