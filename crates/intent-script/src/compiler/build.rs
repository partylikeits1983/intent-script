//! Stage G: Build — produce final unsigned transactions from the execution plan.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::SolCall;

use crate::compiler::plan::ExecutionPlan;
use crate::eip712;
use crate::ir::ConcreteCall;
use crate::output::{
    CallData, CompileOutput, Eip712Domain, Eip712IntentOutput, IntentBatchData, UnsignedTx,
};

// ABI definition for IntentRouter.executeDirect()
alloy_sol_types::sol! {
    struct RouterCall {
        address target;
        bytes callData;
        uint256 value;
    }

    function executeDirect(RouterCall[] calls, address[] tokensToSweep) external payable;
}

/// Build the final compile output from an execution plan.
pub fn build(
    plan: ExecutionPlan,
    chain_id: u64,
    signer: Address,
    nonce: u64,
    deadline: u64,
) -> CompileOutput {
    match plan {
        ExecutionPlan::Single(call) => {
            CompileOutput::SingleTx(call_to_unsigned_tx(call, chain_id, signer))
        }
        ExecutionPlan::Sequence(calls) => {
            let txs = calls
                .into_iter()
                .map(|c| call_to_unsigned_tx(c, chain_id, signer))
                .collect();
            CompileOutput::TxSequence(txs)
        }
        ExecutionPlan::Batched {
            calls,
            router,
            tokens_to_sweep,
        } => {
            // Sum all ETH values across calls
            let total_value = calls.iter().fold(U256::ZERO, |acc, c| acc + c.value);

            // Build descriptions
            let descriptions: Vec<String> = calls.iter().map(|c| c.description.clone()).collect();
            let description = format!("Batched via router: [{}]", descriptions.join(", "));

            // Build CallData for EIP-712
            let call_data_vec: Vec<CallData> = calls
                .iter()
                .map(|c| CallData {
                    target: c.to,
                    call_data: c.calldata.clone(),
                    value: c.value,
                })
                .collect();

            // Build the direct tx (executeDirect calldata)
            let router_calls: Vec<RouterCall> = calls
                .iter()
                .map(|c| RouterCall {
                    target: c.to,
                    callData: c.calldata.to_vec().into(),
                    value: c.value,
                })
                .collect();

            let direct_calldata = executeDirectCall {
                calls: router_calls,
                tokensToSweep: tokens_to_sweep.clone(),
            }
            .abi_encode();

            let direct_tx = UnsignedTx {
                to: router,
                data: Bytes::from(direct_calldata),
                value: total_value,
                chain_id,
                from: signer,
                description: description.clone(),
            };

            // Build EIP-712 domain
            let domain = Eip712Domain {
                name: "IntentRouter".to_string(),
                version: "1".to_string(),
                chain_id,
                verifying_contract: router,
            };

            // Compute EIP-712 hashes
            let domain_separator = eip712::compute_domain_separator(
                &domain.name,
                &domain.version,
                domain.chain_id,
                domain.verifying_contract,
            );

            let eip712_calls: Vec<(Address, Bytes, U256)> = call_data_vec
                .iter()
                .map(|c| (c.target, c.call_data.clone(), c.value))
                .collect();

            let struct_hash =
                eip712::hash_intent_batch(signer, &eip712_calls, &tokens_to_sweep, nonce, deadline);

            let typed_data_hash = eip712::compute_typed_data_hash(&domain_separator, &struct_hash);

            let intent_batch = IntentBatchData {
                signer,
                calls: call_data_vec,
                tokens_to_sweep,
                nonce,
                deadline,
            };

            CompileOutput::Eip712Intent(Eip712IntentOutput {
                domain,
                intent_batch,
                typed_data_hash,
                description,
                direct_tx,
            })
        }
    }
}

fn call_to_unsigned_tx(call: ConcreteCall, chain_id: u64, from: Address) -> UnsignedTx {
    UnsignedTx {
        to: call.to,
        data: call.calldata,
        value: call.value,
        chain_id,
        from,
        description: call.description,
    }
}
