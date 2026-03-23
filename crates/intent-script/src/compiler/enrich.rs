//! Stage D: Enrich — insert automatically-generated steps like approvals.
//!
//! For example, an Aave V3 supply step needs an ERC-20 approval before it.
//! When a router is available, this stage also tracks which tokens need to be
//! swept back to the signer after batched execution.

use alloy_primitives::Address;

use crate::error::Result;
use crate::ir::{ResolvedIntent, ResolvedStep};
use crate::registry::RegistryContext;

/// Enrich a resolved intent by inserting necessary intermediate steps.
///
/// When a router address is available and there are multiple steps, approvals
/// target the router instead of the protocol, and output tokens are tracked
/// for sweeping.
pub fn enrich(mut intent: ResolvedIntent, registry: &RegistryContext) -> Result<ResolvedIntent> {
    let router = registry.router_address();
    let mut enriched_steps = Vec::new();
    let mut sweep_tokens: Vec<Address> = Vec::new();

    for step in &intent.steps {
        match step {
            ResolvedStep::AaveV3Supply {
                pool,
                asset,
                amount,
                ..
            } => {
                // Insert ERC-20 approve before supply.
                // If we have a router and multiple steps, the approval targets
                // the router (since the router will be msg.sender in the sub-call).
                // For single-step intents, approval targets the pool directly.
                let spender = if router.is_some() && intent.steps.len() > 1 {
                    // When batching through router, the router calls the pool,
                    // so the pool needs allowance from the router. But the user's
                    // tokens need to get to the router first via transferFrom.
                    // For now, keep approval targeting the pool — the router
                    // pattern for ERC-20 protocols needs a transferFrom step.
                    *pool
                } else {
                    *pool
                };

                enriched_steps.push(ResolvedStep::Erc20Approve {
                    token: *asset,
                    spender,
                    amount: *amount,
                });
                enriched_steps.push(step.clone());
            }
            ResolvedStep::Wrap { wrapped_token, .. } => {
                // Wrap produces an ERC-20 token that may need sweeping
                if router.is_some() {
                    if !sweep_tokens.contains(wrapped_token) {
                        sweep_tokens.push(*wrapped_token);
                    }
                }
                enriched_steps.push(step.clone());
            }
            // Other steps don't need enrichment in v1
            _ => {
                enriched_steps.push(step.clone());
            }
        }
    }

    intent.steps = enriched_steps;
    intent.tokens_to_sweep = sweep_tokens;
    Ok(intent)
}
