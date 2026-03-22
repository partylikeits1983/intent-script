//! Stage C: Validate — check the resolved IR for correctness.

use alloy_primitives::Address;

use crate::error::{CompileError, Result};
use crate::ir::ResolvedIntent;
use crate::registry::RegistryContext;

/// Validate a resolved intent.
pub fn validate(intent: &ResolvedIntent, _registry: &RegistryContext) -> Result<()> {
    // Check signer is not zero address
    if intent.signer == Address::ZERO {
        return Err(CompileError::Validation(
            "Signer address cannot be zero".to_string(),
        ));
    }

    // Check we have at least one step
    if intent.steps.is_empty() {
        return Err(CompileError::Validation(
            "Intent must have at least one step".to_string(),
        ));
    }

    Ok(())
}
