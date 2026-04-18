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

    // Compute effective intent deadline (Task 2)
    let effective_deadline = match script.deadline {
        Some(d) if d > 0 => d,
        _ => match script.current_timestamp {
            Some(ts) => ts + DEFAULT_INTENT_DEADLINE_SECS,
            None => 0, // backward compat when no timestamp provided
        },
    };
    if effective_deadline == 0 {
        warnings.push(
            "Intent has no deadline: neither 'deadline' nor 'current_timestamp' was provided. \
             Batched intents will be rejected by the router (deadline > 0 required). \
             Set 'current_timestamp' to the current Unix timestamp to auto-compute a 30-minute deadline."
                .to_string(),
        );
    }

    let mut steps = Vec::new();
    for step in &script.steps {
        let resolved = normalize_step(step, signer, registry, &mut warnings, script, &steps)?;
        steps.push(resolved);
    }

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

                    // If swapping from native ETH, use WETH address as token_in
                    // (Uniswap V3 router wraps ETH internally)
                    let effective_token_in = if token_in == Address::ZERO {
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
                    lido: contract,
                    amount,
                    referral: Address::ZERO,
                }),
                _ => Err(CompileError::UnsupportedStep(format!(
                    "staking into '{}' is not yet supported",
                    s.into
                ))),
            }
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
    _registry: &RegistryContext,
) -> Result<U256> {
    if amount_str == "all" {
        for step in previous_steps.iter().rev() {
            if let Some((produced_token, guaranteed)) = step_produces(step) {
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
