//! ERC-20 approve and permit adapters.
//!
//! Generates:
//! - `token.approve(spender, amount)` calldata
//! - `token.permit(owner, spender, value, deadline, v, r, s)` calldata

use alloc::format;
use alloc::vec;

use alloy_primitives::{B256, Bytes, U256};
use alloy_sol_types::SolCall;

use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, ResolvedStep};

alloy_sol_types::sol! {
    function approve(address spender, uint256 amount) external returns (bool);
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
    function permit(address owner, address spender, uint256 value, uint256 deadline, uint8 v, bytes32 r, bytes32 s) external;
}

/// Lower an Erc20Approve step to a concrete approve() call.
pub fn lower_approve(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::Erc20Approve {
        token,
        spender,
        amount,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "erc20",
            expected: "Erc20Approve",
        });
    };

    let calldata = approveCall {
        spender: *spender,
        amount: *amount,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *token,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Approve {} wei of token {} for spender {}",
            amount, token, spender
        ),
    }])
}

/// Lower an Erc20TransferFrom step to a concrete transferFrom() call.
pub fn lower_transfer_from(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::Erc20TransferFrom {
        token,
        from,
        to,
        amount,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "erc20",
            expected: "Erc20TransferFrom",
        });
    };

    let calldata = transferFromCall {
        from: *from,
        to: *to,
        amount: *amount,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *token,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "TransferFrom {} wei of token {} from {} to {}",
            amount, token, from, to
        ),
    }])
}

/// Lower an Erc20Permit step to a concrete permit() call.
///
/// Note: at compile time, `v`, `r`, `s` are placeholder zeros — the frontend
/// fills them after the user signs the EIP-2612 permit message.
pub fn lower_permit(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::Erc20Permit {
        token,
        owner,
        spender,
        value,
        deadline,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "erc20",
            expected: "Erc20Permit",
        });
    };

    let calldata = permitCall {
        owner: *owner,
        spender: *spender,
        value: *value,
        deadline: *deadline,
        v: 0,
        r: B256::ZERO,
        s: B256::ZERO,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *token,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: format!(
            "Permit {} wei of token {} from {} for spender {} (signature placeholder)",
            value, token, owner, spender
        ),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permit_selector() {
        // permit(address,address,uint256,uint256,uint8,bytes32,bytes32) selector = 0xd505accf
        let calldata = permitCall {
            owner: alloy_primitives::Address::ZERO,
            spender: alloy_primitives::Address::ZERO,
            value: U256::ZERO,
            deadline: U256::ZERO,
            v: 0,
            r: B256::ZERO,
            s: B256::ZERO,
        }
        .abi_encode();

        assert_eq!(&calldata[..4], &[0xd5, 0x05, 0xac, 0xcf]);
    }
}
