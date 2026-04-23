//! Across V3 bridging adapter.
//!
//! Lowers an `AcrossDepositV3` step to a `SpokePool.depositV3(...)` call. v1
//! supports only source-chain deposits on Ethereum mainnet; receiving on the
//! destination chain is a separate intent.

use alloc::format;
use alloc::string::ToString;
use alloc::vec;

use alloy_primitives::{Bytes, U256};
use alloy_sol_types::SolCall;

use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, ResolvedStep};

alloy_sol_types::sol! {
    function depositV3(
        address depositor,
        address recipient,
        address inputToken,
        address outputToken,
        uint256 inputAmount,
        uint256 outputAmount,
        uint256 destinationChainId,
        address exclusiveRelayer,
        uint32 quoteTimestamp,
        uint32 fillDeadline,
        uint32 exclusivityDeadline,
        bytes message
    ) external payable;
}

pub fn lower_deposit_v3(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::AcrossDepositV3 {
        spoke_pool,
        depositor,
        recipient,
        input_token,
        output_token,
        input_amount,
        output_amount,
        destination_chain_id,
        exclusive_relayer,
        quote_timestamp,
        fill_deadline,
        exclusivity_deadline,
        message,
    } = step
    else {
        return Err(CompileError::Adapter(
            "Expected AcrossDepositV3 step".to_string(),
        ));
    };

    let calldata = depositV3Call {
        depositor: *depositor,
        recipient: *recipient,
        inputToken: *input_token,
        outputToken: *output_token,
        inputAmount: *input_amount,
        outputAmount: *output_amount,
        destinationChainId: *destination_chain_id,
        exclusiveRelayer: *exclusive_relayer,
        quoteTimestamp: *quote_timestamp,
        fillDeadline: *fill_deadline,
        exclusivityDeadline: *exclusivity_deadline,
        message: message.clone(),
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *spoke_pool,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Across V3 deposit of {} wei of token {} to chain {}",
            input_amount, input_token, destination_chain_id
        ),
    }])
}
