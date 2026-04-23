//! Stage B: Normalize — convert public AST into canonical IR.
//!
//! Resolves aliases to addresses, parses human-readable amounts to U256,
//! and maps protocol names to concrete deployment addresses.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use alloy_primitives::{Address, Bytes, U256};
use hashbrown::HashMap;

use crate::error::{CompileError, Result, closest_match};
use crate::ir::{ResolvedBalances, ResolvedIntent, ResolvedStep, step_produces};
use crate::registry::RegistryContext;
use crate::schema::{IntentScript, Step, SwapStep};

/// Collect the list of known protocol aliases for inclusion in `UnknownProtocol` errors.
fn known_protocols(registry: &RegistryContext) -> Vec<String> {
    let mut out: Vec<String> = registry.protocols.keys().cloned().collect();
    out.sort();
    out
}

/// Default swap deadline: 20 minutes from current_timestamp.
const DEFAULT_SWAP_DEADLINE_SECS: u64 = 1200;
/// Default intent deadline: 30 minutes from current_timestamp.
const DEFAULT_INTENT_DEADLINE_SECS: u64 = 1800;

/// Result of normalization: the resolved intent plus any warnings.
pub struct NormalizeResult {
    pub intent: ResolvedIntent,
    pub warnings: Vec<String>,
}

/// Normalize a parsed intent script into the canonical IR.
pub fn normalize(script: &IntentScript, registry: &RegistryContext) -> Result<NormalizeResult> {
    let signer = parse_address(&script.from)?;
    let mut warnings = Vec::new();

    // Compute effective intent deadline (Task 2).
    // A deadline of 0 is only a problem when the intent is ultimately batched
    // and relayed via `executeSigned` (which rejects expired/missing deadlines).
    // The warning is emitted in the top-level `compile()` after planning so it
    // doesn't fire spuriously on single-tx outputs.
    let effective_deadline = match script.deadline {
        Some(d) if d > 0 => d,
        _ => match script.current_timestamp {
            Some(ts) => ts + DEFAULT_INTENT_DEADLINE_SECS,
            None => 0, // backward compat when no timestamp provided
        },
    };

    let mut steps = Vec::new();
    for step in &script.steps {
        let resolved = normalize_step(step, signer, registry, &mut warnings, script, &steps)?;
        steps.push(resolved);
    }

    // Defense in depth against a common LLM error: emitting `wrap ETH→WETH`
    // right before a step that already accepts native ETH (Lido submit,
    // Uniswap V3 SwapRouter with tokenIn=WETH). The wrap is pure waste —
    // it costs gas, creates a redundant msg.value hop, and the wrapped WETH
    // ends up orphaned because the next step consumes ETH, not WETH. We
    // rewrite the pair in place rather than erroring, so chats keep flowing.
    elide_wasteful_wraps(&mut steps, registry, &mut warnings)?;

    // Parse optional user balances
    let user_balances = if let Some(ref balances) = script.balances {
        Some(normalize_balances(balances, registry)?)
    } else {
        None
    };

    Ok(NormalizeResult {
        intent: ResolvedIntent {
            chain_id: registry.chain.chain_id,
            signer,
            steps,
            tokens_to_sweep: Vec::new(),
            nonce: script.nonce.unwrap_or(0),
            deadline: effective_deadline,
            user_balances,
            required_pulls: Vec::new(),
            fee_bps: registry.fee_bps(),
        },
        warnings,
    })
}

fn normalize_step(
    step: &Step,
    signer: Address,
    registry: &RegistryContext,
    warnings: &mut Vec<String>,
    script: &IntentScript,
    previous_steps: &[ResolvedStep],
) -> Result<ResolvedStep> {
    match step {
        Step::Wrap(w) => {
            if w.asset == "stETH" {
                // Wrap stETH → wstETH via wstETH.wrap(uint256)
                let lido_protocol = registry.protocols.get("lido").ok_or_else(|| {
                    CompileError::UnknownProtocol {
                        protocol: "lido".to_string(),
                        network: registry.network.clone(),
                        available: known_protocols(registry),
                    }
                })?;

                let wsteth_addr = lido_protocol.contracts.get("wsteth").ok_or_else(|| {
                    CompileError::Adapter(
                        "Protocol 'lido' has no 'wsteth' contract configured".to_string(),
                    )
                })?;
                let wsteth = parse_address(wsteth_addr)?;

                let steth_addr = lido_protocol.contracts.get("steth").ok_or_else(|| {
                    CompileError::Adapter(
                        "Protocol 'lido' has no 'steth' contract configured".to_string(),
                    )
                })?;
                let steth = parse_address(steth_addr)?;

                // "all" resolves against the stETH address (the thing being consumed).
                let decimals = resolve_asset_decimals(&w.asset, registry)?;
                let amount = resolve_amount_or_all(
                    &w.amount,
                    decimals,
                    steth,
                    previous_steps,
                    &w.asset,
                    registry,
                )?;

                Ok(ResolvedStep::WstETHWrap {
                    wsteth,
                    steth,
                    amount,
                })
            } else {
                // Wrap: asset should be native (ETH) or we wrap to the wrapped native
                let wrapped_token =
                    resolve_asset_address(&registry.chain.wrapped_native, registry)?;
                let decimals = resolve_asset_decimals(&w.asset, registry)?;
                let amount = resolve_amount_or_all(
                    &w.amount,
                    decimals,
                    wrapped_token,
                    previous_steps,
                    &w.asset,
                    registry,
                )?;
                Ok(ResolvedStep::Wrap {
                    wrapped_token,
                    amount,
                })
            }
        }
        Step::Unwrap(u) => {
            if u.asset == "wstETH" {
                // Unwrap wstETH → stETH via wstETH.unwrap(uint256)
                let lido_protocol = registry.protocols.get("lido").ok_or_else(|| {
                    CompileError::UnknownProtocol {
                        protocol: "lido".to_string(),
                        network: registry.network.clone(),
                        available: known_protocols(registry),
                    }
                })?;

                let wsteth_addr = lido_protocol.contracts.get("wsteth").ok_or_else(|| {
                    CompileError::Adapter(
                        "Protocol 'lido' has no 'wsteth' contract configured".to_string(),
                    )
                })?;
                let wsteth = parse_address(wsteth_addr)?;

                let steth_addr = lido_protocol.contracts.get("steth").ok_or_else(|| {
                    CompileError::Adapter(
                        "Protocol 'lido' has no 'steth' contract configured".to_string(),
                    )
                })?;
                let steth = parse_address(steth_addr)?;

                // "all" resolves against the wstETH address (the thing being consumed).
                let decimals = resolve_asset_decimals(&u.asset, registry)?;
                let amount = resolve_amount_or_all(
                    &u.amount,
                    decimals,
                    wsteth,
                    previous_steps,
                    &u.asset,
                    registry,
                )?;

                Ok(ResolvedStep::WstETHUnwrap {
                    wsteth,
                    steth,
                    amount,
                })
            } else {
                // Unwrap: asset should be WETH or the wrapped native
                let wrapped_token = resolve_asset_address(&u.asset, registry)?;
                let decimals = resolve_asset_decimals(&u.asset, registry)?;
                let amount = resolve_amount_or_all(
                    &u.amount,
                    decimals,
                    wrapped_token,
                    previous_steps,
                    &u.asset,
                    registry,
                )?;
                Ok(ResolvedStep::Unwrap {
                    wrapped_token,
                    amount,
                })
            }
        }
        Step::Deposit(d) => {
            let asset = resolve_asset_address(&d.asset, registry)?;
            let decimals = resolve_asset_decimals(&d.asset, registry)?;
            let amount = resolve_amount_or_all(
                &d.amount,
                decimals,
                asset,
                previous_steps,
                &d.asset,
                registry,
            )?;

            let protocol =
                registry
                    .protocols
                    .get(&d.into)
                    .ok_or_else(|| CompileError::UnknownProtocol {
                        protocol: d.into.clone(),
                        network: registry.network.clone(),
                        available: known_protocols(registry),
                    })?;

            let pool_addr = protocol.contracts.get("pool").ok_or_else(|| {
                CompileError::Adapter(format!(
                    "Protocol '{}' has no 'pool' contract configured",
                    d.into
                ))
            })?;
            let pool = parse_address(pool_addr)?;

            Ok(ResolvedStep::AaveV3Supply {
                pool,
                asset,
                amount,
                on_behalf_of: signer,
            })
        }
        Step::Borrow(b) => {
            let asset = resolve_asset_address(&b.asset, registry)?;
            let decimals = resolve_asset_decimals(&b.asset, registry)?;
            let amount = resolve_amount_or_all(
                &b.amount,
                decimals,
                asset,
                previous_steps,
                &b.asset,
                registry,
            )?;

            let protocol =
                registry
                    .protocols
                    .get(&b.from)
                    .ok_or_else(|| CompileError::UnknownProtocol {
                        protocol: b.from.clone(),
                        network: registry.network.clone(),
                        available: known_protocols(registry),
                    })?;

            let pool_addr = protocol.contracts.get("pool").ok_or_else(|| {
                CompileError::Adapter(format!(
                    "Protocol '{}' has no 'pool' contract configured",
                    b.from
                ))
            })?;
            let pool = parse_address(pool_addr)?;

            Ok(ResolvedStep::AaveV3Borrow {
                pool,
                asset,
                amount,
                rate_mode: 2, // Variable rate by default
                on_behalf_of: signer,
            })
        }
        Step::Withdraw(w) => {
            let asset = resolve_asset_address(&w.asset, registry)?;
            let decimals = resolve_asset_decimals(&w.asset, registry)?;
            let amount = resolve_amount_or_all(
                &w.amount,
                decimals,
                asset,
                previous_steps,
                &w.asset,
                registry,
            )?;

            let protocol =
                registry
                    .protocols
                    .get(&w.from)
                    .ok_or_else(|| CompileError::UnknownProtocol {
                        protocol: w.from.clone(),
                        network: registry.network.clone(),
                        available: known_protocols(registry),
                    })?;

            let pool_addr = protocol.contracts.get("pool").ok_or_else(|| {
                CompileError::Adapter(format!(
                    "Protocol '{}' has no 'pool' contract configured",
                    w.from
                ))
            })?;
            let pool = parse_address(pool_addr)?;

            Ok(ResolvedStep::AaveV3Withdraw {
                pool,
                asset,
                amount,
                to: signer,
            })
        }
        Step::Swap(s) => {
            let token_in = resolve_asset_address(&s.from, registry)?;
            let token_in_decimals = resolve_asset_decimals(&s.from, registry)?;
            let amount_in = resolve_amount_or_all(
                &s.amount,
                token_in_decimals,
                token_in,
                previous_steps,
                &s.from,
                registry,
            )?;
            let token_out = resolve_asset_address(&s.to, registry)?;

            // Determine routing provider
            let via = s.via.as_deref().unwrap_or("uniswap");

            // Compute effective deadline for this swap (Task 2)
            let effective_deadline = match script.deadline {
                Some(d) if d > 0 => d,
                _ => match script.current_timestamp {
                    Some(ts) => ts + DEFAULT_INTENT_DEADLINE_SECS,
                    None => 0,
                },
            };

            match via {
                "uniswap" | "" => {
                    // Parse optional fee tier (default 3000 = 0.3%)
                    let fee: u32 = s.fee.as_deref().unwrap_or("3000").parse().map_err(|_| {
                        CompileError::InvalidAmount(format!(
                            "Invalid fee tier: {}",
                            s.fee.as_deref().unwrap_or("")
                        ))
                    })?;

                    // Look up Uniswap V3 router from protocol config
                    let protocol = registry.protocols.get("uniswap").ok_or_else(|| {
                        CompileError::UnknownProtocol {
                            protocol: "uniswap".to_string(),
                            network: registry.network.clone(),
                            available: known_protocols(registry),
                        }
                    })?;

                    let router_addr = protocol.contracts.get("router").ok_or_else(|| {
                        CompileError::Adapter(
                            "Protocol 'uniswap' has no 'router' contract configured".to_string(),
                        )
                    })?;
                    let router = parse_address(router_addr)?;

                    // If swapping from native ETH, put the wrapped-native
                    // address in the calldata (that's what SwapRouter expects
                    // for tokenIn). The `native_input` flag below tells the
                    // rest of the pipeline to pay `amount_in` as msg.value
                    // instead of emitting an ERC-20 transferFrom/approve —
                    // the SwapRouter's `pay()` helper wraps msg.value ETH
                    // into WETH on the fly when tokenIn == WETH9.
                    let native_input = token_in == Address::ZERO;
                    let effective_token_in = if native_input {
                        resolve_asset_address(&registry.chain.wrapped_native, registry)?
                    } else {
                        token_in
                    };

                    // If swapping to native ETH, use WETH address as token_out
                    let effective_token_out = if token_out == Address::ZERO {
                        resolve_asset_address(&registry.chain.wrapped_native, registry)?
                    } else {
                        token_out
                    };

                    // Compute amount_out_minimum from slippage params
                    let amount_out_minimum =
                        compute_amount_out_minimum(s, amount_in, &s.to, registry, warnings)?;

                    // Compute swap deadline (Task 2)
                    let swap_deadline = match s.deadline {
                        Some(d) => d,
                        None => {
                            if effective_deadline > 0 {
                                effective_deadline
                            } else {
                                match script.current_timestamp {
                                    Some(ts) => ts + DEFAULT_SWAP_DEADLINE_SECS,
                                    None => u64::MAX, // backward compat
                                }
                            }
                        }
                    };

                    Ok(ResolvedStep::UniswapV3Swap {
                        router,
                        token_in: effective_token_in,
                        token_out: effective_token_out,
                        amount_in,
                        fee,
                        recipient: signer,
                        deadline: U256::from(swap_deadline),
                        amount_out_minimum,
                        native_input,
                    })
                }
                "1inch" => {
                    // Require pre-fetched calldata
                    let calldata_hex = s.calldata.as_deref().ok_or_else(|| {
                        CompileError::Adapter(
                            "1inch swap requires 'calldata' field with pre-fetched calldata"
                                .to_string(),
                        )
                    })?;

                    // Parse hex calldata (strip 0x prefix if present)
                    let hex_str = calldata_hex.strip_prefix("0x").unwrap_or(calldata_hex);
                    let calldata_bytes = hex_decode(hex_str).map_err(|_| {
                        CompileError::Adapter(format!(
                            "Invalid hex calldata for 1inch swap: {}",
                            calldata_hex
                        ))
                    })?;

                    // Look up 1inch router from protocol config
                    let protocol = registry.protocols.get("1inch").ok_or_else(|| {
                        CompileError::UnknownProtocol {
                            protocol: "1inch".to_string(),
                            network: registry.network.clone(),
                            available: known_protocols(registry),
                        }
                    })?;

                    let router_addr = protocol.contracts.get("router").ok_or_else(|| {
                        CompileError::Adapter(
                            "Protocol '1inch' has no 'router' contract configured".to_string(),
                        )
                    })?;
                    let router = parse_address(router_addr)?;

                    // If swapping from native ETH, use WETH address as token_in
                    let effective_token_in = if token_in == Address::ZERO {
                        resolve_asset_address(&registry.chain.wrapped_native, registry)?
                    } else {
                        token_in
                    };

                    let effective_token_out = if token_out == Address::ZERO {
                        resolve_asset_address(&registry.chain.wrapped_native, registry)?
                    } else {
                        token_out
                    };

                    Ok(ResolvedStep::OneInchSwap {
                        router,
                        token_in: effective_token_in,
                        token_out: effective_token_out,
                        amount_in,
                        calldata: Bytes::from(calldata_bytes),
                    })
                }
                other => Err(CompileError::UnsupportedStep(format!(
                    "swap via '{}' is not supported; use 'uniswap' or '1inch'",
                    other
                ))),
            }
        }
        Step::Stake(s) => {
            let decimals = resolve_asset_decimals(&s.asset, registry)?;
            let amount = resolve_amount_or_all(
                &s.amount,
                decimals,
                Address::ZERO,
                previous_steps,
                &s.asset,
                registry,
            )?;

            let protocol =
                registry
                    .protocols
                    .get(&s.into)
                    .ok_or_else(|| CompileError::UnknownProtocol {
                        protocol: s.into.clone(),
                        network: registry.network.clone(),
                        available: known_protocols(registry),
                    })?;

            let contract_key = match s.into.as_str() {
                "lido" => "steth",
                _ => "pool",
            };

            let contract_addr = protocol.contracts.get(contract_key).ok_or_else(|| {
                CompileError::Adapter(format!(
                    "Protocol '{}' has no '{}' contract configured",
                    s.into, contract_key
                ))
            })?;
            let contract = parse_address(contract_addr)?;

            match s.into.as_str() {
                "lido" => Ok(ResolvedStep::LidoStake {
                    steth: contract,
                    amount,
                    referral: Address::ZERO,
                }),
                _ => Err(CompileError::UnsupportedStep(format!(
                    "staking into '{}' is not yet supported",
                    s.into
                ))),
            }
        }
        Step::RequestWithdrawal(r) => {
            if r.from != "lido" {
                return Err(CompileError::UnsupportedStep(format!(
                    "request_withdrawal from '{}' is not supported (only 'lido')",
                    r.from
                )));
            }

            let is_wsteth = match r.asset.as_str() {
                "stETH" => false,
                "wstETH" => true,
                other => {
                    return Err(CompileError::Validation(format!(
                        "request_withdrawal asset must be 'stETH' or 'wstETH' (got '{}')",
                        other
                    )));
                }
            };

            let lido_protocol =
                registry
                    .protocols
                    .get("lido")
                    .ok_or_else(|| CompileError::UnknownProtocol {
                        protocol: "lido".to_string(),
                        network: registry.network.clone(),
                        available: known_protocols(registry),
                    })?;

            let queue_addr = lido_protocol
                .contracts
                .get("withdrawal_queue")
                .ok_or_else(|| {
                    CompileError::Adapter(
                        "Protocol 'lido' has no 'withdrawal_queue' contract configured".to_string(),
                    )
                })?;
            let queue = parse_address(queue_addr)?;

            let token_key = if is_wsteth { "wsteth" } else { "steth" };
            let token_addr = lido_protocol.contracts.get(token_key).ok_or_else(|| {
                CompileError::Adapter(format!(
                    "Protocol 'lido' has no '{}' contract configured",
                    token_key
                ))
            })?;
            let token = parse_address(token_addr)?;

            let decimals = resolve_asset_decimals(&r.asset, registry)?;
            let mut amounts = Vec::with_capacity(r.amounts.len());
            for a in &r.amounts {
                // Explicit amounts only for request_withdrawal: "all" would need
                // to index into per-previous-step produces/consumes, which the
                // v1 multi-amount flow doesn't support.
                if a == "all" {
                    return Err(CompileError::InvalidAmount(
                        "request_withdrawal amounts must be explicit (not 'all')".to_string(),
                    ));
                }
                amounts.push(parse_amount(a, decimals)?);
            }

            Ok(ResolvedStep::LidoRequestWithdrawal {
                queue,
                token,
                is_wsteth,
                amounts,
                owner: signer,
            })
        }
        Step::ClaimWithdrawal(c) => {
            if c.protocol != "lido" {
                return Err(CompileError::UnsupportedStep(format!(
                    "claim_withdrawal for protocol '{}' is not supported (only 'lido')",
                    c.protocol
                )));
            }

            let lido_protocol =
                registry
                    .protocols
                    .get("lido")
                    .ok_or_else(|| CompileError::UnknownProtocol {
                        protocol: "lido".to_string(),
                        network: registry.network.clone(),
                        available: known_protocols(registry),
                    })?;

            let queue_addr = lido_protocol
                .contracts
                .get("withdrawal_queue")
                .ok_or_else(|| {
                    CompileError::Adapter(
                        "Protocol 'lido' has no 'withdrawal_queue' contract configured".to_string(),
                    )
                })?;
            let queue = parse_address(queue_addr)?;

            let request_ids = c.request_ids.iter().copied().map(U256::from).collect();
            let hints = c.hints.iter().copied().map(U256::from).collect();

            Ok(ResolvedStep::LidoClaimWithdrawal {
                queue,
                request_ids,
                hints,
            })
        }
        Step::LpMint(m) => {
            if m.protocol != "uniswap" {
                return Err(CompileError::UnsupportedStep(format!(
                    "lp_mint for protocol '{}' is not supported (only 'uniswap')",
                    m.protocol
                )));
            }

            let fee = parse_uniswap_fee_tier(&m.fee)?;
            let tick_spacing = uniswap_tick_spacing(fee);
            validate_uniswap_tick_range(m.tick_lower, m.tick_upper, tick_spacing)?;

            let npm = resolve_uniswap_position_manager(registry)?;

            // Lexicographically sort (token0, token1) by address, swapping
            // paired amount / min fields in lockstep so the NPM sees the
            // pair in canonical order regardless of DSL-side ordering.
            let (token0_alias, token1_alias, amount0_raw, amount1_raw, min0_raw, min1_raw) = {
                let token0_addr = resolve_asset_address(&m.token0, registry)?;
                let token1_addr = resolve_asset_address(&m.token1, registry)?;
                if token0_addr <= token1_addr {
                    (
                        m.token0.as_str(),
                        m.token1.as_str(),
                        m.amount0.as_str(),
                        m.amount1.as_str(),
                        m.min_amount0.as_str(),
                        m.min_amount1.as_str(),
                    )
                } else {
                    (
                        m.token1.as_str(),
                        m.token0.as_str(),
                        m.amount1.as_str(),
                        m.amount0.as_str(),
                        m.min_amount1.as_str(),
                        m.min_amount0.as_str(),
                    )
                }
            };

            let token0 = resolve_asset_address(token0_alias, registry)?;
            let token1 = resolve_asset_address(token1_alias, registry)?;
            let dec0 = resolve_asset_decimals(token0_alias, registry)?;
            let dec1 = resolve_asset_decimals(token1_alias, registry)?;

            reject_all_amount("lp_mint", "amount0", amount0_raw)?;
            reject_all_amount("lp_mint", "amount1", amount1_raw)?;
            reject_all_amount("lp_mint", "min_amount0", min0_raw)?;
            reject_all_amount("lp_mint", "min_amount1", min1_raw)?;

            let amount0 = parse_amount(amount0_raw, dec0)?;
            let amount1 = parse_amount(amount1_raw, dec1)?;
            let amount0_min = parse_amount(min0_raw, dec0)?;
            let amount1_min = parse_amount(min1_raw, dec1)?;

            let deadline = resolve_step_deadline(m.deadline, script);

            Ok(ResolvedStep::UniswapV3LpMint {
                npm,
                token0,
                token1,
                fee,
                tick_lower: m.tick_lower,
                tick_upper: m.tick_upper,
                amount0,
                amount1,
                amount0_min,
                amount1_min,
                recipient: signer,
                deadline: U256::from(deadline),
            })
        }
        Step::LpIncrease(inc) => {
            let npm = resolve_uniswap_position_manager(registry)?;
            let token_id = parse_u256_decimal("position_id", &inc.position_id)?;

            // Sort (token0, token1) + amounts in lockstep so the compiler's
            // canonical ordering always matches the NFT's on-chain ordering.
            let (token0_alias, token1_alias, amount0_raw, amount1_raw, min0_raw, min1_raw) = {
                let a = resolve_asset_address(&inc.token0, registry)?;
                let b = resolve_asset_address(&inc.token1, registry)?;
                if a <= b {
                    (
                        inc.token0.as_str(),
                        inc.token1.as_str(),
                        inc.amount0.as_str(),
                        inc.amount1.as_str(),
                        inc.min_amount0.as_str(),
                        inc.min_amount1.as_str(),
                    )
                } else {
                    (
                        inc.token1.as_str(),
                        inc.token0.as_str(),
                        inc.amount1.as_str(),
                        inc.amount0.as_str(),
                        inc.min_amount1.as_str(),
                        inc.min_amount0.as_str(),
                    )
                }
            };

            let token0 = resolve_asset_address(token0_alias, registry)?;
            let token1 = resolve_asset_address(token1_alias, registry)?;
            let dec0 = resolve_asset_decimals(token0_alias, registry)?;
            let dec1 = resolve_asset_decimals(token1_alias, registry)?;

            reject_all_amount("lp_increase", "amount0", amount0_raw)?;
            reject_all_amount("lp_increase", "amount1", amount1_raw)?;
            reject_all_amount("lp_increase", "min_amount0", min0_raw)?;
            reject_all_amount("lp_increase", "min_amount1", min1_raw)?;

            let amount0 = parse_amount(amount0_raw, dec0)?;
            let amount1 = parse_amount(amount1_raw, dec1)?;
            let amount0_min = parse_amount(min0_raw, dec0)?;
            let amount1_min = parse_amount(min1_raw, dec1)?;

            let deadline = resolve_step_deadline(inc.deadline, script);

            Ok(ResolvedStep::UniswapV3LpIncrease {
                npm,
                token0,
                token1,
                token_id,
                amount0,
                amount1,
                amount0_min,
                amount1_min,
                deadline: U256::from(deadline),
            })
        }
        Step::LpDecrease(dec) => {
            let npm = resolve_uniswap_position_manager(registry)?;
            let token_id = parse_u256_decimal("position_id", &dec.position_id)?;

            let liquidity: u128 = if dec.liquidity == "all" {
                return Err(CompileError::InvalidAmount(
                    "lp_decrease liquidity='all' is not supported in v1 — supply the explicit u128 liquidity amount"
                        .to_string(),
                ));
            } else {
                dec.liquidity.parse().map_err(|_| {
                    CompileError::InvalidAmount(format!(
                        "Invalid liquidity value: '{}' (must fit in u128)",
                        dec.liquidity
                    ))
                })?
            };

            // Sort token aliases so min_amount{0,1} line up with the position's
            // canonical (token0, token1) ordering.
            let (token0_alias, token1_alias, min0_raw, min1_raw) = {
                let a = resolve_asset_address(&dec.token0, registry)?;
                let b = resolve_asset_address(&dec.token1, registry)?;
                if a <= b {
                    (
                        dec.token0.as_str(),
                        dec.token1.as_str(),
                        dec.min_amount0.as_str(),
                        dec.min_amount1.as_str(),
                    )
                } else {
                    (
                        dec.token1.as_str(),
                        dec.token0.as_str(),
                        dec.min_amount1.as_str(),
                        dec.min_amount0.as_str(),
                    )
                }
            };
            let dec0 = resolve_asset_decimals(token0_alias, registry)?;
            let dec1 = resolve_asset_decimals(token1_alias, registry)?;

            reject_all_amount("lp_decrease", "min_amount0", min0_raw)?;
            reject_all_amount("lp_decrease", "min_amount1", min1_raw)?;
            let amount0_min = parse_amount(min0_raw, dec0)?;
            let amount1_min = parse_amount(min1_raw, dec1)?;

            let deadline = resolve_step_deadline(dec.deadline, script);

            Ok(ResolvedStep::UniswapV3LpDecrease {
                npm,
                token_id,
                liquidity,
                amount0_min,
                amount1_min,
                deadline: U256::from(deadline),
            })
        }
        Step::LpCollect(col) => {
            let npm = resolve_uniswap_position_manager(registry)?;
            let token_id = parse_u256_decimal("position_id", &col.position_id)?;

            let (token0_alias, token1_alias) = {
                let a = resolve_asset_address(&col.token0, registry)?;
                let b = resolve_asset_address(&col.token1, registry)?;
                if a <= b {
                    (col.token0.as_str(), col.token1.as_str())
                } else {
                    (col.token1.as_str(), col.token0.as_str())
                }
            };
            let token0 = resolve_asset_address(token0_alias, registry)?;
            let token1 = resolve_asset_address(token1_alias, registry)?;

            // Collect everything that's uncollected on the position — the
            // standard NPM pattern. Callers who need a cap can wait for a
            // future DSL revision.
            Ok(ResolvedStep::UniswapV3LpCollect {
                npm,
                token0,
                token1,
                token_id,
                recipient: signer,
                amount0_max: u128::MAX,
                amount1_max: u128::MAX,
            })
        }
        Step::Send(s) => {
            let to = parse_address(&s.to)?;
            let asset_type = s.asset_type.as_deref().unwrap_or("erc20");

            match asset_type {
                "erc721" => {
                    // ERC-721 send
                    let contract_str = s.contract.as_deref().ok_or_else(|| {
                        CompileError::Validation(
                            "ERC-721 send requires 'contract' field".to_string(),
                        )
                    })?;
                    let contract = parse_address(contract_str)?;

                    let token_id_str = s.token_id.as_deref().ok_or_else(|| {
                        CompileError::Validation(
                            "ERC-721 send requires 'token_id' field".to_string(),
                        )
                    })?;
                    let token_id: u128 = token_id_str.parse().map_err(|_| {
                        CompileError::InvalidAmount(format!("Invalid token_id: {}", token_id_str))
                    })?;

                    Ok(ResolvedStep::SendErc721 {
                        contract,
                        from: signer,
                        to,
                        token_id: U256::from(token_id),
                    })
                }
                _ => {
                    // ERC-20 or ETH send
                    let asset_alias = s.asset.as_deref().ok_or_else(|| {
                        CompileError::Validation(
                            "Send requires 'asset' field (e.g. 'USDC', 'ETH')".to_string(),
                        )
                    })?;
                    let amount_str = s.amount.as_deref().ok_or_else(|| {
                        CompileError::Validation("Send requires 'amount' field".to_string())
                    })?;

                    if registry.is_native(asset_alias) {
                        // Send ETH
                        let decimals = resolve_asset_decimals(asset_alias, registry)?;
                        let amount = resolve_amount_or_all(
                            amount_str,
                            decimals,
                            Address::ZERO,
                            previous_steps,
                            asset_alias,
                            registry,
                        )?;
                        Ok(ResolvedStep::SendEth { to, amount })
                    } else {
                        // Send ERC-20
                        let token = resolve_asset_address(asset_alias, registry)?;
                        let decimals = resolve_asset_decimals(asset_alias, registry)?;
                        let amount = resolve_amount_or_all(
                            amount_str,
                            decimals,
                            token,
                            previous_steps,
                            asset_alias,
                            registry,
                        )?;
                        Ok(ResolvedStep::SendErc20 { token, to, amount })
                    }
                }
            }
        }
        Step::Custom(_) => Err(CompileError::UnsupportedStep(
            "custom steps are not yet implemented in v1".to_string(),
        )),
    }
}

/// Post-normalization pass: elide `Wrap ETH→WETH` steps that immediately
/// precede a step which already takes native ETH.
///
/// Two safe rewrites:
/// 1. `Wrap { WETH, X }` + `LidoStake { X }` → drop the wrap. Lido's
///    `submit()` is payable and takes ETH as msg.value directly.
/// 2. `Wrap { WETH, X }` + `UniswapV3Swap { token_in: WETH, amount_in: X,
///    native_input: false }` → drop the wrap and set `native_input: true`.
///    The SwapRouter's internal `pay()` helper auto-wraps msg.value.
///
/// Both rewrites require the amounts to match exactly; mismatched amounts
/// suggest the user actually wants to keep some wrapped WETH, so we leave
/// things untouched.
fn elide_wasteful_wraps(
    steps: &mut Vec<ResolvedStep>,
    registry: &RegistryContext,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let weth = resolve_asset_address(&registry.chain.wrapped_native, registry)?;

    let mut i = 0;
    while i + 1 < steps.len() {
        // Read the wrap amount without holding a borrow into the later match.
        let wrap_amount = match &steps[i] {
            ResolvedStep::Wrap {
                wrapped_token,
                amount,
            } if *wrapped_token == weth => *amount,
            _ => {
                i += 1;
                continue;
            }
        };

        let mut elided_kind: Option<&'static str> = None;
        match &mut steps[i + 1] {
            ResolvedStep::LidoStake {
                amount: stake_amt, ..
            } if *stake_amt == wrap_amount => {
                elided_kind = Some("stake");
            }
            ResolvedStep::UniswapV3Swap {
                token_in,
                amount_in,
                native_input,
                ..
            } if *token_in == weth && *amount_in == wrap_amount && !*native_input => {
                *native_input = true;
                elided_kind = Some("swap");
            }
            _ => {}
        }

        if let Some(kind) = elided_kind {
            warnings.push(format!(
                "Elided redundant 'wrap ETH→WETH' before '{kind}': the destination \
                 accepts native ETH directly. This costs no extra wallet prompts \
                 and saves gas."
            ));
            steps.remove(i);
            // Don't advance `i` — the step now at position i is the rewritten
            // consumer (the stake/swap), not another wrap candidate.
        } else {
            i += 1;
        }
    }
    Ok(())
}

/// Resolve an amount string, supporting the "all" keyword.
///
/// When `amount_str` is `"all"`, resolves to the guaranteed output of the most
/// recent previous step that produces the given token. Otherwise, parses as a
/// normal human-readable amount.
fn resolve_amount_or_all(
    amount_str: &str,
    decimals: u8,
    token: Address,
    previous_steps: &[ResolvedStep],
    _asset_alias: &str,
    registry: &RegistryContext,
) -> Result<U256> {
    if amount_str == "all" {
        let fee_bps = registry.fee_bps();
        for step in previous_steps.iter().rev() {
            if let Some((produced_token, guaranteed)) = step_produces(step, fee_bps) {
                if produced_token == token {
                    if guaranteed == U256::ZERO {
                        return Err(CompileError::InvalidChain(
                            "Cannot use 'all': previous step has zero guaranteed output".into(),
                        ));
                    }
                    return Ok(guaranteed);
                }
            }
        }
        return Err(CompileError::InvalidChain(
            "Cannot use 'all': no previous step produces this token".into(),
        ));
    }
    parse_amount(amount_str, decimals)
}

/// Compute `amount_out_minimum` for a Uniswap swap from the JSON slippage params.
///
/// Precedence:
/// 1. `min_amount_out` provided → parse with output token decimals, use directly
/// 2. `price` provided (+ optional `slippage`, default 0.5%) → compute: amount_in_human * price * (1 - slippage/100)
/// 3. `slippage` without `price` → error
/// 4. Neither → U256::ZERO + warning
fn compute_amount_out_minimum(
    swap: &SwapStep,
    amount_in: U256,
    output_token_alias: &str,
    registry: &RegistryContext,
    warnings: &mut Vec<String>,
) -> Result<U256> {
    // Case 1: Explicit min_amount_out
    if let Some(ref min_out_str) = swap.min_amount_out {
        let out_decimals = resolve_asset_decimals(output_token_alias, registry)?;
        return parse_amount(min_out_str, out_decimals);
    }

    // Case 2: price provided (with optional slippage)
    if let Some(ref price_str) = swap.price {
        let in_decimals = resolve_asset_decimals(&swap.from, registry)?;
        let out_decimals = resolve_asset_decimals(output_token_alias, registry)?;

        // Parse price as a fixed-point integer scaled to `out_decimals` fractional digits.
        // This avoids f64 precision loss on large amounts.
        let price_scaled = parse_amount(price_str, out_decimals).map_err(|_| {
            CompileError::InvalidAmount(format!("Invalid price value: {}", price_str))
        })?;
        if price_scaled == U256::ZERO {
            return Err(CompileError::InvalidAmount(
                "Price must be positive".to_string(),
            ));
        }

        // Parse slippage as basis points (1% = 100 bps, 0.5% = 50 bps).
        // Scale is 10000 bps = 100%, so parse the percent string with 2 decimals.
        let slippage_bps_u256 = match &swap.slippage {
            Some(s) => {
                if s.starts_with('-') {
                    return Err(CompileError::InvalidAmount(format!(
                        "Slippage must be between 0 and 100, got {}",
                        s
                    )));
                }
                parse_amount(s, 2).map_err(|_| {
                    CompileError::InvalidAmount(format!(
                        "Slippage must be between 0 and 100, got {}",
                        s
                    ))
                })?
            }
            None => U256::from(50u64), // 0.5% = 50 bps
        };
        let slippage_bps: u64 = slippage_bps_u256.try_into().map_err(|_| {
            CompileError::InvalidAmount(format!(
                "Slippage must be between 0 and 100, got {}",
                swap.slippage.as_deref().unwrap_or("?")
            ))
        })?;
        if slippage_bps >= 10_000 {
            return Err(CompileError::InvalidAmount(format!(
                "Slippage must be between 0 and 100, got {}",
                swap.slippage.as_deref().unwrap_or("?")
            )));
        }

        // expected_output_smallest = amount_in * price_scaled / 10^in_decimals
        //   where price_scaled is price * 10^out_decimals.
        // This yields the expected output in the output token's smallest units.
        let scale_in = U256::from(10u128).pow(U256::from(in_decimals as u64));
        let expected = amount_in.checked_mul(price_scaled).ok_or_else(|| {
            CompileError::InvalidAmount("Overflow computing expected output".to_string())
        })? / scale_in;

        // min_output = expected * (10000 - slippage_bps) / 10000
        let numerator = U256::from(10_000u64 - slippage_bps);
        let denominator = U256::from(10_000u64);
        let min_output = expected
            .checked_mul(numerator)
            .ok_or_else(|| CompileError::InvalidAmount("Overflow applying slippage".to_string()))?
            / denominator;

        return Ok(min_output);
    }

    // Case 3: slippage without price → error
    if swap.slippage.is_some() {
        return Err(CompileError::InvalidAmount(
            "Slippage requires the 'price' field to be set. \
             Provide 'price' (output tokens per 1 input token) alongside 'slippage', \
             or use 'min_amount_out' for an explicit minimum."
                .to_string(),
        ));
    }

    // Case 4: Neither provided → zero with warning
    warnings.push(
        "No slippage protection: swap has no 'min_amount_out', 'price', or 'slippage' specified. \
         The swap will use amountOutMinimum=0, making it vulnerable to sandwich attacks. \
         Consider adding 'min_amount_out' or 'price'+'slippage' to the swap step."
            .to_string(),
    );
    Ok(U256::ZERO)
}

/// Resolve an asset alias to its on-chain address.
fn resolve_asset_address(alias: &str, registry: &RegistryContext) -> Result<Address> {
    let config = registry
        .assets
        .get(alias)
        .ok_or_else(|| CompileError::UnknownAsset {
            asset: alias.to_string(),
            network: registry.network.clone(),
            suggestion: closest_match(alias, registry.assets.keys().map(|s| s.as_str())),
        })?;

    if config.address == "native" {
        // Native assets don't have a token address — but for wrap/unwrap
        // we need the wrapped token address. The caller handles this.
        // Return zero address as sentinel for native.
        Ok(Address::ZERO)
    } else {
        parse_address(&config.address)
    }
}

/// Resolve an asset alias to its decimal count.
fn resolve_asset_decimals(alias: &str, registry: &RegistryContext) -> Result<u8> {
    let config = registry
        .assets
        .get(alias)
        .ok_or_else(|| CompileError::UnknownAsset {
            asset: alias.to_string(),
            network: registry.network.clone(),
            suggestion: closest_match(alias, registry.assets.keys().map(|s| s.as_str())),
        })?;
    Ok(config.decimals)
}

/// Parse a hex address string into an Address.
fn parse_address(s: &str) -> Result<Address> {
    s.parse::<Address>()
        .map_err(|_| CompileError::InvalidAddress(s.to_string()))
}

/// Parse a uint256 as a decimal string (no unit scaling — raw integer).
///
/// Used for step fields that don't have an associated asset decimals (e.g.
/// `position_id`, or LP `amount*` fields where the pair's tokens are baked
/// into the NFT position and the DSL string is already in wei).
fn parse_u256_decimal(field: &str, s: &str) -> Result<U256> {
    U256::from_str_radix(s, 10).map_err(|_| {
        CompileError::InvalidAmount(format!(
            "Invalid {}: '{}' is not a valid decimal integer",
            field, s
        ))
    })
}

/// Reject `"all"` sugar in fields where we can't resolve it meaningfully.
fn reject_all_amount(step_name: &str, field: &str, value: &str) -> Result<()> {
    if value == "all" {
        Err(CompileError::InvalidAmount(format!(
            "{} does not support 'all' for {} — supply an explicit amount",
            step_name, field
        )))
    } else {
        Ok(())
    }
}

/// Parse a Uniswap V3 fee tier from a string, accepting only the canonical
/// values.
fn parse_uniswap_fee_tier(s: &str) -> Result<u32> {
    match s {
        "500" => Ok(500),
        "3000" => Ok(3000),
        "10000" => Ok(10000),
        other => Err(CompileError::InvalidAmount(format!(
            "Invalid Uniswap V3 fee tier '{}' (must be 500, 3000, or 10000)",
            other
        ))),
    }
}

/// Canonical tick spacing for a given Uniswap V3 fee tier.
fn uniswap_tick_spacing(fee: u32) -> i32 {
    match fee {
        500 => 10,
        3000 => 60,
        10000 => 200,
        // parse_uniswap_fee_tier gates inputs to this table; any other value
        // here signals a caller bug.
        _ => 1,
    }
}

/// Validate that an LP tick range is non-empty, bounded, and aligned with
/// the pool's tick spacing.
fn validate_uniswap_tick_range(lower: i32, upper: i32, spacing: i32) -> Result<()> {
    // Uniswap V3 MIN_TICK / MAX_TICK from TickMath.sol.
    const MIN_TICK: i32 = -887272;
    const MAX_TICK: i32 = 887272;
    if lower >= upper {
        return Err(CompileError::InvalidChain(format!(
            "LP tick_lower ({}) must be strictly less than tick_upper ({})",
            lower, upper
        )));
    }
    if lower < MIN_TICK || upper > MAX_TICK {
        return Err(CompileError::InvalidChain(format!(
            "LP tick range [{}, {}] exceeds allowed bounds [{}, {}]",
            lower, upper, MIN_TICK, MAX_TICK
        )));
    }
    if lower % spacing != 0 || upper % spacing != 0 {
        return Err(CompileError::InvalidChain(format!(
            "LP ticks ({}, {}) must be multiples of tick spacing {} for this fee tier",
            lower, upper, spacing
        )));
    }
    Ok(())
}

/// Look up the Uniswap V3 NonfungiblePositionManager address from the
/// protocol registry.
fn resolve_uniswap_position_manager(registry: &RegistryContext) -> Result<Address> {
    let protocol =
        registry
            .protocols
            .get("uniswap")
            .ok_or_else(|| CompileError::UnknownProtocol {
                protocol: "uniswap".to_string(),
                network: registry.network.clone(),
                available: known_protocols(registry),
            })?;

    let npm_addr = protocol.contracts.get("position_manager").ok_or_else(|| {
        CompileError::Adapter(
            "Protocol 'uniswap' has no 'position_manager' contract configured".to_string(),
        )
    })?;
    parse_address(npm_addr)
}

/// Compute a deadline for an LP step, falling back to the script's
/// effective deadline and ultimately the default swap window.
fn resolve_step_deadline(step_deadline: Option<u64>, script: &IntentScript) -> u64 {
    if let Some(d) = step_deadline {
        if d > 0 {
            return d;
        }
    }
    match script.deadline {
        Some(d) if d > 0 => d,
        _ => match script.current_timestamp {
            Some(ts) => ts + DEFAULT_SWAP_DEADLINE_SECS,
            None => u64::MAX,
        },
    }
}

/// Normalize user balance information into resolved types.
fn normalize_balances(
    balances: &crate::schema::UserBalances,
    registry: &RegistryContext,
) -> Result<ResolvedBalances> {
    let mut tokens = HashMap::new();
    for (alias, amount_str) in &balances.tokens {
        if let Ok(addr) = resolve_asset_address(alias, registry) {
            let decimals = resolve_asset_decimals(alias, registry)?;
            let amount = parse_amount(amount_str, decimals)?;
            tokens.insert(addr, amount);
        }
        // Skip unknown assets in balances — they're informational, not critical
    }

    let mut aave_supplied = HashMap::new();
    let mut aave_borrowed = HashMap::new();
    let mut aave_health_factor = None;

    if let Some(ref aave) = balances.aave_positions {
        for (alias, amount_str) in &aave.supplied {
            if let Ok(addr) = resolve_asset_address(alias, registry) {
                let decimals = resolve_asset_decimals(alias, registry)?;
                let amount = parse_amount(amount_str, decimals)?;
                aave_supplied.insert(addr, amount);
            }
        }
        for (alias, amount_str) in &aave.borrowed {
            if let Ok(addr) = resolve_asset_address(alias, registry) {
                let decimals = resolve_asset_decimals(alias, registry)?;
                let amount = parse_amount(amount_str, decimals)?;
                aave_borrowed.insert(addr, amount);
            }
        }
        if let Some(ref hf_str) = aave.health_factor {
            aave_health_factor = hf_str.parse::<f64>().ok();
        }
    }

    Ok(ResolvedBalances {
        tokens,
        aave_supplied,
        aave_borrowed,
        aave_health_factor,
    })
}

/// Parse a human-readable amount string (e.g. "1.5", "10000", "0.01") into U256
/// using the token's decimal places.
pub(crate) fn parse_amount(amount_str: &str, decimals: u8) -> Result<U256> {
    // Split on decimal point
    let parts: Vec<&str> = amount_str.split('.').collect();

    match parts.len() {
        1 => {
            // Integer amount like "10000"
            let whole: u128 = parts[0]
                .parse()
                .map_err(|_| CompileError::InvalidAmount(amount_str.to_string()))?;
            let multiplier = 10u128.pow(decimals as u32);
            Ok(U256::from(whole) * U256::from(multiplier))
        }
        2 => {
            // Decimal amount like "1.5" or "0.01"
            let whole: u128 = parts[0]
                .parse()
                .map_err(|_| CompileError::InvalidAmount(amount_str.to_string()))?;

            let frac_str = parts[1];
            let frac_len = frac_str.len() as u8;

            if frac_len > decimals {
                return Err(CompileError::InvalidAmount(format!(
                    "{amount_str} has more decimal places than token supports ({decimals})"
                )));
            }

            let frac: u128 = frac_str
                .parse()
                .map_err(|_| CompileError::InvalidAmount(amount_str.to_string()))?;

            let multiplier = 10u128.pow(decimals as u32);
            let frac_multiplier = 10u128.pow((decimals - frac_len) as u32);

            Ok(U256::from(whole) * U256::from(multiplier)
                + U256::from(frac) * U256::from(frac_multiplier))
        }
        _ => Err(CompileError::InvalidAmount(amount_str.to_string())),
    }
}

/// Decode a hex string into bytes.
fn hex_decode(hex: &str) -> core::result::Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("Odd-length hex string".to_string());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| format!("Invalid hex: {}", e)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_amount_integer() {
        // 10000 USDC (6 decimals) = 10_000_000_000
        let result = parse_amount("10000", 6).unwrap();
        assert_eq!(result, U256::from(10_000_000_000u64));
    }

    #[test]
    fn test_parse_amount_decimal() {
        // 1.5 ETH (18 decimals) = 1_500_000_000_000_000_000
        let result = parse_amount("1.5", 18).unwrap();
        assert_eq!(result, U256::from(1_500_000_000_000_000_000u64));
    }

    #[test]
    fn test_parse_amount_small_decimal() {
        // 0.01 WBTC (8 decimals) = 1_000_000
        let result = parse_amount("0.01", 8).unwrap();
        assert_eq!(result, U256::from(1_000_000u64));
    }

    #[test]
    fn test_parse_amount_whole_number_no_frac() {
        // 5000 USDC (6 decimals) = 5_000_000_000
        let result = parse_amount("5000", 6).unwrap();
        assert_eq!(result, U256::from(5_000_000_000u64));
    }
}
