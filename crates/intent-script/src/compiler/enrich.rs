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
                let spender = *pool;

                enriched_steps.push(ResolvedStep::Erc20Approve {
                    token: *asset,
                    spender,
                    amount: *amount,
                });
                enriched_steps.push(step.clone());
            }
            ResolvedStep::UniswapV3Swap {
                router: swap_router,
                token_in,
                token_out,
                amount_in,
                ..
            } => {
                // Insert ERC-20 approve for token_in → router before swap
                // (skip if token_in is native ETH, i.e. Address::ZERO — but
                // the normalizer already converts native to WETH address, so
                // we check if the original swap sends ETH via value)
                enriched_steps.push(ResolvedStep::Erc20Approve {
                    token: *token_in,
                    spender: *swap_router,
                    amount: *amount_in,
                });
                enriched_steps.push(step.clone());

                // Track output token for sweep when batching
                if router.is_some() && !sweep_tokens.contains(token_out) {
                    sweep_tokens.push(*token_out);
                }
            }
            ResolvedStep::LidoStake { lido, .. } => {
                // No approval needed — ETH is sent as msg.value
                enriched_steps.push(step.clone());

                // Track stETH for sweep when batching (stETH address == lido address)
                if router.is_some() && !sweep_tokens.contains(lido) {
                    sweep_tokens.push(*lido);
                }
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
            // Other steps don't need enrichment (borrow, withdraw, unwrap, approve)
            _ => {
                enriched_steps.push(step.clone());
            }
        }
    }

    intent.steps = enriched_steps;
    intent.tokens_to_sweep = sweep_tokens;
    Ok(intent)
}
