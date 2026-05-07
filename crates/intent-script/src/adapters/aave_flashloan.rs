//! Aave V3 single-asset flashloan adapter.
//!
//! Lowers an `AaveFlashloan` step to a single outer
//! `pool.flashLoanSimple(receiver, asset, amount, params, 0)` call. The
//! `params` argument carries the inner pipeline ABI-encoded as the router's
//! `Call[]` struct. Aave transfers `amount` of `asset` to `receiver` (= router)
//! before calling `receiver.executeOperation(asset, amount, premium, initiator,
//! params)`. The router's `executeOperation` decodes `params`, executes each
//! inner call subject to the allowlist, then approves the pool to pull
//! `amount + premium` (Aave repays via `transferFrom`, unlike Balancer's
//! balance-comparison repayment).

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
    /// Local to this module so the type isn't shared with balancer.rs by name.
    struct AaveRouterCall {
        address target;
        bytes callData;
        uint256 value;
    }

    function flashLoanSimple(
        address receiverAddress,
        address asset,
        uint256 amount,
        bytes params,
        uint16 referralCode
    ) external;
}

pub fn lower_flashloan(
    step: &ResolvedStep,
    registry: &RegistryContext,
) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::AaveFlashloan {
        pool,
        asset,
        amount,
        premium_bps: _,
        inner_steps,
    } = step
    else {
        return Err(CompileError::AdapterStepMismatch {
            adapter: "aave_flashloan",
            expected: "AaveFlashloan",
        });
    };

    // 1. Lower the inner pipeline bottom-up into concrete calls.
    let mut inner_calls: Vec<ConcreteCall> = Vec::new();
    for inner in inner_steps {
        // Nested flashloans are rejected by validate, but be defensive.
        if matches!(
            inner,
            ResolvedStep::BalancerFlashloan { .. } | ResolvedStep::AaveFlashloan { .. }
        ) {
            return Err(CompileError::Validation(
                "nested flashloans are not allowed (max depth 1)".to_string(),
            ));
        }
        let sub = adapters::lower_step(inner, registry)?;
        inner_calls.extend(sub);
    }

    // 2. Convert to the wire-level Call[] tuple that IntentRouter expects.
    let router_calls: Vec<AaveRouterCall> = inner_calls
        .iter()
        .map(|c| AaveRouterCall {
            target: c.to,
            callData: c.calldata.clone(),
            value: c.value,
        })
        .collect();

    // ABI-encode `Call[]` so the router's `abi.decode(params, (Call[]))`
    // round-trips cleanly.
    let params = Bytes::from(router_calls.abi_encode());

    // 3. Receiver must be the IntentRouter — that's where Aave transfers
    //    tokens and which it calls back via `executeOperation`.
    let receiver =
        registry
            .router_address()
            .ok_or_else(|| CompileError::ProtocolContractMissing {
                protocol: "intent_router".to_string(),
                contract: "router".to_string(),
            })?;

    let calldata = flashLoanSimpleCall {
        receiverAddress: receiver,
        asset: *asset,
        amount: *amount,
        params,
        referralCode: 0u16,
    }
    .abi_encode();

    let descr = format!(
        "Aave V3 flashLoanSimple of {} with {} inner call(s)",
        amount,
        inner_calls.len()
    );
    Ok(vec![ConcreteCall {
        to: *pool,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        description: descr,
    }])
}

/// Decode the `params` ABI back into inner calls — used by tests to assert
/// round-trip correctness without requiring Solidity.
#[allow(dead_code)]
pub fn decode_inner_calls(params: &[u8]) -> Result<Vec<ConcreteCall>> {
    let decoded: Vec<AaveRouterCall> = Vec::<AaveRouterCall>::abi_decode(params)
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
