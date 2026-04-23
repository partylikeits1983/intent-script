//! Stage D: Enrich — insert automatically-generated steps like approvals.
//!
//! For example, an Aave V3 supply step needs an ERC-20 approval before it.
//! When a router is available, this stage also:
//! - Inserts `transferFrom(user, router, amount)` to pull user-held tokens into the router
//! - Redirects intermediate token flows through the router (e.g., swap recipient → router)
//! - Tracks which tokens are already in the router to avoid unnecessary transfers
//! - Tracks which tokens need to be swept back to the signer after execution

use alloc::vec::Vec;

use alloy_primitives::{Address, U256};
use hashbrown::{HashMap, HashSet};

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
    // When there's only one user step, the planner will emit a direct SingleTx
    // that bypasses the intent router. In that case we must not redirect the
    // swap recipient to the router (it would strand the output there with no
    // sweep to recover it).
    let is_single_user_step = intent.steps.len() == 1;
    let mut enriched_steps = Vec::new();
    let mut sweep_tokens: Vec<Address> = Vec::new();
    // Track tokens that are already in the router from previous steps.
    // When a step produces tokens into the router (e.g., swap with recipient=router),
    // subsequent steps that consume those tokens don't need a transferFrom.
    let mut tokens_in_router: HashSet<Address> = HashSet::new();
    // Aggregate per-token totals pulled from the signer into the router during
    // the batch. Consumed downstream by the builder to decide which
    // `approve(router, amount)` prerequisite txs to emit.
    let mut required_pulls: HashMap<Address, U256> = HashMap::new();

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
                        *required_pulls.entry(*asset).or_insert(U256::ZERO) += *amount;
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
                native_input,
                ..
            } => {
                // A native-input swap that's the only user step compiles to a
                // direct SingleTx (no intent router in the flow). For that
                // shape, emit the step as-is — no transferFrom, no approval,
                // and recipient stays as the signer so the user receives the
                // output tokens straight from the swap router.
                if *native_input && is_single_user_step {
                    enriched_steps.push(step.clone());
                } else if let Some(router_addr) = router {
                    // For native-input swaps the user pays with msg.value and
                    // the SwapRouter auto-wraps — no ERC-20 pull or approval
                    // is needed. For ERC-20 inputs, pull tokens into the router
                    // (if not already there) and approve the swap router.
                    if !*native_input {
                        if !tokens_in_router.contains(token_in) {
                            enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                                token: *token_in,
                                from: signer,
                                to: router_addr,
                                amount: *amount_in,
                            });
                            *required_pulls.entry(*token_in).or_insert(U256::ZERO) += *amount_in;
                        }
                        enriched_steps.push(ResolvedStep::Erc20Approve {
                            token: *token_in,
                            spender: *swap_router,
                            amount: *amount_in,
                        });
                    }
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
                        native_input: *native_input,
                    });
                    // Track output token as being in the router
                    tokens_in_router.insert(*token_out);
                    if !sweep_tokens.contains(token_out) {
                        sweep_tokens.push(*token_out);
                    }
                } else if *native_input {
                    // No router and native input: emit the swap directly with
                    // recipient=signer (already set by normalize).
                    enriched_steps.push(step.clone());
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
            ResolvedStep::LidoStake { steth, .. } => {
                // No approval or transferFrom needed — ETH is sent as msg.value
                enriched_steps.push(step.clone());

                // Track stETH as being in the router when batching
                if router.is_some() {
                    tokens_in_router.insert(*steth);
                    if !sweep_tokens.contains(steth) {
                        sweep_tokens.push(*steth);
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
                        *required_pulls.entry(*steth).or_insert(U256::ZERO) += *amount;
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
            ResolvedStep::WstETHUnwrap {
                wsteth,
                steth,
                amount,
            } => {
                // When batching via router, pull wstETH from user if not already in router
                if let Some(router_addr) = router {
                    if !tokens_in_router.contains(wsteth) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token: *wsteth,
                            from: signer,
                            to: router_addr,
                            amount: *amount,
                        });
                        *required_pulls.entry(*wsteth).or_insert(U256::ZERO) += *amount;
                    }
                }
                // No approve: `unwrap()` burns the caller's own wstETH balance.
                enriched_steps.push(step.clone());

                // Track stETH as being in the router when batching
                if router.is_some() {
                    tokens_in_router.insert(*steth);
                    if !sweep_tokens.contains(steth) {
                        sweep_tokens.push(*steth);
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
                        *required_pulls.entry(*token_in).or_insert(U256::ZERO) += *amount_in;
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
            ResolvedStep::LidoRequestWithdrawal {
                queue,
                token,
                amounts,
                ..
            } => {
                let total = amounts
                    .iter()
                    .copied()
                    .fold(U256::ZERO, |acc, a| acc.saturating_add(a));

                // When batching via router, pull the stETH/wstETH from user if
                // not already in router so the router can approve the queue.
                if let Some(router_addr) = router {
                    if !tokens_in_router.contains(token) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token: *token,
                            from: signer,
                            to: router_addr,
                            amount: total,
                        });
                        *required_pulls.entry(*token).or_insert(U256::ZERO) += total;
                    }
                }

                // Queue pulls tokens via transferFrom on request; approve first.
                enriched_steps.push(ResolvedStep::Erc20Approve {
                    token: *token,
                    spender: *queue,
                    amount: total,
                });
                enriched_steps.push(step.clone());

                // NFTs mint to `owner` (= signer in v1), not the router, so no
                // sweep_tokens entry is needed for the NFT.
            }
            ResolvedStep::UniswapV3LpMint {
                npm,
                token0,
                token1,
                amount0,
                amount1,
                ..
            } => {
                // Pull + approve BOTH tokens. `amount == 0` is legitimate
                // for a single-sided out-of-range mint — skip the pull and
                // approve for that side so we don't waste gas on no-op txs.
                if let Some(router_addr) = router {
                    if *amount0 > U256::ZERO && !tokens_in_router.contains(token0) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token: *token0,
                            from: signer,
                            to: router_addr,
                            amount: *amount0,
                        });
                        *required_pulls.entry(*token0).or_insert(U256::ZERO) += *amount0;
                    }
                    if *amount1 > U256::ZERO && !tokens_in_router.contains(token1) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token: *token1,
                            from: signer,
                            to: router_addr,
                            amount: *amount1,
                        });
                        *required_pulls.entry(*token1).or_insert(U256::ZERO) += *amount1;
                    }
                }
                if *amount0 > U256::ZERO {
                    enriched_steps.push(ResolvedStep::Erc20Approve {
                        token: *token0,
                        spender: *npm,
                        amount: *amount0,
                    });
                }
                if *amount1 > U256::ZERO {
                    enriched_steps.push(ResolvedStep::Erc20Approve {
                        token: *token1,
                        spender: *npm,
                        amount: *amount1,
                    });
                }
                enriched_steps.push(step.clone());

                // NPM refunds unused `amountXDesired - amountXUsed` back to
                // msg.sender (the router) after mint. Sweep both sides so
                // the dust flows back to the signer.
                if router.is_some() {
                    if *amount0 > U256::ZERO && !sweep_tokens.contains(token0) {
                        sweep_tokens.push(*token0);
                    }
                    if *amount1 > U256::ZERO && !sweep_tokens.contains(token1) {
                        sweep_tokens.push(*token1);
                    }
                }
                // `recipient=signer` on mint → the NFT goes straight to
                // the user; router never holds it, so no NFT sweep.
            }
            ResolvedStep::UniswapV3LpIncrease {
                npm,
                token0,
                token1,
                amount0,
                amount1,
                ..
            } => {
                if let Some(router_addr) = router {
                    if *amount0 > U256::ZERO && !tokens_in_router.contains(token0) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token: *token0,
                            from: signer,
                            to: router_addr,
                            amount: *amount0,
                        });
                        *required_pulls.entry(*token0).or_insert(U256::ZERO) += *amount0;
                    }
                    if *amount1 > U256::ZERO && !tokens_in_router.contains(token1) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token: *token1,
                            from: signer,
                            to: router_addr,
                            amount: *amount1,
                        });
                        *required_pulls.entry(*token1).or_insert(U256::ZERO) += *amount1;
                    }
                }
                if *amount0 > U256::ZERO {
                    enriched_steps.push(ResolvedStep::Erc20Approve {
                        token: *token0,
                        spender: *npm,
                        amount: *amount0,
                    });
                }
                if *amount1 > U256::ZERO {
                    enriched_steps.push(ResolvedStep::Erc20Approve {
                        token: *token1,
                        spender: *npm,
                        amount: *amount1,
                    });
                }
                enriched_steps.push(step.clone());

                // Same dust pattern as mint — sweep leftovers.
                if router.is_some() {
                    if *amount0 > U256::ZERO && !sweep_tokens.contains(token0) {
                        sweep_tokens.push(*token0);
                    }
                    if *amount1 > U256::ZERO && !sweep_tokens.contains(token1) {
                        sweep_tokens.push(*token1);
                    }
                }
            }
            ResolvedStep::UniswapV3LpDecrease { .. } => {
                // Decrease moves liquidity into the position's uncollected
                // fees — no ERC-20 transfers happen yet. The user must
                // follow up with `lp_collect` (same or later intent) to
                // receive the tokens. Router must be NPM-approved for the
                // NFT; the user does that out-of-band.
                enriched_steps.push(step.clone());
            }
            ResolvedStep::UniswapV3LpCollect { token0, token1, .. } => {
                // NPM sends both tokens to `recipient`. Normalize sets that
                // to signer, meaning the tokens bypass the router. No sweep
                // required. Router still needs NFT approval from user.
                enriched_steps.push(step.clone());
                // Belt-and-suspenders: if a future revision switches collect
                // recipient to router, a sweep entry is ready.
                let _ = (token0, token1);
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
                        *required_pulls.entry(*token).or_insert(U256::ZERO) += *amount;
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

    // Sort by token address for stable output (tests + UI rely on it).
    let mut required_pulls_vec: Vec<(Address, U256)> = required_pulls.into_iter().collect();
    required_pulls_vec.sort_by_key(|(addr, _)| *addr);
    intent.required_pulls = required_pulls_vec;

    Ok(intent)
}
