//! Uniswap V3 adapter.
//!
//! Generates calldata for:
//! - `router.exactInputSingle(ExactInputSingleParams)` — single-hop swap

use alloy_primitives::{Bytes, U256, Uint};
use alloy_sol_types::SolCall;

use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, ResolvedStep};

alloy_sol_types::sol! {
    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 deadline;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }

    function exactInputSingle(ExactInputSingleParams params) external payable returns (uint256 amountOut);
}

/// Lower a UniswapV3Swap step to a concrete router.exactInputSingle() call.
pub fn lower_swap(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::UniswapV3Swap {
        router,
        token_in,
        token_out,
        amount_in,
        fee,
        recipient,
        deadline,
        amount_out_minimum,
    } = step
    else {
        return Err(CompileError::Adapter(
            "Expected UniswapV3Swap step".to_string(),
        ));
    };

    let params = ExactInputSingleParams {
        tokenIn: *token_in,
        tokenOut: *token_out,
        fee: Uint::from(*fee),
        recipient: *recipient,
        deadline: *deadline,
        amountIn: *amount_in,
        amountOutMinimum: *amount_out_minimum,
        sqrtPriceLimitX96: Uint::ZERO,
    };

    let calldata = exactInputSingleCall { params }.abi_encode();

    Ok(vec![ConcreteCall {
        to: *router,
        calldata: Bytes::from(calldata),
        value: U256::ZERO, // ERC-20 swap, no ETH value
        description: format!(
            "Swap {} wei of {} → {} via Uniswap V3 (fee tier {})",
            amount_in, token_in, token_out, fee
        ),
    }])
}
