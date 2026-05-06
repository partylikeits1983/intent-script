//! Morpho Blue adapter.
//!
//! Generates calldata for:
//! - `morpho.supply(MarketParams, assets, shares, onBehalf, data)` — supply loan asset
//! - `morpho.supplyCollateral(MarketParams, assets, onBehalf, data)` — supply collateral
//! - `morpho.borrow(MarketParams, assets, shares, onBehalf, receiver)` — borrow
//! - `morpho.withdraw(MarketParams, assets, shares, onBehalf, receiver)` — withdraw loan asset
//! - `morpho.withdrawCollateral(MarketParams, assets, onBehalf, receiver)` — withdraw collateral
//! - `morpho.repay(MarketParams, assets, shares, onBehalf, data)` — repay loan asset
//!
//! `shares` is always passed as `0` and `data` as empty — v1 does not use
//! Morpho callbacks (`onMorphoSupply` / `onMorphoRepay`).

use alloc::format;
use alloc::string::ToString;
use alloc::vec;

use alloy_primitives::{Bytes, U256};
use alloy_sol_types::SolCall;

use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, MorphoMarketParams, ResolvedStep};

alloy_sol_types::sol! {
    struct MarketParams {
        address loanToken;
        address collateralToken;
        address oracle;
        address irm;
        uint256 lltv;
    }
    function supply(MarketParams marketParams, uint256 assets, uint256 shares,
                    address onBehalf, bytes data) external returns (uint256, uint256);
    function supplyCollateral(MarketParams marketParams, uint256 assets,
                    address onBehalf, bytes data) external;
    function borrow(MarketParams marketParams, uint256 assets, uint256 shares,
                    address onBehalf, address receiver) external returns (uint256, uint256);
    function withdraw(MarketParams marketParams, uint256 assets, uint256 shares,
                    address onBehalf, address receiver) external returns (uint256, uint256);
    function withdrawCollateral(MarketParams marketParams, uint256 assets,
                    address onBehalf, address receiver) external;
    function repay(MarketParams marketParams, uint256 assets, uint256 shares,
                    address onBehalf, bytes data) external returns (uint256, uint256);
}

fn to_market_params(mp: &MorphoMarketParams) -> MarketParams {
    MarketParams {
        loanToken: mp.loan_token,
        collateralToken: mp.collateral_token,
        oracle: mp.oracle,
        irm: mp.irm,
        lltv: mp.lltv,
    }
}

pub fn lower_supply(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::MorphoSupply {
        pool,
        market_params,
        amount,
        on_behalf,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "morpho",
            expected: "MorphoSupply",
        });
    };

    let calldata = supplyCall {
        marketParams: to_market_params(market_params),
        assets: *amount,
        shares: U256::ZERO,
        onBehalf: *on_behalf,
        data: Bytes::new(),
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *pool,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Supply {} wei of loan asset {} to Morpho Blue",
            amount, market_params.loan_token
        ),
    }])
}

pub fn lower_supply_collateral(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::MorphoSupplyCollat {
        pool,
        market_params,
        amount,
        on_behalf,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "morpho",
            expected: "MorphoSupplyCollat",
        });
    };

    let calldata = supplyCollateralCall {
        marketParams: to_market_params(market_params),
        assets: *amount,
        onBehalf: *on_behalf,
        data: Bytes::new(),
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *pool,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Supply {} wei of collateral {} to Morpho Blue",
            amount, market_params.collateral_token
        ),
    }])
}

pub fn lower_borrow(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::MorphoBorrow {
        pool,
        market_params,
        amount,
        on_behalf,
        receiver,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "morpho",
            expected: "MorphoBorrow",
        });
    };

    let calldata = borrowCall {
        marketParams: to_market_params(market_params),
        assets: *amount,
        shares: U256::ZERO,
        onBehalf: *on_behalf,
        receiver: *receiver,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *pool,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Borrow {} wei of loan asset {} from Morpho Blue",
            amount, market_params.loan_token
        ),
    }])
}

pub fn lower_withdraw(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::MorphoWithdraw {
        pool,
        market_params,
        amount,
        on_behalf,
        receiver,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "morpho",
            expected: "MorphoWithdraw",
        });
    };

    let calldata = withdrawCall {
        marketParams: to_market_params(market_params),
        assets: *amount,
        shares: U256::ZERO,
        onBehalf: *on_behalf,
        receiver: *receiver,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *pool,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Withdraw {} wei of loan asset {} from Morpho Blue",
            amount, market_params.loan_token
        ),
    }])
}

pub fn lower_withdraw_collateral(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::MorphoWithdrawCollat {
        pool,
        market_params,
        amount,
        on_behalf,
        receiver,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "morpho",
            expected: "MorphoWithdrawCollat",
        });
    };

    let calldata = withdrawCollateralCall {
        marketParams: to_market_params(market_params),
        assets: *amount,
        onBehalf: *on_behalf,
        receiver: *receiver,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *pool,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Withdraw {} wei of collateral {} from Morpho Blue",
            amount, market_params.collateral_token
        ),
    }])
}

pub fn lower_repay(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::MorphoRepay {
        pool,
        market_params,
        amount,
        on_behalf,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "morpho",
            expected: "MorphoRepay",
        });
    };

    let calldata = repayCall {
        marketParams: to_market_params(market_params),
        assets: *amount,
        shares: U256::ZERO,
        onBehalf: *on_behalf,
        data: Bytes::new(),
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *pool,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Repay {} wei of loan asset {} to Morpho Blue",
            amount, market_params.loan_token
        ),
    }])
}
