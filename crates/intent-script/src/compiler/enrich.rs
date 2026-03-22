//! Stage D: Enrich — insert automatically-generated steps like approvals.
//!
//! For example, an Aave V3 supply step needs an ERC-20 approval before it.

use crate::error::Result;
use crate::ir::{ResolvedIntent, ResolvedStep};
use crate::registry::RegistryContext;

/// Enrich a resolved intent by inserting necessary intermediate steps.
pub fn enrich(mut intent: ResolvedIntent, _registry: &RegistryContext) -> Result<ResolvedIntent> {
    let mut enriched_steps = Vec::new();

    for step in &intent.steps {
        match step {
            ResolvedStep::AaveV3Supply {
                pool,
                asset,
                amount,
                ..
            } => {
                // Insert ERC-20 approve before supply
                enriched_steps.push(ResolvedStep::Erc20Approve {
                    token: *asset,
                    spender: *pool,
                    amount: *amount,
                });
                enriched_steps.push(step.clone());
            }
            // Other steps don't need enrichment in v1
            _ => {
                enriched_steps.push(step.clone());
            }
        }
    }

    intent.steps = enriched_steps;
    Ok(intent)
}
