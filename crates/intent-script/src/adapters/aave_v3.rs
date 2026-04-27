//! Aave V3 adapter.
//!
//! Generates calldata for:
//! - `pool.supply(address asset, uint256 amount, address onBehalfOf, uint16 referralCode)`
//! - `pool.borrow(address asset, uint256 amount, uint256 interestRateMode, uint16 referralCode, address onBehalfOf)`
//! - `pool.withdraw(address asset, uint256 amount, address to)`

use alloc::format;
use alloc::string::ToString;
use alloc::vec;

use alloy_primitives::{Bytes, U256};
use alloy_sol_types::SolCall;

use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, ResolvedStep};

alloy_sol_types::sol! {
    function supply(address asset, uint256 amount, address onBehalfOf, uint16 referralCode) external;
    function borrow(address asset, uint256 amount, uint256 interestRateMode, uint16 referralCode, address onBehalfOf) external;
    function withdraw(address asset, uint256 amount, address to) external returns (uint256);
    function repay(address asset, uint256 amount, uint256 rateMode, address onBehalfOf) external returns (uint256);
}

/// Lower an AaveV3Supply step to a concrete pool.supply() call.
pub fn lower_supply(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::AaveV3Supply {
        pool,
        asset,
        amount,
        on_behalf_of,
    } = step
    else {
        return Err(CompileError::Adapter(
            "Expected AaveV3Supply step".to_string(),
        ));
    };

    let calldata = supplyCall {
        asset: *asset,
        amount: *amount,
        onBehalfOf: *on_behalf_of,
        referralCode: 0,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *pool,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!("Supply {} wei of asset {} to Aave V3", amount, asset),
    }])
}

/// Lower an AaveV3Borrow step to a concrete pool.borrow() call.
pub fn lower_borrow(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::AaveV3Borrow {
        pool,
        asset,
        amount,
        rate_mode,
        on_behalf_of,
    } = step
    else {
        return Err(CompileError::Adapter(
            "Expected AaveV3Borrow step".to_string(),
        ));
    };

    let calldata = borrowCall {
        asset: *asset,
        amount: *amount,
        interestRateMode: U256::from(*rate_mode),
        referralCode: 0,
        onBehalfOf: *on_behalf_of,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *pool,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Borrow {} wei of asset {} from Aave V3 (rate mode {})",
            amount, asset, rate_mode
        ),
    }])
}

/// Lower an AaveV3Repay step to a concrete pool.repay() call.
pub fn lower_repay(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::AaveV3Repay {
        pool,
        asset,
        amount,
        rate_mode,
        on_behalf_of,
    } = step
    else {
        return Err(CompileError::Adapter(
            "Expected AaveV3Repay step".to_string(),
        ));
    };

    let calldata = repayCall {
        asset: *asset,
        amount: *amount,
        rateMode: U256::from(*rate_mode),
        onBehalfOf: *on_behalf_of,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *pool,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Repay {} wei of asset {} to Aave V3 (rate mode {})",
            amount, asset, rate_mode
        ),
    }])
}

/// Lower an AaveV3Withdraw step to a concrete pool.withdraw() call.
pub fn lower_withdraw(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::AaveV3Withdraw {
        pool,
        asset,
        amount,
        to,
    } = step
    else {
        return Err(CompileError::Adapter(
            "Expected AaveV3Withdraw step".to_string(),
        ));
    };

    let calldata = withdrawCall {
        asset: *asset,
        amount: *amount,
        to: *to,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *pool,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!("Withdraw {} wei of asset {} from Aave V3", amount, asset),
    }])
}
