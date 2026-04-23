//! Uniswap V3 NonfungiblePositionManager adapter.
//!
//! Generates calldata for the LP lifecycle:
//! - `NPM.mint(MintParams)` — mint a new position NFT
//! - `NPM.increaseLiquidity(IncreaseLiquidityParams)`
//! - `NPM.decreaseLiquidity(DecreaseLiquidityParams)`
//! - `NPM.collect(CollectParams)`

use alloc::format;
use alloc::string::ToString;
use alloc::vec;

use alloy_primitives::{Bytes, U256, Uint};
use alloy_sol_types::SolCall;

use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, ResolvedStep};

alloy_sol_types::sol! {
    struct MintParams {
        address token0;
        address token1;
        uint24 fee;
        int24 tickLower;
        int24 tickUpper;
        uint256 amount0Desired;
        uint256 amount1Desired;
        uint256 amount0Min;
        uint256 amount1Min;
        address recipient;
        uint256 deadline;
    }
    function mint(MintParams params) external payable
        returns (uint256 tokenId, uint128 liquidity, uint256 amount0, uint256 amount1);

    struct IncreaseLiquidityParams {
        uint256 tokenId;
        uint256 amount0Desired;
        uint256 amount1Desired;
        uint256 amount0Min;
        uint256 amount1Min;
        uint256 deadline;
    }
    function increaseLiquidity(IncreaseLiquidityParams params) external payable
        returns (uint128 liquidity, uint256 amount0, uint256 amount1);

    struct DecreaseLiquidityParams {
        uint256 tokenId;
        uint128 liquidity;
        uint256 amount0Min;
        uint256 amount1Min;
        uint256 deadline;
    }
    function decreaseLiquidity(DecreaseLiquidityParams params) external
        returns (uint256 amount0, uint256 amount1);

    struct CollectParams {
        uint256 tokenId;
        address recipient;
        uint128 amount0Max;
        uint128 amount1Max;
    }
    function collect(CollectParams params) external
        returns (uint256 amount0, uint256 amount1);
}

pub fn lower_lp_mint(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::UniswapV3LpMint {
        npm,
        token0,
        token1,
        fee,
        tick_lower,
        tick_upper,
        amount0,
        amount1,
        amount0_min,
        amount1_min,
        recipient,
        deadline,
    } = step
    else {
        return Err(CompileError::Adapter(
            "Expected UniswapV3LpMint step".to_string(),
        ));
    };

    let params = MintParams {
        token0: *token0,
        token1: *token1,
        fee: Uint::from(*fee),
        tickLower: alloy_primitives::Signed::try_from(*tick_lower).map_err(|_| {
            CompileError::InvalidAmount(format!("tick_lower {} out of int24 range", tick_lower))
        })?,
        tickUpper: alloy_primitives::Signed::try_from(*tick_upper).map_err(|_| {
            CompileError::InvalidAmount(format!("tick_upper {} out of int24 range", tick_upper))
        })?,
        amount0Desired: *amount0,
        amount1Desired: *amount1,
        amount0Min: *amount0_min,
        amount1Min: *amount1_min,
        recipient: *recipient,
        deadline: *deadline,
    };
    let calldata = mintCall { params }.abi_encode();

    Ok(vec![ConcreteCall {
        to: *npm,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Uniswap V3 mint LP {}/{} fee={} range=[{},{}]",
            token0, token1, fee, tick_lower, tick_upper
        ),
    }])
}

pub fn lower_lp_increase(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::UniswapV3LpIncrease {
        npm,
        token_id,
        amount0,
        amount1,
        amount0_min,
        amount1_min,
        deadline,
        ..
    } = step
    else {
        return Err(CompileError::Adapter(
            "Expected UniswapV3LpIncrease step".to_string(),
        ));
    };

    let params = IncreaseLiquidityParams {
        tokenId: *token_id,
        amount0Desired: *amount0,
        amount1Desired: *amount1,
        amount0Min: *amount0_min,
        amount1Min: *amount1_min,
        deadline: *deadline,
    };
    let calldata = increaseLiquidityCall { params }.abi_encode();

    Ok(vec![ConcreteCall {
        to: *npm,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!("Uniswap V3 increase LP #{}", token_id),
    }])
}

pub fn lower_lp_decrease(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::UniswapV3LpDecrease {
        npm,
        token_id,
        liquidity,
        amount0_min,
        amount1_min,
        deadline,
    } = step
    else {
        return Err(CompileError::Adapter(
            "Expected UniswapV3LpDecrease step".to_string(),
        ));
    };

    let params = DecreaseLiquidityParams {
        tokenId: *token_id,
        liquidity: *liquidity,
        amount0Min: *amount0_min,
        amount1Min: *amount1_min,
        deadline: *deadline,
    };
    let calldata = decreaseLiquidityCall { params }.abi_encode();

    Ok(vec![ConcreteCall {
        to: *npm,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Uniswap V3 decrease LP #{} by {} liquidity",
            token_id, liquidity
        ),
    }])
}

pub fn lower_lp_collect(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::UniswapV3LpCollect {
        npm,
        token_id,
        recipient,
        amount0_max,
        amount1_max,
        ..
    } = step
    else {
        return Err(CompileError::Adapter(
            "Expected UniswapV3LpCollect step".to_string(),
        ));
    };

    let params = CollectParams {
        tokenId: *token_id,
        recipient: *recipient,
        amount0Max: *amount0_max,
        amount1Max: *amount1_max,
    };
    let calldata = collectCall { params }.abi_encode();

    Ok(vec![ConcreteCall {
        to: *npm,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!("Uniswap V3 collect LP #{}", token_id),
    }])
}
