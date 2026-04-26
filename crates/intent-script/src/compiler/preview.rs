//! Structured preview builder.
//!
//! Walks the resolved IR to produce a user-facing summary of what the intent
//! consumes, what it produces, and the ordered sequence of meaningful steps.
//! Auto-inserted approvals and router transfers are excluded from `steps`.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use alloy_primitives::{Address, U256};
use hashbrown::HashMap;

use crate::ir::{ResolvedIntent, ResolvedStep, step_consumes, step_produces};
use crate::output::{Preview, PreviewStepInfo, PreviewToken};
use crate::registry::RegistryContext;

/// Build a user-facing preview from a resolved intent.
pub fn build_preview(intent: &ResolvedIntent, registry: &RegistryContext) -> Preview {
    let mut inputs: HashMap<Address, U256> = HashMap::new();
    let mut outputs: HashMap<Address, U256> = HashMap::new();

    // Aggregate at RAW amounts (fee_bps=0). The router's sweep-time fee only
    // hits tokens that actually leave the router back to the user — i.e. the
    // net outflow after intermediate overlap is cancelled. If we applied the
    // fee here, an intermediate hop like `wrap 100 ETH → deposit 100 WETH`
    // would net to `-0.1 WETH` (100 consumed − 99.9 produced) instead of
    // cancelling cleanly. Fee is applied to the surviving outputs below.
    for step in &intent.steps {
        if let Some((token, amount)) = step_consumes(step) {
            *inputs.entry(token).or_insert(U256::ZERO) += amount;
        }
        if let Some((token, amount)) = step_produces(step, 0) {
            *outputs.entry(token).or_insert(U256::ZERO) += amount;
        }
        // Flashloans are self-contained router-side accounting — the vault
        // seeds the inner pipeline and the pipeline repays at the end, so
        // `step_consumes` / `step_produces` above correctly return None for
        // the outer `BalancerFlashloan`. What does NOT net inside that scope
        // is the user's equity contribution, which the leverage-sugar expander
        // emits as an explicit `Erc20TransferFrom` from the signer to the
        // router. Surface those transferFroms as user-visible inputs so the
        // preview card can show "You send: 5 WETH" for a leveraged long
        // instead of an empty "inputs" list.
        if let ResolvedStep::BalancerFlashloan { inner_steps, .. } = step {
            for inner in inner_steps {
                if let ResolvedStep::Erc20TransferFrom {
                    from,
                    token,
                    amount,
                    ..
                } = inner
                    && *from == intent.signer
                {
                    *inputs.entry(*token).or_insert(U256::ZERO) += *amount;
                }
            }
        }
    }

    // Net out: tokens that appear on both sides are intermediate — drop the
    // overlap so the preview only shows the user's actual send/receive.
    //
    // We tolerate a small residual — up to `produced * fee_bps / 10_000` —
    // and treat it as intermediate. This absorbs the "all"+fee_bps asymmetry:
    // when a consumer uses `"amount": "all"`, the compiler resolves that
    // concrete amount against `step_produces` with fee_bps applied (so later
    // sweep accounting stays conservative). The consumed amount therefore
    // ends up at `produced * (1 - fee_bps/10_000)`, leaving a `fee_bps`-sized
    // residual vs. the raw produced amount we aggregated above. Without the
    // tolerance, every swap→deposit-all chain would show a spurious dust
    // leftover equal to the fee_bps percentage of the intermediate.
    let overlap: Vec<Address> = inputs
        .keys()
        .filter(|k| outputs.contains_key(*k))
        .copied()
        .collect();
    for token in overlap {
        let consumed = *inputs.get(&token).unwrap_or(&U256::ZERO);
        let produced = *outputs.get(&token).unwrap_or(&U256::ZERO);
        let tolerance = if intent.fee_bps > 0 {
            produced * U256::from(intent.fee_bps as u64) / U256::from(10_000u64)
        } else {
            U256::ZERO
        };
        if consumed > produced {
            let diff = consumed - produced;
            if diff <= tolerance {
                inputs.remove(&token);
            } else {
                inputs.insert(token, diff);
            }
            outputs.remove(&token);
        } else if produced > consumed {
            let diff = produced - consumed;
            if diff <= tolerance {
                outputs.remove(&token);
            } else {
                outputs.insert(token, diff);
            }
            inputs.remove(&token);
        } else {
            // Exactly cancel out — pure intermediate.
            inputs.remove(&token);
            outputs.remove(&token);
        }
    }

    // Apply the router sweep fee only to the surviving outputs — the
    // leftover tokens that actually get swept back to the user at the end
    // of the intent. Intermediate produces already netted out against
    // consumes in the loop above.
    if intent.fee_bps > 0 {
        let fee_num = U256::from(10_000u64 - intent.fee_bps as u64);
        let fee_den = U256::from(10_000u64);
        for amount in outputs.values_mut() {
            *amount = *amount * fee_num / fee_den;
        }
    }

    Preview {
        inputs: to_preview_tokens(&inputs, registry),
        outputs: to_preview_tokens(&outputs, registry),
        steps: intent
            .steps
            .iter()
            .filter_map(|s| describe_step(s, registry))
            .collect(),
    }
}

fn to_preview_tokens(
    map: &HashMap<Address, U256>,
    registry: &RegistryContext,
) -> Vec<PreviewToken> {
    let mut out: Vec<PreviewToken> = map
        .iter()
        .map(|(addr, amount)| {
            let symbol = registry.symbol_for_address(addr);
            let decimals = registry.decimals_for_address(addr);
            PreviewToken {
                symbol: symbol.clone(),
                amount: format_amount(*amount, decimals),
                amount_raw: amount.to_string(),
                address: if *addr == Address::ZERO {
                    "native".to_string()
                } else {
                    format!("{:?}", addr)
                },
            }
        })
        .collect();
    // Stable ordering for deterministic output.
    out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    out
}

/// Format a raw amount with the given decimals, trimming trailing zeros but
/// keeping at least one decimal digit when fractional.
fn format_amount(amount: U256, decimals: u8) -> String {
    if decimals == 0 {
        return amount.to_string();
    }
    let s = amount.to_string();
    let d = decimals as usize;
    if s.len() <= d {
        let pad = d - s.len();
        let mut frac = "0".repeat(pad);
        frac.push_str(&s);
        trim_zeros(&format!("0.{}", frac))
    } else {
        let split = s.len() - d;
        let (whole, frac) = s.split_at(split);
        trim_zeros(&format!("{}.{}", whole, frac))
    }
}

fn trim_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Render a user-friendly "range X–Y <quote> per <base>" description for an
/// LP position. We never surface raw ticks: the full-range sentinels map to
/// "full range" and explicit bounds are converted back to human prices.
/// Direction is auto-picked so prices tend to read ≥ 1 (a 3000 USDC/WETH
/// pool reads as "USDC per WETH", not "0.000333 WETH per USDC").
fn describe_lp_price_range(
    tick_lower: i32,
    tick_upper: i32,
    dec0: u8,
    dec1: u8,
    sym0: &str,
    sym1: &str,
) -> String {
    use crate::compiler::uniswap_ticks::{MAX_TICK, MIN_TICK, tick_to_price};

    // Full-range (or effectively full-range after snapping) deserves its own
    // copy — showing 1.0001^±887220 as a decimal is noise.
    let spacing_slack = 300; // more than any fee-tier spacing, conservatively wide.
    let is_min = tick_lower <= MIN_TICK + spacing_slack;
    let is_max = tick_upper >= MAX_TICK - spacing_slack;
    if is_min && is_max {
        return "full range".to_string();
    }

    // Compute canonical (token1 per token0) prices, then invert if the values
    // are both below 1 so the user sees a "nicer" number. This is a display
    // heuristic only — the underlying range is unchanged.
    let p_lo_canon = tick_to_price(tick_lower, true, dec0, dec1);
    let p_hi_canon = tick_to_price(tick_upper, true, dec0, dec1);

    let (p_lo, p_hi, quote, base) = if p_lo_canon.max(p_hi_canon) < 1.0 {
        // Invert: show token0 per token1 (larger numerator).
        (1.0 / p_hi_canon, 1.0 / p_lo_canon, sym0, sym1)
    } else {
        (p_lo_canon, p_hi_canon, sym1, sym0)
    };

    format!(
        "range {} to {} {} per {}",
        format_price(p_lo),
        format_price(p_hi),
        quote,
        base,
    )
}

/// Format a price for human display with sensible precision.
fn format_price(p: f64) -> String {
    if !p.is_finite() || p <= 0.0 {
        return "?".to_string();
    }
    // Pick a precision that keeps small stablecoin deltas visible while not
    // drowning larger numbers in trailing decimals.
    let formatted = if p >= 1000.0 {
        format!("{:.2}", p)
    } else if p >= 1.0 {
        format!("{:.4}", p)
    } else {
        format!("{:.6}", p)
    };
    trim_zeros(&formatted)
}

/// Produce a preview step entry for user-meaningful steps. Returns None for
/// auto-inserted approvals, transferFroms, and permits.
fn describe_step(step: &ResolvedStep, registry: &RegistryContext) -> Option<PreviewStepInfo> {
    match step {
        ResolvedStep::Erc20Approve { .. }
        | ResolvedStep::Erc20TransferFrom { .. }
        | ResolvedStep::Erc20Permit { .. } => None,

        ResolvedStep::Wrap {
            wrapped_token,
            amount,
        } => {
            let sym = registry.symbol_for_address(wrapped_token);
            let dec = registry.decimals_for_address(wrapped_token);
            Some(PreviewStepInfo {
                action: "wrap".into(),
                protocol: "weth".into(),
                description: format!(
                    "Wrap {} {} to {}",
                    format_amount(*amount, dec),
                    registry.chain.native_asset,
                    sym
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::Unwrap {
            wrapped_token,
            amount,
        } => {
            let sym = registry.symbol_for_address(wrapped_token);
            let dec = registry.decimals_for_address(wrapped_token);
            Some(PreviewStepInfo {
                action: "unwrap".into(),
                protocol: "weth".into(),
                description: format!(
                    "Unwrap {} {} to {}",
                    format_amount(*amount, dec),
                    sym,
                    registry.chain.native_asset
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::UniswapV3Swap {
            token_in,
            token_out,
            amount_in,
            amount_out_minimum,
            native_input,
            ..
        } => {
            // For native-input swaps, the calldata carries WETH but the user
            // is actually paying the native asset — surface that in the UI.
            let (in_sym, in_dec) = if *native_input {
                (
                    registry.chain.native_asset.clone(),
                    registry.decimals_for_address(&Address::ZERO),
                )
            } else {
                (
                    registry.symbol_for_address(token_in),
                    registry.decimals_for_address(token_in),
                )
            };
            let out_sym = registry.symbol_for_address(token_out);
            let out_dec = registry.decimals_for_address(token_out);
            Some(PreviewStepInfo {
                action: "swap".into(),
                protocol: "uniswap_v3".into(),
                description: format!(
                    "Swap {} {} for at least {} {}",
                    format_amount(*amount_in, in_dec),
                    in_sym,
                    format_amount(*amount_out_minimum, out_dec),
                    out_sym
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::AaveV3Supply { asset, amount, .. } => {
            let sym = registry.symbol_for_address(asset);
            let dec = registry.decimals_for_address(asset);
            Some(PreviewStepInfo {
                action: "deposit".into(),
                protocol: "aave_v3".into(),
                description: format!("Deposit {} {} to Aave V3", format_amount(*amount, dec), sym),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::AaveV3Borrow { asset, amount, .. } => {
            let sym = registry.symbol_for_address(asset);
            let dec = registry.decimals_for_address(asset);
            Some(PreviewStepInfo {
                action: "borrow".into(),
                protocol: "aave_v3".into(),
                description: format!(
                    "Borrow {} {} from Aave V3",
                    format_amount(*amount, dec),
                    sym
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::AaveV3Withdraw { asset, amount, .. } => {
            let sym = registry.symbol_for_address(asset);
            let dec = registry.decimals_for_address(asset);
            Some(PreviewStepInfo {
                action: "withdraw".into(),
                protocol: "aave_v3".into(),
                description: format!(
                    "Withdraw {} {} from Aave V3",
                    format_amount(*amount, dec),
                    sym
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::AaveV3Repay { asset, amount, .. } => {
            let sym = registry.symbol_for_address(asset);
            let dec = registry.decimals_for_address(asset);
            Some(PreviewStepInfo {
                action: "repay".into(),
                protocol: "aave_v3".into(),
                description: format!("Repay {} {} to Aave V3", format_amount(*amount, dec), sym),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::MorphoSupply {
            market_params,
            amount,
            ..
        } => {
            let sym = registry.symbol_for_address(&market_params.loan_token);
            let dec = registry.decimals_for_address(&market_params.loan_token);
            Some(PreviewStepInfo {
                action: "deposit".into(),
                protocol: "morpho".into(),
                description: format!(
                    "Supply {} {} to Morpho Blue",
                    format_amount(*amount, dec),
                    sym
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::MorphoSupplyCollat {
            market_params,
            amount,
            ..
        } => {
            let sym = registry.symbol_for_address(&market_params.collateral_token);
            let dec = registry.decimals_for_address(&market_params.collateral_token);
            Some(PreviewStepInfo {
                action: "deposit".into(),
                protocol: "morpho".into(),
                description: format!(
                    "Supply {} {} as collateral to Morpho Blue",
                    format_amount(*amount, dec),
                    sym
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::MorphoBorrow {
            market_params,
            amount,
            ..
        } => {
            let sym = registry.symbol_for_address(&market_params.loan_token);
            let dec = registry.decimals_for_address(&market_params.loan_token);
            Some(PreviewStepInfo {
                action: "borrow".into(),
                protocol: "morpho".into(),
                description: format!(
                    "Borrow {} {} from Morpho Blue",
                    format_amount(*amount, dec),
                    sym
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::MorphoWithdraw {
            market_params,
            amount,
            ..
        } => {
            let sym = registry.symbol_for_address(&market_params.loan_token);
            let dec = registry.decimals_for_address(&market_params.loan_token);
            Some(PreviewStepInfo {
                action: "withdraw".into(),
                protocol: "morpho".into(),
                description: format!(
                    "Withdraw {} {} from Morpho Blue",
                    format_amount(*amount, dec),
                    sym
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::MorphoWithdrawCollat {
            market_params,
            amount,
            ..
        } => {
            let sym = registry.symbol_for_address(&market_params.collateral_token);
            let dec = registry.decimals_for_address(&market_params.collateral_token);
            Some(PreviewStepInfo {
                action: "withdraw".into(),
                protocol: "morpho".into(),
                description: format!(
                    "Withdraw {} {} collateral from Morpho Blue",
                    format_amount(*amount, dec),
                    sym
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::MorphoRepay {
            market_params,
            amount,
            ..
        } => {
            let sym = registry.symbol_for_address(&market_params.loan_token);
            let dec = registry.decimals_for_address(&market_params.loan_token);
            Some(PreviewStepInfo {
                action: "repay".into(),
                protocol: "morpho".into(),
                description: format!(
                    "Repay {} {} to Morpho Blue",
                    format_amount(*amount, dec),
                    sym
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::LidoStake { amount, .. } => Some(PreviewStepInfo {
            action: "stake".into(),
            protocol: "lido".into(),
            description: format!(
                "Stake {} {} with Lido",
                format_amount(*amount, 18),
                registry.chain.native_asset
            ),
            inner_steps: Vec::new(),
        }),
        ResolvedStep::WstETHWrap { amount, .. } => Some(PreviewStepInfo {
            action: "wrap".into(),
            protocol: "lido".into(),
            description: format!("Wrap {} stETH to wstETH", format_amount(*amount, 18)),
            inner_steps: Vec::new(),
        }),
        ResolvedStep::WstETHUnwrap { amount, .. } => Some(PreviewStepInfo {
            action: "unwrap".into(),
            protocol: "lido".into(),
            description: format!("Unwrap {} wstETH to stETH", format_amount(*amount, 18)),
            inner_steps: Vec::new(),
        }),
        ResolvedStep::LidoRequestWithdrawal {
            amounts, is_wsteth, ..
        } => {
            let label = if *is_wsteth { "wstETH" } else { "stETH" };
            let total = amounts
                .iter()
                .copied()
                .fold(U256::ZERO, |acc, a| acc.saturating_add(a));
            Some(PreviewStepInfo {
                action: "request_withdrawal".into(),
                protocol: "lido".into(),
                description: format!(
                    "Request Lido withdrawal of {} {} ({} NFT{})",
                    format_amount(total, 18),
                    label,
                    amounts.len(),
                    if amounts.len() == 1 { "" } else { "s" }
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::LidoClaimWithdrawal { request_ids, .. } => Some(PreviewStepInfo {
            action: "claim_withdrawal".into(),
            protocol: "lido".into(),
            description: format!(
                "Claim {} Lido withdrawal NFT{}",
                request_ids.len(),
                if request_ids.len() == 1 { "" } else { "s" }
            ),
            inner_steps: Vec::new(),
        }),
        ResolvedStep::UniswapV3LpMint {
            token0,
            token1,
            fee,
            tick_lower,
            tick_upper,
            amount0,
            amount1,
            ..
        } => {
            let sym0 = registry.symbol_for_address(token0);
            let sym1 = registry.symbol_for_address(token1);
            let dec0 = registry.decimals_for_address(token0);
            let dec1 = registry.decimals_for_address(token1);
            let range = describe_lp_price_range(*tick_lower, *tick_upper, dec0, dec1, &sym0, &sym1);
            Some(PreviewStepInfo {
                action: "lp_mint".into(),
                protocol: "uniswap".into(),
                description: format!(
                    "Mint Uniswap V3 LP {}/{} ({}bp): {} {} + {} {} — {}",
                    sym0,
                    sym1,
                    fee,
                    format_amount(*amount0, dec0),
                    sym0,
                    format_amount(*amount1, dec1),
                    sym1,
                    range,
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::UniswapV3LpIncrease {
            token0,
            token1,
            token_id,
            amount0,
            amount1,
            ..
        } => {
            let sym0 = registry.symbol_for_address(token0);
            let sym1 = registry.symbol_for_address(token1);
            let dec0 = registry.decimals_for_address(token0);
            let dec1 = registry.decimals_for_address(token1);
            Some(PreviewStepInfo {
                action: "lp_increase".into(),
                protocol: "uniswap".into(),
                description: format!(
                    "Increase Uniswap V3 LP #{}: +{} {} +{} {}",
                    token_id,
                    format_amount(*amount0, dec0),
                    sym0,
                    format_amount(*amount1, dec1),
                    sym1
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::UniswapV3LpDecrease {
            token_id,
            liquidity,
            ..
        } => Some(PreviewStepInfo {
            action: "lp_decrease".into(),
            protocol: "uniswap".into(),
            description: format!(
                "Decrease Uniswap V3 LP #{} by {} liquidity units",
                token_id, liquidity
            ),
            inner_steps: Vec::new(),
        }),
        ResolvedStep::UniswapV3LpCollect {
            token0,
            token1,
            token_id,
            ..
        } => {
            let sym0 = registry.symbol_for_address(token0);
            let sym1 = registry.symbol_for_address(token1);
            Some(PreviewStepInfo {
                action: "lp_collect".into(),
                protocol: "uniswap".into(),
                description: format!("Collect Uniswap V3 LP #{} ({}/{})", token_id, sym0, sym1),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::SendErc20 { token, amount, to } => {
            let sym = registry.symbol_for_address(token);
            let dec = registry.decimals_for_address(token);
            Some(PreviewStepInfo {
                action: "send".into(),
                protocol: "erc20".into(),
                description: format!("Send {} {} to {:?}", format_amount(*amount, dec), sym, to),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::SendEth { amount, to } => Some(PreviewStepInfo {
            action: "send".into(),
            protocol: "native".into(),
            description: format!(
                "Send {} {} to {:?}",
                format_amount(*amount, 18),
                registry.chain.native_asset,
                to
            ),
            inner_steps: Vec::new(),
        }),
        ResolvedStep::SendErc721 { token_id, to, .. } => Some(PreviewStepInfo {
            action: "send".into(),
            protocol: "erc721".into(),
            description: format!("Send NFT #{} to {:?}", token_id, to),
            inner_steps: Vec::new(),
        }),
        ResolvedStep::AcrossDepositV3 {
            input_token,
            input_amount,
            destination_chain_id,
            ..
        } => {
            let sym = registry.symbol_for_address(input_token);
            let dec = registry.decimals_for_address(input_token);
            Some(PreviewStepInfo {
                action: "bridge".into(),
                protocol: "across".into(),
                description: format!(
                    "Bridge {} {} via Across to chain {}",
                    format_amount(*input_amount, dec),
                    sym,
                    destination_chain_id
                ),
                inner_steps: Vec::new(),
            })
        }
        ResolvedStep::BalancerFlashloan {
            tokens,
            amounts,
            inner_steps,
            ..
        } => {
            let token_summary: Vec<String> = tokens
                .iter()
                .zip(amounts.iter())
                .map(|(t, a)| {
                    let sym = registry.symbol_for_address(t);
                    let dec = registry.decimals_for_address(t);
                    format!("{} {}", format_amount(*a, dec), sym)
                })
                .collect();
            // Recurse: the inner pipeline is where the user-meaningful work
            // happens (supply, borrow, swap, …). Describing only the flashloan
            // envelope would hide the user's actual actions from the preview.
            let described_inner: Vec<PreviewStepInfo> = inner_steps
                .iter()
                .filter_map(|s| describe_step(s, registry))
                .collect();
            let inner_count = described_inner.len();
            Some(PreviewStepInfo {
                action: "flashloan".into(),
                protocol: "balancer".into(),
                description: format!(
                    "Flashloan {} via Balancer V2 with {} inner step(s)",
                    token_summary.join(" + "),
                    inner_count
                ),
                inner_steps: described_inner,
            })
        }
    }
}
