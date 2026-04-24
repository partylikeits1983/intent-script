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

    enrich_steps(
        &intent.steps,
        router,
        signer,
        is_single_user_step,
        &mut enriched_steps,
        &mut sweep_tokens,
        &mut tokens_in_router,
        &mut required_pulls,
    )?;

    intent.steps = enriched_steps;
    intent.tokens_to_sweep = sweep_tokens;

    // Sort by token address for stable output (tests + UI rely on it).
    let mut required_pulls_vec: Vec<(Address, U256)> = required_pulls.into_iter().collect();
    required_pulls_vec.sort_by_key(|(addr, _)| *addr);
    intent.required_pulls = required_pulls_vec;

    Ok(intent)
}

#[allow(clippy::too_many_arguments)]
fn enrich_steps(
    input_steps: &[ResolvedStep],
    router: Option<Address>,
    signer: Address,
    is_single_user_step: bool,
    enriched_steps: &mut Vec<ResolvedStep>,
    sweep_tokens: &mut Vec<Address>,
    tokens_in_router: &mut HashSet<Address>,
    required_pulls: &mut HashMap<Address, U256>,
) -> Result<()> {
    for step in input_steps {
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
            ResolvedStep::AaveV3Repay {
                pool,
                asset,
                amount,
                ..
            } => {
                // Repay pulls `amount` of `asset` from msg.sender (router) via
                // transferFrom inside Aave, so we need the token in router and
                // an approval for the pool.
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
                // asset back to the user after execution — and mark it as in-router so
                // downstream steps in the same batch consume it from the router rather
                // than re-pulling from the signer (crucial for flashloan-assisted loops).
                if router.is_some() {
                    tokens_in_router.insert(*asset);
                    if !sweep_tokens.contains(asset) {
                        sweep_tokens.push(*asset);
                    }
                }
                enriched_steps.push(step.clone());
            }
            ResolvedStep::MorphoSupply {
                pool,
                market_params,
                amount,
                ..
            } => {
                // Supplying the loan asset: pull from user + approve pool.
                let token = market_params.loan_token;
                if let Some(router_addr) = router {
                    if !tokens_in_router.contains(&token) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token,
                            from: signer,
                            to: router_addr,
                            amount: *amount,
                        });
                        *required_pulls.entry(token).or_insert(U256::ZERO) += *amount;
                    }
                }
                enriched_steps.push(ResolvedStep::Erc20Approve {
                    token,
                    spender: *pool,
                    amount: *amount,
                });
                enriched_steps.push(step.clone());
            }
            ResolvedStep::MorphoSupplyCollat {
                pool,
                market_params,
                amount,
                ..
            } => {
                let token = market_params.collateral_token;
                if let Some(router_addr) = router {
                    if !tokens_in_router.contains(&token) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token,
                            from: signer,
                            to: router_addr,
                            amount: *amount,
                        });
                        *required_pulls.entry(token).or_insert(U256::ZERO) += *amount;
                    }
                }
                enriched_steps.push(ResolvedStep::Erc20Approve {
                    token,
                    spender: *pool,
                    amount: *amount,
                });
                enriched_steps.push(step.clone());
            }
            ResolvedStep::MorphoRepay {
                pool,
                market_params,
                amount,
                ..
            } => {
                let token = market_params.loan_token;
                if let Some(router_addr) = router {
                    if !tokens_in_router.contains(&token) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token,
                            from: signer,
                            to: router_addr,
                            amount: *amount,
                        });
                        *required_pulls.entry(token).or_insert(U256::ZERO) += *amount;
                    }
                }
                enriched_steps.push(ResolvedStep::Erc20Approve {
                    token,
                    spender: *pool,
                    amount: *amount,
                });
                enriched_steps.push(step.clone());
            }
            ResolvedStep::MorphoBorrow {
                pool,
                market_params,
                amount,
                on_behalf,
                ..
            } => {
                // When batching, redirect receiver to the router so the loan
                // token can participate in downstream steps, then sweep back
                // to the signer. Without a router, leave the tokens flowing
                // straight to the signer.
                let token = market_params.loan_token;
                let receiver = router.unwrap_or(*on_behalf);
                enriched_steps.push(ResolvedStep::MorphoBorrow {
                    pool: *pool,
                    market_params: market_params.clone(),
                    amount: *amount,
                    on_behalf: *on_behalf,
                    receiver,
                });
                if router.is_some() {
                    tokens_in_router.insert(token);
                    if !sweep_tokens.contains(&token) {
                        sweep_tokens.push(token);
                    }
                }
            }
            ResolvedStep::MorphoWithdraw {
                pool,
                market_params,
                amount,
                on_behalf,
                ..
            } => {
                let token = market_params.loan_token;
                let receiver = router.unwrap_or(*on_behalf);
                enriched_steps.push(ResolvedStep::MorphoWithdraw {
                    pool: *pool,
                    market_params: market_params.clone(),
                    amount: *amount,
                    on_behalf: *on_behalf,
                    receiver,
                });
                if router.is_some() {
                    tokens_in_router.insert(token);
                    if !sweep_tokens.contains(&token) {
                        sweep_tokens.push(token);
                    }
                }
            }
            ResolvedStep::MorphoWithdrawCollat {
                pool,
                market_params,
                amount,
                on_behalf,
                ..
            } => {
                let token = market_params.collateral_token;
                let receiver = router.unwrap_or(*on_behalf);
                enriched_steps.push(ResolvedStep::MorphoWithdrawCollat {
                    pool: *pool,
                    market_params: market_params.clone(),
                    amount: *amount,
                    on_behalf: *on_behalf,
                    receiver,
                });
                if router.is_some() {
                    tokens_in_router.insert(token);
                    if !sweep_tokens.contains(&token) {
                        sweep_tokens.push(token);
                    }
                }
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
            ResolvedStep::AcrossDepositV3 {
                spoke_pool,
                input_token,
                input_amount,
                ..
            } => {
                // Pull input_token from user (if not already in router) and
                // approve the SpokePool for input_amount. No sweep — the
                // tokens are in flight cross-chain, not coming back.
                if let Some(router_addr) = router {
                    if !tokens_in_router.contains(input_token) {
                        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
                            token: *input_token,
                            from: signer,
                            to: router_addr,
                            amount: *input_amount,
                        });
                        *required_pulls.entry(*input_token).or_insert(U256::ZERO) += *input_amount;
                    }
                }
                enriched_steps.push(ResolvedStep::Erc20Approve {
                    token: *input_token,
                    spender: *spoke_pool,
                    amount: *input_amount,
                });
                enriched_steps.push(step.clone());
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
            ResolvedStep::BalancerFlashloan {
                vault,
                tokens,
                amounts,
                inner_steps,
            } => {
                // Enrich the inner pipeline recursively with a fresh local
                // state, seeded to reflect that Balancer transfers the
                // flashloaned tokens onto the router before calling back.
                // Downstream (sweep_tokens, required_pulls) from the inner
                // pass merges into the OUTER state:
                //   - inner sweep_tokens minus the flashloaned tokens → outer
                //     sweep list (leftover dust after repayment flows back).
                //   - inner required_pulls → outer required_pulls (user may
                //     still contribute their own collateral alongside the
                //     flashloan).
                //   - inner tokens_in_router end-state is discarded — after
                //     `receiveFlashLoan` returns, only non-flashloan dust is
                //     still in the router.
                let mut inner_enriched: Vec<ResolvedStep> = Vec::new();
                let mut inner_sweep: Vec<Address> = Vec::new();
                let mut inner_in_router: HashSet<Address> = HashSet::new();
                for t in tokens {
                    inner_in_router.insert(*t);
                }
                let mut inner_pulls: HashMap<Address, U256> = HashMap::new();
                enrich_steps(
                    inner_steps,
                    router,
                    signer,
                    false, // inner pipeline always runs via router
                    &mut inner_enriched,
                    &mut inner_sweep,
                    &mut inner_in_router,
                    &mut inner_pulls,
                )?;

                // Emit a single outer BalancerFlashloan carrying the enriched
                // inner pipeline. Lowering will ABI-encode inner_steps into
                // `userData` for the vault.flashLoan call.
                enriched_steps.push(ResolvedStep::BalancerFlashloan {
                    vault: *vault,
                    tokens: tokens.clone(),
                    amounts: amounts.clone(),
                    inner_steps: inner_enriched,
                });

                // Merge inner pulls into outer (user approvals cover both).
                for (t, a) in inner_pulls {
                    *required_pulls.entry(t).or_insert(U256::ZERO) += a;
                }
                // Dust sweep: inner non-flashloan tokens that ended up in the
                // router are still there after the callback returns.
                for t in inner_sweep {
                    let is_flashloaned = tokens.contains(&t);
                    if !is_flashloaned && !sweep_tokens.contains(&t) {
                        sweep_tokens.push(t);
                    }
                }
                // After the flashloan returns, any non-flashloan inner-produced
                // token is still in the router balance; surface to the outer
                // tokens_in_router so downstream outer steps can re-use them.
                for t in inner_in_router {
                    if !tokens.contains(&t) {
                        tokens_in_router.insert(t);
                    }
                }
            }
            ResolvedStep::Erc20TransferFrom {
                token,
                from,
                amount,
                ..
            } => {
                // Pre-existing transferFrom (emitted by leverage sugar to pull
                // user equity into the router before a supply). Track it in
                // required_pulls so the builder surfaces a prerequisite
                // approve(router, amount) for the signer, and mark the token
                // as in-router so subsequent inner steps don't duplicate the
                // pull. Auto-inserted transferFroms never reach this arm
                // because they're inserted via explicit pushes in other arms.
                if *from == signer {
                    *required_pulls.entry(*token).or_insert(U256::ZERO) += *amount;
                }
                tokens_in_router.insert(*token);
                enriched_steps.push(step.clone());
            }
            // SendEth, SendErc721, and other steps don't need enrichment
            _ => {
                enriched_steps.push(step.clone());
            }
        }
    }

    Ok(())
}
