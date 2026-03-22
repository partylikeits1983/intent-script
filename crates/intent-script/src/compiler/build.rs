//! Stage G: Build — produce final unsigned transactions from the execution plan.

use alloy_primitives::Address;

use crate::compiler::plan::ExecutionPlan;
use crate::ir::ConcreteCall;
use crate::output::{CompileOutput, UnsignedTx};

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
