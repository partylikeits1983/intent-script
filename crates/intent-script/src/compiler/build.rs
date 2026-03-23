//! Stage G: Build — produce final unsigned transactions from the execution plan.

use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::SolCall;

use crate::compiler::plan::ExecutionPlan;
use crate::ir::ConcreteCall;
use crate::output::{CompileOutput, UnsignedTx};

// ABI definition for IntentRouter.execute()
alloy_sol_types::sol! {
    struct RouterCall {
        address target;
        bytes callData;
        uint256 value;
    }

    function execute(RouterCall[] calls, address[] tokensToSweep) external payable;
}

/// Build the final compile output from an execution plan.
pub fn build(plan: ExecutionPlan, chain_id: u64, signer: Address) -> CompileOutput {
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

            // Encode calls into RouterCall structs
            let router_calls: Vec<RouterCall> = calls
                .iter()
                .map(|c| RouterCall {
                    target: c.to,
                    callData: c.calldata.to_vec().into(),
                    value: c.value,
                })
                .collect();

            // ABI-encode router.execute(calls, tokensToSweep)
            let calldata = executeCall {
                calls: router_calls,
                tokensToSweep: tokens_to_sweep,
            }
            .abi_encode();

            CompileOutput::SingleTx(UnsignedTx {
                to: router,
                data: Bytes::from(calldata),
                value: total_value,
                chain_id,
                from: signer,
                description,
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
