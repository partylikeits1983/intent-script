//! Stage F: Execution planner — decide the execution strategy.
//!
//! - 1 call → SingleTx (direct EOA transaction)
//! - N calls + router available → Batched (single tx through IntentRouter)
//! - N calls + no router → TxSequence (EOA signs each separately)

use crate::ir::ConcreteCall;
use alloc::vec::Vec;
use alloy_primitives::Address;

/// Execution strategy decided by the planner.
pub enum ExecutionPlan {
    /// A single transaction (no router needed)
    Single(ConcreteCall),
    /// Multiple transactions to be signed and submitted in order (no router)
    Sequence(Vec<ConcreteCall>),
    /// Multiple calls batched into a single router.execute() transaction
    Batched {
        calls: Vec<ConcreteCall>,
        router: Address,
        tokens_to_sweep: Vec<Address>,
    },
}

/// Decide the execution strategy for a list of concrete calls.
///
/// When a router address is provided and there are multiple calls,
/// the planner produces a `Batched` plan instead of a `Sequence`.
pub fn plan(
    calls: &[ConcreteCall],
    router: Option<Address>,
    tokens_to_sweep: Vec<Address>,
) -> ExecutionPlan {
    match calls.len() {
        0 => unreachable!("Validator should have caught empty steps"),
        1 => ExecutionPlan::Single(calls[0].clone()),
        _ => {
            if let Some(router_addr) = router {
                ExecutionPlan::Batched {
                    calls: calls.to_vec(),
                    router: router_addr,
                    tokens_to_sweep,
                }
            } else {
                ExecutionPlan::Sequence(calls.to_vec())
            }
        }
    }
}
