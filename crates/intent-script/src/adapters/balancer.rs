//! Balancer V2 flashloan adapter.
//!
//! Lowers a `BalancerFlashloan` step to a single outer
//! `vault.flashLoan(recipient, tokens, amounts, userData)` call. The
//! `userData` carries the inner pipeline ABI-encoded as the router's
//! `Call[]` struct. The Vault transfers `amounts[i]` of each `tokens[i]` to
//! `recipient` (= router), then invokes `recipient.receiveFlashLoan(...)`,
//! which decodes `userData` and executes each inner call.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::{SolCall, SolValue, sol};

use crate::adapters;
use crate::error::{CompileError, Result};
use crate::ir::{ConcreteCall, ResolvedStep};
use crate::registry::RegistryContext;

sol! {
    /// Mirrors IntentRouter.Call — kept in sync with contracts/src/IntentRouter.sol.
    struct RouterCall {
        address target;
        bytes callData;
        uint256 value;
    }

    function flashLoan(
        address recipient,
        address[] tokens,
        uint256[] amounts,
        bytes userData
    ) external;
}

pub fn lower_flashloan(
    step: &ResolvedStep,
    registry: &RegistryContext,
) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::BalancerFlashloan {
        vault,
        tokens,
        amounts,
        inner_steps,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "balancer",
            expected: "BalancerFlashloan",
        });
    };

    // 1. Lower the inner pipeline bottom-up into concrete calls.
    let mut inner_calls: Vec<ConcreteCall> = Vec::new();
    for inner in inner_steps {
        // Nested flashloans are rejected by validate, but be defensive.
        if matches!(inner, ResolvedStep::BalancerFlashloan { .. }) {
            return Err(CompileError::Validation(
                "nested flashloans are not allowed (max depth 1)".to_string(),
            ));
        }
        let sub = adapters::lower_step(inner, registry)?;
        inner_calls.extend(sub);
    }

    // 2. Convert to the wire-level Call[] tuple that IntentRouter expects.
    let router_calls: Vec<RouterCall> = inner_calls
        .iter()
        .map(|c| RouterCall {
            target: c.to,
            callData: c.calldata.clone(),
            value: c.value,
        })
        .collect();

    // ABI-encode `Call[]` so the router's `abi.decode(userData, (Call[]))`
    // round-trips cleanly.
    let user_data = Bytes::from(router_calls.abi_encode());

    // 3. Recipient must be the IntentRouter — that's where the Vault transfers
    //    tokens and which it calls back.
    let recipient =
        registry
            .router_address()
            .ok_or_else(|| CompileError::ProtocolContractMissing {
                protocol: "intent_router".to_string(),
                contract: "router".to_string(),
            })?;

    let calldata = flashLoanCall {
        recipient,
        tokens: tokens.clone(),
        amounts: amounts.clone(),
        userData: user_data,
    }
    .abi_encode();

    let descr = format!(
        "Balancer V2 flashloan of {} asset(s) with {} inner call(s)",
        tokens.len(),
        inner_calls.len()
    );
    Ok(vec![ConcreteCall {
        to: *vault,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: descr,
    }])
}

/// Decode the `userData` ABI back into inner calls — used by tests to assert
/// round-trip correctness without requiring Solidity.
pub fn decode_inner_calls(user_data: &[u8]) -> Result<Vec<ConcreteCall>> {
    let decoded: Vec<RouterCall> = Vec::<RouterCall>::abi_decode(user_data)
        .map_err(|e| CompileError::Adapter(format!("decode inner Call[] failed: {e}")))?;
    Ok(decoded
        .into_iter()
        .map(|c| ConcreteCall {
            to: c.target,
            calldata: c.callData,
            value: c.value,
            description: String::new(),
        })
        .collect())
}

#[allow(dead_code)]
fn _suppress_unused_address(_a: Address) {}
