//! Uniswap V3 adapter.
//!
//! Generates calldata for `SwapRouter02.exactInputSingle(params)` — the
//! modern unified V3 router used on every chain we target (Ethereum L1,
//! Base, Arbitrum, Optimism, …). The original V3 SwapRouter (with a
//! `deadline` field) is intentionally not supported; switching all
//! deployments to SwapRouter02 means a single ABI everywhere.

use alloc::format;
use alloc::vec;

use alloy_primitives::{Bytes, U256, Uint};
use alloy_sol_types::SolCall;

use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, ResolvedStep};

alloy_sol_types::sol! {
    /// SwapRouter02 ABI — no `deadline` field. Function selector
    /// `0x04e45aaf`. Deadline handling moved to a Multicall wrapper in
    /// V2; we don't use that wrapper, so the pool itself enforces price
    /// freshness via the slippage `amountOutMinimum` check instead.
    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }

    function exactInputSingle(ExactInputSingleParams params) external payable returns (uint256 amountOut);
}

/// Lower a `UniswapV3Swap` step to a single SwapRouter02
/// `exactInputSingle` call.
pub fn lower_swap(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::UniswapV3Swap {
        router,
        token_in,
        token_out,
        amount_in,
        fee,
        recipient,
        amount_out_minimum,
        native_input,
        ..
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "uniswap_v3",
            expected: "UniswapV3Swap",
        });
    };

    let params = ExactInputSingleParams {
        tokenIn: *token_in,
        tokenOut: *token_out,
        fee: Uint::from(*fee),
        recipient: *recipient,
        amountIn: *amount_in,
        amountOutMinimum: *amount_out_minimum,
        sqrtPriceLimitX96: Uint::ZERO,
    };

    let calldata = exactInputSingleCall { params }.abi_encode();

    // Native-input swaps pay `amount_in` as msg.value; the SwapRouter's
    // internal `pay()` wraps it into WETH when tokenIn == WETH9.
    let value = if *native_input {
        *amount_in
    } else {
        U256::ZERO
    };

    Ok(vec![ConcreteCall {
        to: *router,
        calldata: Bytes::from(calldata),
        value,
        description: format!(
            "Swap {} wei of {} → {} via Uniswap V3 (fee tier {})",
            amount_in, token_in, token_out, fee
        ),
    }])
}
