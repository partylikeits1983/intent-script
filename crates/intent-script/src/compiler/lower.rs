//! Stage E: Lower — convert resolved steps into concrete EVM calls.

use alloc::vec::Vec;

use crate::adapters;
use crate::error::Result;
use crate::ir::{ConcreteCall, ResolvedIntent};
use crate::registry::RegistryContext;

/// Lower all resolved steps into concrete EVM calls.
pub fn lower(intent: &ResolvedIntent, registry: &RegistryContext) -> Result<Vec<ConcreteCall>> {
    let mut calls = Vec::new();

    for step in &intent.steps {
        let step_calls = adapters::lower_step(step, registry)?;
        calls.extend(step_calls);
    }

    Ok(calls)
}
