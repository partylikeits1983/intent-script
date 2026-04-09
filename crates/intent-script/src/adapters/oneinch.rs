//! 1inch adapter.
//!
//! Passes through pre-fetched calldata from the frontend.
//! The compiler does not make HTTP calls — the calldata is provided
//! in the intent JSON by the frontend after querying the 1inch API.

use alloc::string::ToString;
use alloc::vec;

use alloy_primitives::U256;

use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, ResolvedStep};

/// Lower a OneInchSwap step to a concrete call with pre-fetched calldata.
pub fn lower_oneinch_swap(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::OneInchSwap {
        router,
        token_in: _,
        token_out: _,
        amount_in: _,
        calldata,
    } = step
    else {
        return Err(CompileError::Adapter(
            "Expected OneInchSwap step".to_string(),
        ));
    };

    Ok(vec![ConcreteCall {
        to: *router,
        calldata: calldata.clone(),
        value: U256::ZERO,
        description: "Swap via 1inch aggregator (pre-fetched calldata)".to_string(),
    }])
}
