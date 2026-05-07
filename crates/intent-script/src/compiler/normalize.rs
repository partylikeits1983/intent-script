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
use crate::ir::{
    MorphoMarketParams, ResolvedBalances, ResolvedIntent, ResolvedStep, step_produces,
};
use crate::registry::{MorphoMarketConfig, RegistryContext};
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
        // Scratch buffer for steps a branch may want to emit *before* its
        // primary resolved step (e.g. lp_mint auto-injects a Wrap when one
        // side is native ETH). Almost every branch leaves this untouched.
        let mut prepend = Vec::new();
        let resolved = normalize_step(
            step,
            signer,
            registry,
            &mut warnings,
            script,
            &steps,
            &mut prepend,
        )?;
        steps.extend(prepend);
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

    // Any step that must run inside the router's call loop (flashloans, whose
    // callback needs the transient sentinel armed by `_executeCalls`) forces
    // router batching even for single-call pipelines.
    let requires_router = steps.iter().any(|s| {
        matches!(
            s,
            ResolvedStep::BalancerFlashloan { .. } | ResolvedStep::AaveFlashloan { .. }
        )
    });

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
            required_delegations: Vec::new(),
            fee_bps: registry.fee_bps(),
            requires_router,
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
    // Scratch buffer for branches that need to emit resolved steps *before*
    // their primary returned step — currently only `Step::LpMint` uses it,
    // to inject a `Wrap` when one side of the pair is native ETH.
    prepend_steps: &mut Vec<ResolvedStep>,
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
            if d.into == "morpho" {
                return normalize_morpho_deposit(d, signer, registry, previous_steps);
            }
            // Aave (and any other simple pool-keyed lending) path.
            if d.market.is_some() || d.r#as.is_some() {
                return Err(CompileError::Validation(format!(
                    "'market' and 'as' are only valid when depositing into 'morpho' (got '{}')",
                    d.into
                )));
            }
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
            if b.from == "morpho" {
                return normalize_morpho_borrow(b, signer, registry, previous_steps);
            }
            if b.market.is_some() {
                return Err(CompileError::Validation(format!(
                    "'market' is only valid when borrowing from 'morpho' (got '{}')",
                    b.from
                )));
            }
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
            if w.from == "morpho" {
                return normalize_morpho_withdraw(w, signer, registry, previous_steps);
            }
            if w.market.is_some() || w.r#as.is_some() {
                return Err(CompileError::Validation(format!(
                    "'market' and 'as' are only valid when withdrawing from 'morpho' (got '{}')",
                    w.from
                )));
            }
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
                    // Default fee tier: 100 (0.01%) for stable↔stable pairs
                    // (USDC↔USDT, USDC↔DAI, …) where the deep V3 liquidity
                    // lives on the canonical 0.01% pool, and 3000 (0.3%)
                    // for everything else. Without this conditional default,
                    // a stable-stable swap with no explicit `fee` falls back
                    // to 3000, hits a non-existent pool, and reverts with
                    // empty data ("Execution reverted for an unknown reason"
                    // in viem). Explicit `fee` from the LLM/user always
                    // wins — this only kicks in when the field is omitted.
                    let default_fee_tier =
                        if is_stable_symbol(&s.from) && is_stable_symbol(&s.to) {
                            "100"
                        } else {
                            "3000"
                        };
                    let fee: u32 = s
                        .fee
                        .as_deref()
                        .unwrap_or(default_fee_tier)
                        .parse()
                        .map_err(|_| CompileError::UniswapFeeTierUnknown {
                            fee: s.fee.clone().unwrap_or_default(),
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
                other => Err(CompileError::UnsupportedStep(format!(
                    "swap via '{}' is not supported; only 'uniswap' is supported",
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

            let npm = resolve_uniswap_position_manager(registry)?;

            // Substitute native ETH with the chain's wrapped-native (WETH):
            // Uniswap's NPM mint takes two ERC-20 addresses as token0/token1,
            // so a native sentinel (Address::ZERO) can't appear there. When
            // the user names ETH, we rewrite the alias to WETH and later
            // inject a `Wrap` step before the mint so the router holds WETH
            // by the time NPM pulls it. Both-native is nonsensical and is
            // rejected by `validate_asset_compatibility`.
            let addr0_raw = resolve_asset_address(&m.token0, registry)?;
            let addr1_raw = resolve_asset_address(&m.token1, registry)?;
            let native0 = addr0_raw == Address::ZERO;
            let native1 = addr1_raw == Address::ZERO;
            if native0 && native1 {
                return Err(CompileError::InvalidChain(
                    "lp_mint requires at least one non-native token: \
                     both token0 and token1 are the chain's native asset."
                        .to_string(),
                ));
            }
            let weth_alias = registry.chain.wrapped_native.clone();
            let token0_alias_owned: String = if native0 {
                weth_alias.clone()
            } else {
                m.token0.clone()
            };
            let token1_alias_owned: String = if native1 {
                weth_alias.clone()
            } else {
                m.token1.clone()
            };

            // Lexicographically sort (token0, token1) by address, swapping
            // paired amount / min fields in lockstep so the NPM sees the
            // pair in canonical order regardless of DSL-side ordering.
            // Sort *after* the ETH→WETH substitution so the canonical order
            // uses WETH's real address (not the Address::ZERO sentinel).
            // `min_amount0` / `min_amount1` are optional in the public schema:
            // omitting them (or `null`) is equivalent to `"0"`. The range itself
            // is the slippage guard for `lp_mint`; see the discussion in the
            // LLM system prompt.
            let m_min0 = m.min_amount0.as_deref().unwrap_or("0");
            let m_min1 = m.min_amount1.as_deref().unwrap_or("0");
            let (
                token0_alias,
                token1_alias,
                amount0_raw,
                amount1_raw,
                min0_raw,
                min1_raw,
                tokens_swapped,
            ) = {
                let token0_addr = resolve_asset_address(&token0_alias_owned, registry)?;
                let token1_addr = resolve_asset_address(&token1_alias_owned, registry)?;
                if token0_addr <= token1_addr {
                    (
                        token0_alias_owned.as_str(),
                        token1_alias_owned.as_str(),
                        m.amount0.as_str(),
                        m.amount1.as_str(),
                        m_min0,
                        m_min1,
                        false,
                    )
                } else {
                    (
                        token1_alias_owned.as_str(),
                        token0_alias_owned.as_str(),
                        m.amount1.as_str(),
                        m.amount0.as_str(),
                        m_min1,
                        m_min0,
                        true,
                    )
                }
            };

            let token0 = resolve_asset_address(token0_alias, registry)?;
            let token1 = resolve_asset_address(token1_alias, registry)?;
            let dec0 = resolve_asset_decimals(token0_alias, registry)?;
            let dec1 = resolve_asset_decimals(token1_alias, registry)?;

            // If the user wrote the native alias (e.g. "ETH") as `quote_token`,
            // substitute it with the wrapped-native alias so the string-equality
            // check in `classify_quote_direction` lines up against the token
            // aliases we've already rewritten above.
            let quote_token_override: Option<String> =
                m.quote_token
                    .as_deref()
                    .and_then(|qt| match resolve_asset_address(qt, registry) {
                        Ok(addr) if addr == Address::ZERO => Some(weth_alias.clone()),
                        _ => None,
                    });

            // Resolve tick range from either the price form (preferred) or
            // the raw tick form (advanced escape hatch). Exactly one of the
            // two shapes must be supplied, per bound.
            let (tick_lower, tick_upper) = resolve_lp_mint_ticks(
                m,
                token0_alias,
                token1_alias,
                tokens_swapped,
                dec0,
                dec1,
                tick_spacing,
                warnings,
                quote_token_override.as_deref(),
            )?;
            validate_uniswap_tick_range(tick_lower, tick_upper, tick_spacing)?;

            reject_all_amount("lp_mint", "amount0", amount0_raw)?;
            reject_all_amount("lp_mint", "amount1", amount1_raw)?;
            reject_all_amount("lp_mint", "min_amount0", min0_raw)?;
            reject_all_amount("lp_mint", "min_amount1", min1_raw)?;

            let amount0 = parse_amount(amount0_raw, dec0)?;
            let amount1 = parse_amount(amount1_raw, dec1)?;
            let amount0_min = parse_amount(min0_raw, dec0)?;
            let amount1_min = parse_amount(min1_raw, dec1)?;

            let deadline = resolve_step_deadline(m.deadline, script);

            // If one side was native ETH, inject a preceding `Wrap` step for
            // the exact amount the mint needs on that side. The wrap pulls
            // msg.value → WETH into the router, which the enricher sees via
            // `tokens_in_router`; it then skips the user-side transferFrom /
            // prerequisite-approval for WETH and only pulls the ERC-20 side.
            // Amount-zero single-sided mints on the native side — legitimate
            // for out-of-range positions — must not emit a zero-value wrap
            // (the builder rejects zero-amount steps).
            let wrap_amount = match (native0, native1, tokens_swapped) {
                (true, false, false) | (false, true, true) => Some(amount0),
                (false, true, false) | (true, false, true) => Some(amount1),
                (false, false, _) => None,
                (true, true, _) => unreachable!("both-native lp_mint rejected above"),
            };
            if let Some(amt) = wrap_amount
                && amt > U256::ZERO
            {
                let weth_addr = resolve_asset_address(&weth_alias, registry)?;
                prepend_steps.push(ResolvedStep::Wrap {
                    wrapped_token: weth_addr,
                    amount: amt,
                });
            }

            Ok(ResolvedStep::UniswapV3LpMint {
                npm,
                token0,
                token1,
                fee,
                tick_lower,
                tick_upper,
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

            // `min_amount0` / `min_amount1` are optional — default to `"0"`.
            // See the `LpMintStep` branch for the rationale.
            let inc_min0 = inc.min_amount0.as_deref().unwrap_or("0");
            let inc_min1 = inc.min_amount1.as_deref().unwrap_or("0");
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
                        inc_min0,
                        inc_min1,
                    )
                } else {
                    (
                        inc.token1.as_str(),
                        inc.token0.as_str(),
                        inc.amount1.as_str(),
                        inc.amount0.as_str(),
                        inc_min1,
                        inc_min0,
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

            // `min_amount0` / `min_amount1` are optional — default to `"0"`.
            // Unlike `lp_mint`, a positive min is a legitimate sandwich guard
            // for `lp_decrease` (prices can be pushed at removal time). We
            // still accept omission so symmetric intents are valid.
            let dec_min0 = dec.min_amount0.as_deref().unwrap_or("0");
            let dec_min1 = dec.min_amount1.as_deref().unwrap_or("0");
            // Sort token aliases so min_amount{0,1} line up with the position's
            // canonical (token0, token1) ordering.
            let (token0_alias, token1_alias, min0_raw, min1_raw) = {
                let a = resolve_asset_address(&dec.token0, registry)?;
                let b = resolve_asset_address(&dec.token1, registry)?;
                if a <= b {
                    (dec.token0.as_str(), dec.token1.as_str(), dec_min0, dec_min1)
                } else {
                    (dec.token1.as_str(), dec.token0.as_str(), dec_min1, dec_min0)
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
        Step::Flashloan(f) => normalize_flashloan(f, signer, registry, warnings, script),
        Step::Bridge(b) => normalize_bridge(b, signer, registry, script),
        Step::Long(l) => crate::compiler::leverage::expand_leverage(
            l,
            crate::compiler::leverage::Side::Long,
            signer,
            registry,
            script,
        ),
        Step::Short(s) => crate::compiler::leverage::expand_leverage(
            s,
            crate::compiler::leverage::Side::Short,
            signer,
            registry,
            script,
        ),
        Step::ClosePosition(c) => {
            crate::compiler::leverage::expand_close(c, signer, registry, script)
        }
        Step::Custom(_) => Err(CompileError::UnsupportedStep(
            "custom steps are not yet implemented in v1".to_string(),
        )),
    }
}

fn normalize_bridge(
    b: &crate::schema::BridgeStep,
    signer: Address,
    registry: &RegistryContext,
    script: &IntentScript,
) -> Result<ResolvedStep> {
    if b.via != "across" {
        return Err(CompileError::UnsupportedStep(format!(
            "bridge via '{}' is not supported (only 'across' in v1)",
            b.via
        )));
    }

    // Native ETH rejected — Across expects pre-wrapped WETH.
    if registry.is_native(&b.asset) {
        return Err(CompileError::Validation(
            "Across bridge does not accept native ETH — wrap to WETH first with a `wrap` step"
                .to_string(),
        ));
    }

    let protocol =
        registry
            .protocols
            .get("across")
            .ok_or_else(|| CompileError::UnknownProtocol {
                protocol: "across".to_string(),
                network: registry.network.clone(),
                available: known_protocols(registry),
            })?;
    let spoke_addr = protocol.contracts.get("spoke_pool").ok_or_else(|| {
        CompileError::Adapter(
            "Protocol 'across' has no 'spoke_pool' contract configured".to_string(),
        )
    })?;
    let spoke_pool = parse_address(spoke_addr)?;

    let dest_chain = registry
        .all_chains
        .get(&b.to_chain)
        .ok_or_else(|| CompileError::UnknownNetwork(b.to_chain.clone()))?;
    let destination_chain_id = U256::from(dest_chain.chain_id);

    let recipient = parse_address(&b.recipient)?;
    if recipient == Address::ZERO {
        return Err(CompileError::Validation(
            "Across recipient cannot be the zero address".to_string(),
        ));
    }

    let relayer_fee_bps: u64 = b.relayer_fee_bps.parse().map_err(|_| {
        CompileError::InvalidAmount(format!(
            "Invalid relayer_fee_bps '{}' — must be an integer 0..=50",
            b.relayer_fee_bps
        ))
    })?;
    if relayer_fee_bps > 50 {
        return Err(CompileError::Validation(format!(
            "Across relayer_fee_bps {} exceeds cap 50 (0.5%)",
            relayer_fee_bps
        )));
    }

    let input_token = resolve_asset_address(&b.asset, registry)?;
    let decimals = resolve_asset_decimals(&b.asset, registry)?;
    let input_amount = parse_amount(&b.amount, decimals)?;
    let output_amount =
        input_amount * U256::from(10_000u64 - relayer_fee_bps) / U256::from(10_000u64);

    let quote_timestamp = script.current_timestamp.ok_or_else(|| {
        CompileError::Validation(
            "Across bridge requires 'current_timestamp' in the script (used as quote_timestamp)"
                .to_string(),
        )
    })?;
    let quote_timestamp_u32: u32 = quote_timestamp.try_into().map_err(|_| {
        CompileError::InvalidAmount(format!(
            "Across quote_timestamp {} does not fit in uint32",
            quote_timestamp
        ))
    })?;
    let fill_deadline: u32 = quote_timestamp_u32.saturating_add(4 * 3600);

    Ok(ResolvedStep::AcrossDepositV3 {
        spoke_pool,
        depositor: signer,
        recipient,
        input_token,
        output_token: input_token, // v1: same token on destination
        input_amount,
        output_amount,
        destination_chain_id,
        exclusive_relayer: Address::ZERO,
        quote_timestamp: quote_timestamp_u32,
        fill_deadline,
        exclusivity_deadline: 0,
        message: Bytes::new(),
    })
}

fn normalize_flashloan(
    f: &crate::schema::FlashloanStep,
    signer: Address,
    registry: &RegistryContext,
    warnings: &mut Vec<String>,
    script: &IntentScript,
) -> Result<ResolvedStep> {
    if f.via != "balancer" && f.via != "aave" {
        return Err(CompileError::UnsupportedStep(format!(
            "flashloan via '{}' is not supported (use 'balancer' or 'aave')",
            f.via
        )));
    }
    if f.assets.is_empty() {
        return Err(CompileError::Validation(
            "flashloan requires at least one asset".to_string(),
        ));
    }

    let mut tokens = Vec::with_capacity(f.assets.len());
    let mut amounts = Vec::with_capacity(f.assets.len());
    for asset in &f.assets {
        let addr = resolve_asset_address(&asset.asset, registry)?;
        if addr == Address::ZERO {
            return Err(CompileError::Validation(
                "flashloan asset cannot be native — use WETH or another ERC-20".to_string(),
            ));
        }
        let dec = resolve_asset_decimals(&asset.asset, registry)?;
        let amt = parse_amount(&asset.amount, dec)?;
        tokens.push(addr);
        amounts.push(amt);
    }

    // Recursively normalize the inner pipeline. Inner "all" keywords and
    // cross-step amount flow reference only the inner pipeline, not outer
    // steps — each inner step sees the inner-built-so-far slice.
    let mut inner_steps: Vec<ResolvedStep> = Vec::new();
    for step in &f.then {
        // Reject nested flashloans here for a crisper error than validate's
        // recursive check would give.
        if matches!(step, Step::Flashloan(_)) {
            return Err(CompileError::Validation(
                "nested flashloans are not allowed (max depth 1)".to_string(),
            ));
        }
        // Inner pipelines can prepend auto-injected steps (e.g. Wrap for a
        // native-ETH lp_mint inside a flashloan) just like the top-level loop.
        let mut prepend: Vec<ResolvedStep> = Vec::new();
        let resolved = normalize_step(
            step,
            signer,
            registry,
            warnings,
            script,
            &inner_steps,
            &mut prepend,
        )?;
        inner_steps.extend(prepend);
        inner_steps.push(resolved);
    }

    if f.via == "aave" {
        // Aave V3 `flashLoanSimple` is single-asset.
        if f.assets.len() != 1 {
            return Err(CompileError::UnsupportedStep(
                "Aave flashloans support a single asset only (use 'balancer' for multi-asset)"
                    .to_string(),
            ));
        }
        let aave = registry
            .protocols
            .get("aave")
            .ok_or_else(|| CompileError::UnknownProtocol {
                protocol: "aave".to_string(),
                network: registry.network.clone(),
                available: known_protocols(registry),
            })?;
        let pool_addr = aave.contracts.get("pool").ok_or_else(|| {
            CompileError::Adapter("Protocol 'aave' has no 'pool' contract configured".to_string())
        })?;
        let pool = parse_address(pool_addr)?;
        let premium_bps = registry.aave_flashloan_premium_bps();
        return Ok(ResolvedStep::AaveFlashloan {
            pool,
            asset: tokens[0],
            amount: amounts[0],
            premium_bps,
            inner_steps,
        });
    }

    let balancer =
        registry
            .protocols
            .get("balancer")
            .ok_or_else(|| CompileError::UnknownProtocol {
                protocol: "balancer".to_string(),
                network: registry.network.clone(),
                available: known_protocols(registry),
            })?;
    let vault_addr = balancer.contracts.get("vault").ok_or_else(|| {
        CompileError::Adapter("Protocol 'balancer' has no 'vault' contract configured".to_string())
    })?;
    let vault = parse_address(vault_addr)?;

    Ok(ResolvedStep::BalancerFlashloan {
        vault,
        tokens,
        amounts,
        inner_steps,
    })
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
        let wsteth_rate = registry.wsteth_steth_rate_bps();
        for step in previous_steps.iter().rev() {
            if let Some((produced_token, guaranteed)) = step_produces(step, fee_bps, wsteth_rate)
                && produced_token == token
            {
                if guaranteed == U256::ZERO {
                    return Err(CompileError::InvalidChain(
                        "Cannot use 'all': previous step has zero guaranteed output".into(),
                    ));
                }
                return Ok(guaranteed);
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
        // B2: Absolute slippage floor. 5% is the ceiling; anything above
        // indicates either a hallucinated value or a pool so thin that the
        // user should reconsider. Matches the leverage-sugar cap already
        // enforced in leverage.rs.
        if slippage_bps > crate::compiler::validate::MAX_SLIPPAGE_BPS {
            return Err(CompileError::InvalidAmount(format!(
                "Slippage {} bps exceeds the absolute cap of {} bps ({}%). \
                 If you genuinely need wider slippage, re-quote or split the \
                 swap; otherwise tighten the value.",
                slippage_bps,
                crate::compiler::validate::MAX_SLIPPAGE_BPS,
                crate::compiler::validate::MAX_SLIPPAGE_BPS / 100
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

/// Returns `true` for asset symbols that are USD-pegged stablecoins. Used to
/// pick a smarter default Uniswap V3 fee tier for stable-stable pairs (the
/// 0.01% / fee=100 tier holds the deep liquidity for USDC↔USDT, USDC↔DAI
/// etc.). Case-insensitive to tolerate "usdc" vs "USDC" hand-typed input.
///
/// Mirrors the UI's `STABLE_SYMBOLS` set in
/// `intentOS-ui/lib/portfolio-summary.ts` and `lib/uniswap-v3-price.ts`; the
/// two layers are deliberately duplicated rather than abstracted into a
/// shared module since the WASM compiler has no JS imports. Any new stable
/// added one place should be added the other too.
fn is_stable_symbol(alias: &str) -> bool {
    matches!(
        alias.to_ascii_uppercase().as_str(),
        "USDC" | "USDT" | "USDBC" | "DAI" | "USDE" | "FRAX" | "SDAI"
    )
}

/// Parse a Uniswap V3 fee tier from a string, accepting only the canonical
/// values. Fee 100 (0.01%) is the canonical stable-stable tier (USDC/USDT
/// etc.) — it must be accepted here so swaps between two stables route to
/// the correct pool. Without it, a USDC↔USDT swap on Base falls back to
/// fee 3000, where the pool may not exist, and the SwapRouter02 calldata
/// reverts with no reason (empty return data from the address-zero pool
/// fails to decode).
fn parse_uniswap_fee_tier(s: &str) -> Result<u32> {
    match s {
        "100" => Ok(100),
        "500" => Ok(500),
        "3000" => Ok(3000),
        "10000" => Ok(10000),
        other => Err(CompileError::UniswapFeeTierUnknown {
            fee: other.to_string(),
        }),
    }
}

/// Canonical tick spacing for a given Uniswap V3 fee tier.
fn uniswap_tick_spacing(fee: u32) -> i32 {
    match fee {
        100 => 1,
        500 => 10,
        3000 => 60,
        10000 => 200,
        // parse_uniswap_fee_tier gates inputs to this table; any other value
        // here signals a caller bug.
        _ => 1,
    }
}

/// Decide whether the user's `quote_token` refers to token1 (canonical
/// direction) or token0 (inverted). `token0_alias` / `token1_alias` are the
/// *sorted* aliases — i.e. the ones that match the pool's on-chain ordering
/// — so the returned direction is correct regardless of how the user wrote
/// the pair in the DSL.
fn classify_quote_direction(
    quote_token: &str,
    token0_alias: &str,
    token1_alias: &str,
) -> Result<bool> {
    if quote_token == token1_alias {
        Ok(true)
    } else if quote_token == token0_alias {
        Ok(false)
    } else {
        Err(CompileError::InvalidChain(format!(
            "LP quote_token '{}' must equal token0 ('{}') or token1 ('{}')",
            quote_token, token0_alias, token1_alias
        )))
    }
}

/// Resolve an `lp_mint` step's tick range, accepting either the price form
/// (preferred, human-friendly) or the raw tick form (advanced). Exactly one
/// of the two shapes must be supplied per bound. Price-derived ticks are
/// snapped to the fee tier's spacing with a warning describing the snap.
#[allow(clippy::too_many_arguments)]
fn resolve_lp_mint_ticks(
    m: &crate::schema::public_ast::LpMintStep,
    token0_alias: &str,
    token1_alias: &str,
    tokens_swapped: bool,
    dec0: u8,
    dec1: u8,
    spacing: i32,
    _warnings: &mut Vec<String>,
    // When the caller rewrote the DSL's `quote_token` alias to match a
    // substituted pair token (e.g. native "ETH" → "WETH"), pass it here so
    // the string-equality match below sees the same normalized name.
    quote_token_override: Option<&str>,
) -> Result<(i32, i32)> {
    use crate::compiler::uniswap_ticks::{
        maybe_swap_inverted, resolve_lower_bound, resolve_upper_bound, snap_range,
    };

    let has_price = m.price_lower.is_some() || m.price_upper.is_some();
    let has_tick = m.tick_lower.is_some() || m.tick_upper.is_some();

    // Reject mixed / missing shapes up front.
    if has_price && has_tick {
        return Err(CompileError::InvalidChain(
            "lp_mint accepts EITHER price_lower/price_upper OR tick_lower/tick_upper, not both"
                .to_string(),
        ));
    }
    if !has_price && !has_tick {
        return Err(CompileError::InvalidChain(
            "lp_mint is missing a range: supply price_lower + price_upper (preferred) or tick_lower + tick_upper"
                .to_string(),
        ));
    }

    if has_tick {
        let lower = m.tick_lower.ok_or_else(|| {
            CompileError::InvalidChain(
                "lp_mint tick_lower is required when using the tick form".to_string(),
            )
        })?;
        let upper = m.tick_upper.ok_or_else(|| {
            CompileError::InvalidChain(
                "lp_mint tick_upper is required when using the tick form".to_string(),
            )
        })?;
        return Ok((lower, upper));
    }

    // Price form.
    let lower_raw = m.price_lower.as_deref().ok_or_else(|| {
        CompileError::InvalidChain(
            "lp_mint price_lower is required when using the price form".to_string(),
        )
    })?;
    let upper_raw = m.price_upper.as_deref().ok_or_else(|| {
        CompileError::InvalidChain(
            "lp_mint price_upper is required when using the price form".to_string(),
        )
    })?;
    let quote_token_raw = m.quote_token.as_deref().ok_or_else(|| {
        CompileError::InvalidChain(
            "lp_mint quote_token is required when using price_lower / price_upper".to_string(),
        )
    })?;
    let quote_token = quote_token_override.unwrap_or(quote_token_raw);

    let quote_is_token1 = classify_quote_direction(quote_token, token0_alias, token1_alias)?;

    let lower_tick = resolve_lower_bound(lower_raw, quote_is_token1, dec0, dec1)?;
    let upper_tick = resolve_upper_bound(upper_raw, quote_is_token1, dec0, dec1)?;
    // Inverting the quote direction flips which bound is numerically smaller,
    // so swap the pair back into (low, high) tick order.
    let (lower_tick, upper_tick) = maybe_swap_inverted(lower_tick, upper_tick, quote_is_token1);

    let (snapped_lo, snapped_hi) = snap_range(lower_tick, upper_tick, spacing);
    // Snapping is an implementation detail — the realized range always
    // contains the user's requested prices, so we don't emit a warning.
    let _ = tokens_swapped; // kept for signature symmetry.
    Ok((snapped_lo, snapped_hi))
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
    if let Some(d) = step_deadline
        && d > 0
    {
        return d;
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

/// Resolve a Morpho market alias to its parsed parameters.
fn resolve_morpho_market(
    market_alias: &str,
    registry: &RegistryContext,
) -> Result<(Address, MorphoMarketParams, MorphoMarketConfig)> {
    let protocol =
        registry
            .protocols
            .get("morpho")
            .ok_or_else(|| CompileError::UnknownProtocol {
                protocol: "morpho".to_string(),
                network: registry.network.clone(),
                available: known_protocols(registry),
            })?;

    let pool_addr =
        protocol
            .contracts
            .get("pool")
            .ok_or_else(|| CompileError::ProtocolContractMissing {
                protocol: "morpho".to_string(),
                contract: "pool".to_string(),
            })?;
    let pool = parse_address(pool_addr)?;

    let markets =
        protocol
            .markets
            .as_ref()
            .ok_or_else(|| CompileError::ProtocolContractMissing {
                protocol: "morpho".to_string(),
                contract: "markets".to_string(),
            })?;

    let market = markets.get(market_alias).ok_or_else(|| {
        CompileError::Validation(format!(
            "Unknown Morpho market '{}'. Available: {}",
            market_alias,
            {
                let mut keys: Vec<String> = markets.keys().cloned().collect();
                keys.sort();
                keys.join(", ")
            }
        ))
    })?;

    let loan_token = resolve_asset_address(&market.loan, registry)?;
    let collateral_token = resolve_asset_address(&market.collateral, registry)?;
    let oracle = parse_address(&market.oracle)?;
    let irm = parse_address(&market.irm)?;
    let lltv = U256::from_str_radix(&market.lltv, 10).map_err(|_| {
        CompileError::InvalidAmount(format!("Invalid Morpho lltv: {}", market.lltv))
    })?;

    // Parse the 32-byte market id (keccak256 of abi.encode(MarketParams)).
    let id_hex = market.id.strip_prefix("0x").unwrap_or(&market.id);
    if id_hex.len() != 64 {
        return Err(CompileError::Adapter(format!(
            "Morpho market '{}' has invalid id length: expected 32 bytes, got '{}'",
            market_alias, market.id
        )));
    }
    let mut id = [0u8; 32];
    for (i, byte_out) in id.iter_mut().enumerate() {
        *byte_out = u8::from_str_radix(&id_hex[i * 2..i * 2 + 2], 16).map_err(|_| {
            CompileError::Adapter(format!(
                "Morpho market '{}' has invalid id hex: {}",
                market_alias, market.id
            ))
        })?;
    }

    Ok((
        pool,
        MorphoMarketParams {
            loan_token,
            collateral_token,
            oracle,
            irm,
            lltv,
            id,
        },
        market.clone(),
    ))
}

fn normalize_morpho_deposit(
    d: &crate::schema::DepositStep,
    signer: Address,
    registry: &RegistryContext,
    previous_steps: &[ResolvedStep],
) -> Result<ResolvedStep> {
    let market_alias = d
        .market
        .as_deref()
        .ok_or(CompileError::MorphoMarketRequired)?;
    let (pool, params, market_cfg) = resolve_morpho_market(market_alias, registry)?;

    let is_collateral = match d.r#as.as_deref() {
        None | Some("loan") => false,
        Some("collateral") => true,
        Some(other) => {
            return Err(CompileError::Validation(format!(
                "Morpho deposit 'as' must be 'collateral' or 'loan' (got '{}')",
                other
            )));
        }
    };

    // Asset must match the market's loan side (for non-collateral deposits)
    // or collateral side (for collateral deposits).
    let expected_alias = if is_collateral {
        &market_cfg.collateral
    } else {
        &market_cfg.loan
    };
    if &d.asset != expected_alias {
        return Err(CompileError::Validation(format!(
            "Morpho market '{}' expects asset '{}' for {} supply (got '{}')",
            market_alias,
            expected_alias,
            if is_collateral { "collateral" } else { "loan" },
            d.asset
        )));
    }

    let asset_addr = resolve_asset_address(&d.asset, registry)?;
    let decimals = resolve_asset_decimals(&d.asset, registry)?;
    let amount = resolve_amount_or_all(
        &d.amount,
        decimals,
        asset_addr,
        previous_steps,
        &d.asset,
        registry,
    )?;

    if is_collateral {
        Ok(ResolvedStep::MorphoSupplyCollat {
            pool,
            market_params: params,
            amount,
            on_behalf: signer,
        })
    } else {
        Ok(ResolvedStep::MorphoSupply {
            pool,
            market_params: params,
            amount,
            on_behalf: signer,
        })
    }
}

fn normalize_morpho_borrow(
    b: &crate::schema::BorrowStep,
    signer: Address,
    registry: &RegistryContext,
    previous_steps: &[ResolvedStep],
) -> Result<ResolvedStep> {
    let market_alias = b
        .market
        .as_deref()
        .ok_or(CompileError::MorphoMarketRequired)?;
    let (pool, params, market_cfg) = resolve_morpho_market(market_alias, registry)?;

    if b.asset != market_cfg.loan {
        return Err(CompileError::Validation(format!(
            "Morpho market '{}' expects loan asset '{}' (got '{}')",
            market_alias, market_cfg.loan, b.asset
        )));
    }

    let asset_addr = resolve_asset_address(&b.asset, registry)?;
    let decimals = resolve_asset_decimals(&b.asset, registry)?;
    let amount = resolve_amount_or_all(
        &b.amount,
        decimals,
        asset_addr,
        previous_steps,
        &b.asset,
        registry,
    )?;

    Ok(ResolvedStep::MorphoBorrow {
        pool,
        market_params: params,
        amount,
        on_behalf: signer,
        receiver: signer,
    })
}

fn normalize_morpho_withdraw(
    w: &crate::schema::WithdrawStep,
    signer: Address,
    registry: &RegistryContext,
    previous_steps: &[ResolvedStep],
) -> Result<ResolvedStep> {
    let market_alias = w
        .market
        .as_deref()
        .ok_or(CompileError::MorphoMarketRequired)?;
    let (pool, params, market_cfg) = resolve_morpho_market(market_alias, registry)?;

    let is_collateral = match w.r#as.as_deref() {
        None | Some("loan") => false,
        Some("collateral") => true,
        Some(other) => {
            return Err(CompileError::Validation(format!(
                "Morpho withdraw 'as' must be 'collateral' or 'loan' (got '{}')",
                other
            )));
        }
    };
    let expected_alias = if is_collateral {
        &market_cfg.collateral
    } else {
        &market_cfg.loan
    };
    if &w.asset != expected_alias {
        return Err(CompileError::Validation(format!(
            "Morpho market '{}' expects asset '{}' for {} withdraw (got '{}')",
            market_alias,
            expected_alias,
            if is_collateral { "collateral" } else { "loan" },
            w.asset
        )));
    }

    let asset_addr = resolve_asset_address(&w.asset, registry)?;
    let decimals = resolve_asset_decimals(&w.asset, registry)?;
    let amount = resolve_amount_or_all(
        &w.amount,
        decimals,
        asset_addr,
        previous_steps,
        &w.asset,
        registry,
    )?;

    if is_collateral {
        Ok(ResolvedStep::MorphoWithdrawCollat {
            pool,
            market_params: params,
            amount,
            on_behalf: signer,
            receiver: signer,
        })
    } else {
        Ok(ResolvedStep::MorphoWithdraw {
            pool,
            market_params: params,
            amount,
            on_behalf: signer,
            receiver: signer,
        })
    }
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
