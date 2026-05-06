//! Send adapters — ERC-20 transfer, ETH send, ERC-721 safeTransferFrom.

use alloc::format;
use alloc::vec;

use alloy_primitives::{Bytes, U256};
use alloy_sol_types::SolCall;

use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, ResolvedStep};

alloy_sol_types::sol! {
    function transfer(address to, uint256 amount) external returns (bool);
    function safeTransferFrom(address from, address to, uint256 tokenId) external;
}

/// Lower a SendErc20 step to a concrete transfer() call.
pub fn lower_send_erc20(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::SendErc20 { token, to, amount } = step else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "send",
            expected: "SendErc20",
        });
    };

    let calldata = transferCall {
        to: *to,
        amount: *amount,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *token,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!("Transfer {} wei of token {} to {}", amount, token, to),
    }])
}

/// Lower a SendEth step to a concrete ETH transfer (empty calldata).
pub fn lower_send_eth(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::SendEth { to, amount } = step else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "send",
            expected: "SendEth",
        });
    };

    Ok(vec![ConcreteCall {
        to: *to,
        calldata: Bytes::new(),
        value: *amount,
        description: format!("Send {} wei ETH to {}", amount, to),
    }])
}

/// Lower a SendErc721 step to a concrete safeTransferFrom() call.
pub fn lower_send_erc721(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::SendErc721 {
        contract,
        from,
        to,
        token_id,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "send",
            expected: "SendErc721",
        });
    };

    let calldata = safeTransferFromCall {
        from: *from,
        to: *to,
        tokenId: *token_id,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *contract,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Transfer NFT #{} from contract {} from {} to {}",
            token_id, contract, from, to
        ),
    }])
}
