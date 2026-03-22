//! Stage F: Execution planner — decide the execution strategy.
//!
//! For EOA wallets:
//! - 1 call → SingleTx
//! - N calls → TxSequence (EOA can only do one call per tx)

use crate::ir::ConcreteCall;

/// Execution strategy decided by the planner.
pub enum ExecutionPlan {
    /// A single transaction
    Single(ConcreteCall),
    /// Multiple transactions to be signed and submitted in order
    Sequence(Vec<ConcreteCall>),
}

/// Decide the execution strategy for a list of concrete calls.
pub fn plan(calls: &[ConcreteCall]) -> ExecutionPlan {
    match calls.len() {
        0 => unreachable!("Validator should have caught empty steps"),
        1 => ExecutionPlan::Single(calls[0].clone()),
        _ => ExecutionPlan::Sequence(calls.to_vec()),
    }
}
