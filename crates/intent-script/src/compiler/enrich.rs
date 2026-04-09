//! Stage D: Enrich — insert automatically-generated steps like approvals.
//!
//! For example, an Aave V3 supply step needs an ERC-20 approval before it.
//! When a router is available, this stage also:
//! - Inserts `transferFrom(user, router, amount)` to pull user-held tokens into the router
//! - Redirects intermediate token flows through the router (e.g., swap recipient → router)
//! - Tracks which tokens are already in the router to avoid unnecessary transfers
//! - Tracks which tokens need to be swept back to the signer after execution

use alloc::vec::Vec;

use alloy_primitives::Address;
use hashbrown::HashSet;

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
    let signer = intent.signer;
    let mut enriched_steps = Vec::new();
    let mut sweep_tokens: Vec<Address> = Vec::new();
    // Track tokens that are already in the router from previous steps.
    // When a step produces tokens into the router (e.g., swap with recipient=router),
    // subsequent steps that consume those tokens don't need a transferFrom.
    let mut tokens_in_router: HashSet<Address> = HashSet::new();

    for step in &intent.steps {
        match step {
            ResolvedStep::AaveV3Supply {
                pool,
                asset,
                amount,
                ..
            } => {
                // When batching via router, pull tokens from user if not already in router
                if let Some(router_addr) = router {
                    if !tokens_in_router.contains(asset) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token: *asset,
                            from: signer,
                            to: router_addr,
                            amount: *amount,
                        });
                    }
                }
                // Insert ERC-20 approve before supply.
                enriched_steps.push(ResolvedStep::Erc20Approve {
                    token: *asset,
                    spender: *pool,
                    amount: *amount,
                });
                enriched_steps.push(step.clone());
            }
            ResolvedStep::AaveV3Borrow { asset, .. } => {
                // Borrow doesn't need transferFrom (no input tokens consumed from user).
                // But when batching via router, Aave V3 sends borrowed tokens to msg.sender
                // (the router), not to onBehalfOf (the user). So we must sweep the borrowed
                // asset back to the user after execution.
                if router.is_some() {
                    if !sweep_tokens.contains(asset) {
                        sweep_tokens.push(*asset);
                    }
                }
                enriched_steps.push(step.clone());
            }
            ResolvedStep::UniswapV3Swap {
                router: swap_router,
                token_in,
                token_out,
                amount_in,
                fee,
                deadline,
                amount_out_minimum,
                ..
            } => {
                if let Some(router_addr) = router {
                    // Pull token_in from user if not already in router
                    if !tokens_in_router.contains(token_in) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token: *token_in,
                            from: signer,
                            to: router_addr,
                            amount: *amount_in,
                        });
                    }
                    // Approve swap router to spend token_in
                    enriched_steps.push(ResolvedStep::Erc20Approve {
                        token: *token_in,
                        spender: *swap_router,
                        amount: *amount_in,
                    });
                    // Redirect recipient to router so output tokens stay in router
                    enriched_steps.push(ResolvedStep::UniswapV3Swap {
                        router: *swap_router,
                        token_in: *token_in,
                        token_out: *token_out,
                        amount_in: *amount_in,
                        fee: *fee,
                        recipient: router_addr,
                        deadline: *deadline,
                        amount_out_minimum: *amount_out_minimum,
                    });
                    // Track output token as being in the router
                    tokens_in_router.insert(*token_out);
                    if !sweep_tokens.contains(token_out) {
                        sweep_tokens.push(*token_out);
                    }
                } else {
                    // No router — standard approve + swap
                    enriched_steps.push(ResolvedStep::Erc20Approve {
                        token: *token_in,
                        spender: *swap_router,
                        amount: *amount_in,
                    });
                    enriched_steps.push(step.clone());
                }
            }
            ResolvedStep::LidoStake { lido, .. } => {
                // No approval or transferFrom needed — ETH is sent as msg.value
                enriched_steps.push(step.clone());

                // Track stETH as being in the router when batching
                // (stETH address == lido address)
                if router.is_some() {
                    tokens_in_router.insert(*lido);
                    if !sweep_tokens.contains(lido) {
                        sweep_tokens.push(*lido);
                    }
                }
            }
            ResolvedStep::Wrap { wrapped_token, .. } => {
                // No transferFrom needed — ETH is sent as msg.value
                // Wrap produces an ERC-20 token that stays in the router
                enriched_steps.push(step.clone());

                if router.is_some() {
                    tokens_in_router.insert(*wrapped_token);
                    if !sweep_tokens.contains(wrapped_token) {
                        sweep_tokens.push(*wrapped_token);
                    }
                }
            }
            ResolvedStep::WstETHWrap {
                wsteth,
                steth,
                amount,
            } => {
                // When batching via router, pull stETH from user if not already in router
                if let Some(router_addr) = router {
                    if !tokens_in_router.contains(steth) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token: *steth,
                            from: signer,
                            to: router_addr,
                            amount: *amount,
                        });
                    }
                }
                // Insert ERC-20 approve for stETH → wstETH before the wrap
                enriched_steps.push(ResolvedStep::Erc20Approve {
                    token: *steth,
                    spender: *wsteth,
                    amount: *amount,
                });
                enriched_steps.push(step.clone());

                // Track wstETH as being in the router when batching
                if router.is_some() {
                    tokens_in_router.insert(*wsteth);
                    if !sweep_tokens.contains(wsteth) {
                        sweep_tokens.push(*wsteth);
                    }
                }
            }
            ResolvedStep::OneInchSwap {
                router: oneinch_router,
                token_in,
                token_out,
                amount_in,
                ..
            } => {
                // When batching via router, pull token_in from user if not already in router
                if let Some(router_addr) = router {
                    if !tokens_in_router.contains(token_in) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token: *token_in,
                            from: signer,
                            to: router_addr,
                            amount: *amount_in,
                        });
                    }
                }
                // Insert ERC-20 approve for token_in → 1inch router before swap
                enriched_steps.push(ResolvedStep::Erc20Approve {
                    token: *token_in,
                    spender: *oneinch_router,
                    amount: *amount_in,
                });
                enriched_steps.push(step.clone());

                // Track output token for sweep when batching
                if router.is_some() {
                    tokens_in_router.insert(*token_out);
                    if !sweep_tokens.contains(token_out) {
                        sweep_tokens.push(*token_out);
                    }
                }
            }
            ResolvedStep::SendErc20 { token, amount, .. } => {
                // When batching via router, pull tokens from user if not already in router
                if let Some(router_addr) = router {
                    if !tokens_in_router.contains(token) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token: *token,
                            from: signer,
                            to: router_addr,
                            amount: *amount,
                        });
                    }
                }
                enriched_steps.push(step.clone());
            }
            // SendEth, SendErc721, and other steps don't need enrichment
            _ => {
                enriched_steps.push(step.clone());
            }
        }
    }

    intent.steps = enriched_steps;
    intent.tokens_to_sweep = sweep_tokens;
    Ok(intent)
}
