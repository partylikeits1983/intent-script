//! Leverage sugar — desugar `long` / `short` / `close_position` DSL steps
//! into a single flashloan-wrapped IR step (`BalancerFlashloan` or
//! `AaveFlashloan`) carrying an Aave supply/borrow/swap inner pipeline.
//! Provider selection is driven by the user's `via` field with a chain-aware
//! default — Balancer when the registry has it (Ethereum L1), otherwise Aave
//! (Base, where Balancer is not deployed). The flashloan primitives already
//! handle recursive enrichment and repayability validation, so this module
//! stays compiler-only sugar — no new IR for sugar, only provider routing.
//!
//! All sugar forms share a symmetric structure so the IR emitted here is the
//! same shape a power-user could hand-author via the `flashloan` primitive.
//! That makes the sugar trivial to audit (`cargo test` can diff the expanded
//! IR against a hand-written equivalent).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use alloy_primitives::{Address, U256};

use crate::compiler::normalize::parse_amount;
use crate::error::{CompileError, Result};
use crate::ir::ResolvedStep;
use crate::registry::RegistryContext;
use crate::schema::{ClosePositionStep, IntentScript, LeverageStep};

/// Wraps `long`/`short` desugaring. `side` is `Side::Long` when the DSL step
/// was `long`, `Side::Short` when `short`. They share a template: the symbol
/// meaning is encoded in the collateral/borrow choice.
#[derive(Copy, Clone, Debug)]
pub enum Side {
    Long,
    Short,
}

/// Which on-chain protocol provides the flashloan that wraps the inner
/// supply/borrow/swap pipeline. Selected per-leverage step via the `via`
/// DSL field, with a chain-aware default (Balancer when available, else Aave).
#[derive(Copy, Clone, Debug)]
enum FlashloanProvider {
    Balancer,
    Aave,
}

/// Resolve a user-supplied `via` string (or the lack of one) to a concrete
/// provider, validating that it is configured for the active network. The
/// default prefers Balancer when present (no premium) and falls back to Aave
/// (Base, where Balancer is not deployed).
fn select_flashloan_provider(
    via: Option<&str>,
    registry: &RegistryContext,
) -> Result<FlashloanProvider> {
    let has_balancer = registry.protocols.contains_key("balancer");
    let has_aave = registry.protocols.contains_key("aave");
    let trimmed = via.map(str::trim).filter(|s| !s.is_empty());
    match trimmed {
        Some("balancer") if has_balancer => Ok(FlashloanProvider::Balancer),
        Some("balancer") => Err(CompileError::UnsupportedStep(format!(
            "via 'balancer' is not available on network '{}' (no Balancer Vault configured); use 'aave'",
            registry.network
        ))),
        Some("aave") if has_aave => Ok(FlashloanProvider::Aave),
        Some("aave") => Err(CompileError::UnsupportedStep(format!(
            "via 'aave' is not available on network '{}' (no Aave Pool configured)",
            registry.network
        ))),
        Some(other) => Err(CompileError::UnsupportedStep(format!(
            "leverage via '{}' is not supported (use 'balancer' or 'aave')",
            other
        ))),
        None if has_balancer => Ok(FlashloanProvider::Balancer),
        None if has_aave => Ok(FlashloanProvider::Aave),
        None => Err(CompileError::UnsupportedStep(format!(
            "no flashloan provider configured for network '{}'",
            registry.network
        ))),
    }
}

const DEFAULT_SLIPPAGE_BPS: u64 = 50;
const MAX_SLIPPAGE_BPS: u64 = 500; // 5%
const LEVERAGE_SCALE: u64 = 10_000; // leverage stored as fixed-point * 10_000

pub fn expand_leverage(
    step: &LeverageStep,
    side: Side,
    signer: Address,
    registry: &RegistryContext,
    script: &IntentScript,
) -> Result<ResolvedStep> {
    let provider = select_flashloan_provider(step.via.as_deref(), registry)?;

    // Apply the side-swap: `short X collateral=USDC borrow=WETH` is modeled
    // identically to `long USDC collateral=USDC borrow=WETH` — same expansion
    // template, different user-facing semantics.
    let (collateral_alias, borrow_alias) = resolve_sides(step, side)?;
    if collateral_alias == borrow_alias {
        return Err(CompileError::Validation(
            "leverage 'collateral' and 'borrow' assets must differ".to_string(),
        ));
    }

    // Normalize collateral → WETH for native ETH (consistent with existing
    // deposit path — Aave takes WETH, not ETH).
    let collateral_token_alias = if registry.is_native(&collateral_alias) {
        registry.chain.wrapped_native.clone()
    } else {
        collateral_alias.clone()
    };
    let borrow_token_alias = if registry.is_native(&borrow_alias) {
        registry.chain.wrapped_native.clone()
    } else {
        borrow_alias.clone()
    };

    let collateral_token = registry
        .assets
        .get(&collateral_token_alias)
        .ok_or_else(|| unknown_asset(&collateral_token_alias, registry))?;
    let borrow_token = registry
        .assets
        .get(&borrow_token_alias)
        .ok_or_else(|| unknown_asset(&borrow_token_alias, registry))?;

    let collateral_addr = parse_asset_addr(&collateral_token.address, registry)?;
    let borrow_addr = parse_asset_addr(&borrow_token.address, registry)?;
    let collateral_decimals = collateral_token.decimals;
    let borrow_decimals = borrow_token.decimals;

    // Parse and validate leverage as fixed-point × 10_000.
    let leverage_scaled = parse_leverage(&step.leverage)?;
    if leverage_scaled < LEVERAGE_SCALE {
        return Err(CompileError::Validation(format!(
            "leverage must be >= 1.0 (got '{}')",
            step.leverage
        )));
    }

    // Per-asset leverage cap derived from the Aave LTV table.
    let aave = registry
        .protocols
        .get("aave")
        .ok_or_else(|| CompileError::UnknownProtocol {
            protocol: "aave".to_string(),
            network: registry.network.clone(),
            available: registry.protocols.keys().cloned().collect::<Vec<_>>(),
        })?;
    let ltv_table = aave.ltv_bps.as_ref().ok_or_else(|| {
        CompileError::Adapter(
            "Aave protocol config missing 'ltv_bps' table — required for leverage sugar"
                .to_string(),
        )
    })?;
    let ltv_bps = *ltv_table.get(&collateral_token_alias).ok_or_else(|| {
        CompileError::Validation(format!(
            "Aave LTV for collateral '{}' is not configured",
            collateral_token_alias
        ))
    })?;
    if ltv_bps == 0 || ltv_bps >= 10_000 {
        return Err(CompileError::Validation(format!(
            "Aave LTV for '{}' is {} bps, out of [1, 9999]",
            collateral_token_alias, ltv_bps
        )));
    }
    let config_margin = aave.max_leverage_safety_margin_bps.unwrap_or(0);
    let extra_margin = step.safety_margin_bps.unwrap_or(0);
    let total_margin = config_margin.saturating_add(extra_margin);
    // max_leverage_scaled = 10_000 * 10_000 / (10_000 - (ltv_bps - total_margin))
    let effective_ltv: i64 = ltv_bps as i64 - total_margin as i64;
    if effective_ltv <= 0 {
        return Err(CompileError::Validation(format!(
            "safety margin {} bps exceeds LTV {} bps for '{}' — no leverage allowed",
            total_margin, ltv_bps, collateral_token_alias
        )));
    }
    let max_leverage_denom: u64 = (10_000u64).saturating_sub(effective_ltv as u64);
    // Returns u64::MAX when denom == 0 (infinite leverage — impossible since LTV < 100%).
    let max_leverage_scaled: u64 = (LEVERAGE_SCALE * 10_000)
        .checked_div(max_leverage_denom)
        .unwrap_or(u64::MAX);
    if leverage_scaled > max_leverage_scaled {
        return Err(CompileError::Validation(format!(
            "leverage {}x exceeds max {:.4}x for collateral '{}' (Aave LTV {} bps)",
            format_scaled_leverage(leverage_scaled),
            max_leverage_scaled as f64 / LEVERAGE_SCALE as f64,
            collateral_token_alias,
            ltv_bps
        )));
    }

    // Slippage in bps.
    let slippage_bps = parse_slippage(step.slippage.as_deref())?;
    // Swap fee tier — default 0.3% (3000) but overridable for thinly-
    // traded pairs that route better through another tier.
    let swap_fee = match step.fee.as_deref() {
        None => 3000u32,
        Some("100") => 100,
        Some("500") => 500,
        Some("3000") => 3000,
        Some("10000") => 10000,
        Some(other) => {
            return Err(CompileError::InvalidAmount(format!(
                "leverage swap fee tier '{}' must be 100, 500, 3000, or 10000",
                other
            )));
        }
    };

    // User contribution as U256 in collateral-token base units.
    let contribution = parse_amount(&step.amount, collateral_decimals)?;
    if contribution == U256::ZERO {
        return Err(CompileError::Validation(
            "leverage amount must be greater than zero".to_string(),
        ));
    }

    // Flashloaned collateral: (leverage - 1) * contribution.
    // total_collateral = leverage * contribution
    // flashloan_amount = total_collateral - contribution
    let leverage_u = U256::from(leverage_scaled);
    let scale_u = U256::from(LEVERAGE_SCALE);
    let total_collateral = contribution
        .checked_mul(leverage_u)
        .ok_or_else(|| CompileError::InvalidAmount("leverage computation overflow".into()))?
        / scale_u;
    let flashloan_amount = total_collateral.checked_sub(contribution).ok_or_else(|| {
        CompileError::InvalidAmount("leverage < 1 — flashloan would be negative".into())
    })?;

    // When leverage > 1 we need a price to size the borrow. Leverage == 1 is
    // a pass-through (no flashloan needed) — we still build a trivial
    // single-borrow-swap structure for symmetry, but it's degenerate.
    if leverage_scaled > LEVERAGE_SCALE && step.price.is_none() {
        return Err(CompileError::Validation(
            "leverage > 1 requires the 'price' field (borrow tokens per 1 collateral token)"
                .to_string(),
        ));
    }

    // Build the inner pipeline:
    //   1. AaveV3Supply(collateral=collateral, amount=total_collateral, onBehalf=signer)
    //   2. AaveV3Borrow(asset=borrow, amount=borrow_amount, rateMode=2, onBehalf=signer)
    //   3. UniswapV3Swap(borrow → collateral, amount_in=borrow_amount,
    //                    min_out=flashloan_amount)  ← guarantees repayment
    //
    // For leverage == 1, flashloan_amount=0 — we could skip the flashloan
    // entirely, but for code-path uniformity we still emit the wrapper. The
    // subsequent enricher will drop the ERC20TransferFrom of 0 tokens. We
    // prevent that degenerate case by requiring leverage > 1 for the sugar
    // to be useful — and let the user write a plain `deposit` when they
    // want pure unlevered supply.
    if leverage_scaled == LEVERAGE_SCALE {
        return Err(CompileError::Validation(
            "leverage = 1 is a plain deposit — use `{\"deposit\": ...}` instead of `long`/`short`"
                .to_string(),
        ));
    }

    // Aave V3 flashloans charge a premium (`FLASHLOAN_PREMIUM_TOTAL`, currently
    // 5 bps on L1 and Base). Balancer charges nothing today. We grow the
    // amount the inner swap must produce so that the inner pipeline can repay
    // `flashloan + premium` while still keeping the user's leveraged
    // collateral position intact.
    let premium_bps: u16 = match provider {
        FlashloanProvider::Balancer => 0,
        FlashloanProvider::Aave => registry.aave_flashloan_premium_bps(),
    };
    let premium = flashloan_amount.saturating_mul(U256::from(premium_bps as u64))
        / U256::from(10_000u64);
    let target_swap_out = flashloan_amount
        .checked_add(premium)
        .ok_or_else(|| CompileError::InvalidAmount("flashloan+premium overflow".into()))?;

    // Borrow amount sized from price + slippage:
    //   borrow_amount_human = target_swap_out_human * price * (1 + slippage/10_000)
    //
    // We keep amounts in base units throughout. For collateral units C and
    // borrow units B, with price P = B-per-C (human), `price_scaled =
    // P * 10^borrow_decimals`. Then
    //   borrow_base = target_swap_out_base * price_scaled * (10_000 + slippage) /
    //                 (10^collateral_decimals * 10_000)
    let price_scaled = parse_amount(step.price.as_deref().unwrap_or("0"), borrow_decimals)?;
    if price_scaled == U256::ZERO {
        return Err(CompileError::Validation(
            "leverage 'price' must be greater than zero".to_string(),
        ));
    }
    let scale_collateral = U256::from(10u128).pow(U256::from(collateral_decimals as u64));
    let slippage_u = U256::from(10_000u64 + slippage_bps);
    let denom_10k = U256::from(10_000u64);
    let borrow_amount = target_swap_out
        .checked_mul(price_scaled)
        .and_then(|v| v.checked_mul(slippage_u))
        .ok_or_else(|| CompileError::InvalidAmount("borrow-amount overflow".into()))?
        / (scale_collateral * denom_10k);

    // Fetch Aave pool address and Uniswap router.
    let aave_pool_addr = aave.contracts.get("pool").ok_or_else(|| {
        CompileError::Adapter("Protocol 'aave' has no 'pool' contract configured".to_string())
    })?;
    let aave_pool = parse_asset_addr(aave_pool_addr, registry)?;

    let uniswap =
        registry
            .protocols
            .get("uniswap")
            .ok_or_else(|| CompileError::UnknownProtocol {
                protocol: "uniswap".to_string(),
                network: registry.network.clone(),
                available: registry.protocols.keys().cloned().collect::<Vec<_>>(),
            })?;
    let swap_router_addr = uniswap.contracts.get("router").ok_or_else(|| {
        CompileError::Adapter("Protocol 'uniswap' has no 'router' contract configured".to_string())
    })?;
    let swap_router = parse_asset_addr(swap_router_addr, registry)?;

    // Provider-specific outer wrapper address. For Balancer we look up the
    // vault; for Aave we reuse the pool we already fetched for the LTV table.
    let balancer_vault_opt: Option<Address> = match provider {
        FlashloanProvider::Balancer => {
            let balancer =
                registry
                    .protocols
                    .get("balancer")
                    .ok_or_else(|| CompileError::UnknownProtocol {
                        protocol: "balancer".to_string(),
                        network: registry.network.clone(),
                        available: registry.protocols.keys().cloned().collect::<Vec<_>>(),
                    })?;
            let vault_addr = balancer.contracts.get("vault").ok_or_else(|| {
                CompileError::Adapter(
                    "Protocol 'balancer' has no 'vault' contract configured".to_string(),
                )
            })?;
            Some(parse_asset_addr(vault_addr, registry)?)
        }
        FlashloanProvider::Aave => None,
    };

    // Deadline for the inner swap — fall back to the script's effective
    // deadline or the 30-min default.
    let swap_deadline = swap_deadline_from_script(script);

    // Pre-pull the user's equity contribution into the router via a first
    // inner transferFrom. This lets the flashloan validator's balance-sheet
    // check pass: seed (flashloan) + contribution + swap_out ≥ flashloan owed,
    // even when the supply consumes more than the flashloan alone provides.
    // The enricher sees this explicit transferFrom and skips its own auto
    // insert for the same token on the supply.
    let router = registry.router_address().ok_or_else(|| {
        CompileError::Adapter(
            "leverage sugar requires an IntentRouter address in the registry".to_string(),
        )
    })?;

    let inner_steps = vec![
        ResolvedStep::Erc20TransferFrom {
            token: collateral_addr,
            from: signer,
            to: router,
            amount: contribution,
        },
        ResolvedStep::AaveV3Supply {
            pool: aave_pool,
            asset: collateral_addr,
            amount: total_collateral,
            on_behalf_of: signer,
        },
        ResolvedStep::AaveV3Borrow {
            pool: aave_pool,
            asset: borrow_addr,
            amount: borrow_amount,
            rate_mode: 2, // variable
            on_behalf_of: signer,
        },
        ResolvedStep::UniswapV3Swap {
            router: swap_router,
            token_in: borrow_addr,
            token_out: collateral_addr,
            amount_in: borrow_amount,
            fee: swap_fee,
            recipient: signer,
            deadline: U256::from(swap_deadline),
            // Must cover repayment + provider premium. For Balancer (premium=0)
            // this is exactly `flashloan_amount`; for Aave it grows by 5 bps.
            amount_out_minimum: target_swap_out,
            native_input: false,
        },
    ];

    match provider {
        FlashloanProvider::Balancer => Ok(ResolvedStep::BalancerFlashloan {
            vault: balancer_vault_opt.expect("balancer provider implies vault was fetched"),
            tokens: vec![collateral_addr],
            amounts: vec![flashloan_amount],
            inner_steps,
        }),
        FlashloanProvider::Aave => Ok(ResolvedStep::AaveFlashloan {
            pool: aave_pool,
            asset: collateral_addr,
            amount: flashloan_amount,
            premium_bps,
            inner_steps,
        }),
    }
}

pub fn expand_close(
    step: &ClosePositionStep,
    signer: Address,
    registry: &RegistryContext,
    script: &IntentScript,
) -> Result<ResolvedStep> {
    let provider = select_flashloan_provider(step.via.as_deref(), registry)?;

    if step.collateral == step.borrow {
        return Err(CompileError::Validation(
            "close_position 'collateral' and 'borrow' must differ".to_string(),
        ));
    }

    let collateral_alias = if registry.is_native(&step.collateral) {
        registry.chain.wrapped_native.clone()
    } else {
        step.collateral.clone()
    };
    let borrow_alias = if registry.is_native(&step.borrow) {
        registry.chain.wrapped_native.clone()
    } else {
        step.borrow.clone()
    };

    let collateral_cfg = registry
        .assets
        .get(&collateral_alias)
        .ok_or_else(|| unknown_asset(&collateral_alias, registry))?;
    let borrow_cfg = registry
        .assets
        .get(&borrow_alias)
        .ok_or_else(|| unknown_asset(&borrow_alias, registry))?;
    let collateral_addr = parse_asset_addr(&collateral_cfg.address, registry)?;
    let borrow_addr = parse_asset_addr(&borrow_cfg.address, registry)?;

    let current_debt = parse_amount(&step.current_debt, borrow_cfg.decimals)?;
    let current_collateral = parse_amount(&step.current_collateral, collateral_cfg.decimals)?;
    if current_debt == U256::ZERO || current_collateral == U256::ZERO {
        return Err(CompileError::Validation(
            "close_position 'current_debt' and 'current_collateral' must be > 0".to_string(),
        ));
    }

    let slippage_bps = parse_slippage(step.slippage.as_deref())?;

    // Close pipeline:
    //   1. AaveV3Repay(borrow, current_debt, rateMode=2, onBehalf=signer)
    //   2. AaveV3Withdraw(collateral, current_collateral, to=signer)
    //   3. UniswapV3Swap(collateral → borrow, amount_in=?, min_out=current_debt)
    //
    // The flashloan is of `borrow`, sized to match `current_debt`. After
    // repay+withdraw we swap collateral back to borrow with a slippage
    // buffer so we end up with at least `current_debt` of borrow for Balancer.
    let aave = registry
        .protocols
        .get("aave")
        .ok_or_else(|| CompileError::UnknownProtocol {
            protocol: "aave".to_string(),
            network: registry.network.clone(),
            available: registry.protocols.keys().cloned().collect::<Vec<_>>(),
        })?;
    let aave_pool_addr = aave.contracts.get("pool").ok_or_else(|| {
        CompileError::Adapter("Protocol 'aave' has no 'pool' contract configured".to_string())
    })?;
    let aave_pool = parse_asset_addr(aave_pool_addr, registry)?;

    let uniswap =
        registry
            .protocols
            .get("uniswap")
            .ok_or_else(|| CompileError::UnknownProtocol {
                protocol: "uniswap".to_string(),
                network: registry.network.clone(),
                available: registry.protocols.keys().cloned().collect::<Vec<_>>(),
            })?;
    let swap_router_addr = uniswap.contracts.get("router").ok_or_else(|| {
        CompileError::Adapter("Protocol 'uniswap' has no 'router' contract configured".to_string())
    })?;
    let swap_router = parse_asset_addr(swap_router_addr, registry)?;

    // Provider-specific flashloan premium and outer wrapper.
    let premium_bps: u16 = match provider {
        FlashloanProvider::Balancer => 0,
        FlashloanProvider::Aave => registry.aave_flashloan_premium_bps(),
    };
    let premium = current_debt.saturating_mul(U256::from(premium_bps as u64))
        / U256::from(10_000u64);
    let target_swap_out = current_debt
        .checked_add(premium)
        .ok_or_else(|| CompileError::InvalidAmount("close_position premium overflow".into()))?;

    let balancer_vault_opt: Option<Address> = match provider {
        FlashloanProvider::Balancer => {
            let balancer =
                registry
                    .protocols
                    .get("balancer")
                    .ok_or_else(|| CompileError::UnknownProtocol {
                        protocol: "balancer".to_string(),
                        network: registry.network.clone(),
                        available: registry.protocols.keys().cloned().collect::<Vec<_>>(),
                    })?;
            let vault_addr = balancer.contracts.get("vault").ok_or_else(|| {
                CompileError::Adapter(
                    "Protocol 'balancer' has no 'vault' contract configured".to_string(),
                )
            })?;
            Some(parse_asset_addr(vault_addr, registry)?)
        }
        FlashloanProvider::Aave => None,
    };

    let swap_deadline = swap_deadline_from_script(script);
    let _ = slippage_bps; // reserved for a future quote-based swap sizing

    let inner_steps = vec![
        ResolvedStep::AaveV3Repay {
            pool: aave_pool,
            asset: borrow_addr,
            amount: current_debt,
            rate_mode: 2,
            on_behalf_of: signer,
        },
        ResolvedStep::AaveV3Withdraw {
            pool: aave_pool,
            asset: collateral_addr,
            amount: current_collateral,
            to: signer,
        },
        ResolvedStep::UniswapV3Swap {
            router: swap_router,
            token_in: collateral_addr,
            token_out: borrow_addr,
            amount_in: current_collateral,
            fee: 3000,
            recipient: signer,
            deadline: U256::from(swap_deadline),
            // Must cover repayment + provider premium.
            amount_out_minimum: target_swap_out,
            native_input: false,
        },
    ];

    match provider {
        FlashloanProvider::Balancer => Ok(ResolvedStep::BalancerFlashloan {
            vault: balancer_vault_opt.expect("balancer provider implies vault was fetched"),
            tokens: vec![borrow_addr],
            amounts: vec![current_debt],
            inner_steps,
        }),
        FlashloanProvider::Aave => Ok(ResolvedStep::AaveFlashloan {
            pool: aave_pool,
            asset: borrow_addr,
            amount: current_debt,
            premium_bps,
            inner_steps,
        }),
    }
}

fn resolve_sides(step: &LeverageStep, side: Side) -> Result<(String, String)> {
    // Default borrow: volatile collateral → USDC; stable → WETH.
    let default_borrow = match step.collateral.as_str() {
        "USDC" | "USDT" | "DAI" => "WETH",
        _ => "USDC",
    };
    let collateral = step.collateral.clone();
    let borrow = step
        .borrow
        .clone()
        .unwrap_or_else(|| default_borrow.to_string());

    // `short` is `long` with collateral/borrow swapped.
    match side {
        Side::Long => Ok((collateral, borrow)),
        Side::Short => Ok((borrow, collateral)),
    }
}

fn parse_leverage(s: &str) -> Result<u64> {
    // Parse as fixed-point × 10_000. Accepts "1", "3.5", "5.0", etc. Up to 4
    // fractional digits.
    let scaled = parse_amount(s, 4)
        .map_err(|_| CompileError::InvalidAmount(format!("invalid leverage value '{}'", s)))?;
    // parse_amount returns U256; safely narrow.
    let as_u128: u128 = scaled
        .try_into()
        .map_err(|_| CompileError::InvalidAmount(format!("leverage '{}' is absurdly large", s)))?;
    if as_u128 > u64::MAX as u128 {
        return Err(CompileError::InvalidAmount(format!(
            "leverage '{}' is absurdly large",
            s
        )));
    }
    Ok(as_u128 as u64)
}

fn parse_slippage(s: Option<&str>) -> Result<u64> {
    let raw = match s {
        Some(v) => v,
        None => return Ok(DEFAULT_SLIPPAGE_BPS),
    };
    let parsed: u64 = raw.parse().map_err(|_| {
        CompileError::InvalidAmount(format!("invalid slippage_bps '{}' (must be integer)", raw))
    })?;
    if parsed > MAX_SLIPPAGE_BPS {
        return Err(CompileError::Validation(format!(
            "slippage_bps {} exceeds maximum {} (5%)",
            parsed, MAX_SLIPPAGE_BPS
        )));
    }
    Ok(parsed)
}

fn parse_asset_addr(s: &str, _registry: &RegistryContext) -> Result<Address> {
    s.parse::<Address>()
        .map_err(|_| CompileError::InvalidAddress(s.to_string()))
}

fn unknown_asset(alias: &str, registry: &RegistryContext) -> CompileError {
    CompileError::UnknownAsset {
        asset: alias.to_string(),
        network: registry.network.clone(),
        suggestion: None,
    }
}

fn swap_deadline_from_script(script: &IntentScript) -> u64 {
    // Mirror normalize's swap-deadline logic: prefer explicit script.deadline,
    // then current_timestamp + 20 min, finally u64::MAX.
    const DEFAULT_SWAP_DEADLINE_SECS: u64 = 1200;
    if let Some(d) = script.deadline
        && d > 0
    {
        return d;
    }
    match script.current_timestamp {
        Some(ts) => ts + DEFAULT_SWAP_DEADLINE_SECS,
        None => u64::MAX,
    }
}

fn format_scaled_leverage(scaled: u64) -> String {
    let whole = scaled / LEVERAGE_SCALE;
    let frac = scaled % LEVERAGE_SCALE;
    if frac == 0 {
        format!("{}", whole)
    } else {
        format!("{}.{:04}", whole, frac)
    }
}
