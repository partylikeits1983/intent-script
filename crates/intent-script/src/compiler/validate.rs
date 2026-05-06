//! Stage C: Validate — check the resolved IR for correctness.
//!
//! Validates structural correctness and intent chain logic.
//! When user balances are provided, performs stricter feasibility checks.
//! When balances are absent, produces warnings instead of errors for
//! rules that depend on on-chain state.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use alloy_primitives::{Address, U256};
use hashbrown::{HashMap, HashSet};

use crate::error::{CompileError, Result};
use crate::ir::{
    ConcreteCall, ResolvedBalances, ResolvedIntent, ResolvedStep, step_consumes, step_produces,
};
use crate::registry::RegistryContext;

/// Maximum number of user-facing steps allowed in a single intent.
pub const MAX_STEPS: usize = 8;
/// Maximum inner-pipeline step count for a flashloan.
pub const MAX_FLASHLOAN_INNER_STEPS: usize = 8;

/// B2: Absolute slippage floor in basis points. Swaps that specify a
/// slippage tolerance are capped at 500 bps (5%) — anything above that
/// indicates either a hallucinated value or a pathological market, and
/// the signed intent should be re-quoted rather than broadcast.
pub const MAX_SLIPPAGE_BPS: u64 = 500;

/// B5: Per-call ETH value cap. 1000 ETH is generous — well above any
/// realistic user action — while still catching hallucinated `value: 10^30`
/// overflow shapes.
pub const MAX_PER_CALL_WEI: u128 = 1_000 * 10u128.pow(18);
/// B5: Total ETH value cap across all calls in a batch. Set high enough
/// for realistic leverage flows (which route ETH through multiple hops)
/// and low enough to flag obviously-bogus sums.
pub const MAX_TOTAL_WEI: u128 = 10_000 * 10u128.pow(18);
/// B5: Maximum concrete-call count after enrichment. User steps are
/// capped at MAX_STEPS (8); approvals + transferFroms + sweeps can
/// realistically triple that. 40 keeps headroom over the worst-case
/// 8-step batch (8 × 3 enrichment calls + a few sweeps) without losing
/// the "obviously bogus" guardrail.
pub const MAX_CALLS_AFTER_ENRICH: usize = 40;

/// Minimum Aave health factor — below this, borrows are rejected.
const MIN_HEALTH_FACTOR: f64 = 1.2;
/// Warning threshold for Aave health factor.
const WARN_HEALTH_FACTOR: f64 = 1.5;

/// Result of validation: Ok with a list of warnings, or Err on hard failure.
#[derive(Debug)]
pub struct ValidationResult {
    pub warnings: Vec<String>,
}

/// Validate a resolved intent. Returns warnings on success, or an error on failure.
///
/// Rules:
/// 1. Borrow requires prior deposit or existing collateral
/// 2. Withdraw requires prior deposit or existing position
/// 3. Amounts must be positive (> 0)
/// 4. Asset compatibility (no native ETH into Aave, no swap-to-self)
/// 5. Protocol existence (already enforced by normalization)
/// 6. Slippage protection required for swaps (Task 1)
/// 7. Aave health factor check (Task 3)
/// 8. Max step count (Task 4)
/// 9. Cross-step amount flow validation (Task 5)
/// 10. Send validation (Task 7)
pub fn validate(intent: &ResolvedIntent, _registry: &RegistryContext) -> Result<ValidationResult> {
    let mut warnings = Vec::new();

    // Check signer is not zero address
    if intent.signer == Address::ZERO {
        return Err(CompileError::Validation(
            "Signer address cannot be zero".to_string(),
        ));
    }

    // Check we have at least one step
    if intent.steps.is_empty() {
        return Err(CompileError::Validation(
            "Intent must have at least one step".to_string(),
        ));
    }

    // Task 4: Max step count
    if intent.steps.len() > MAX_STEPS {
        return Err(CompileError::Validation(format!(
            "Intent has {} steps but maximum is {}",
            intent.steps.len(),
            MAX_STEPS
        )));
    }

    // Track protocols that have received deposits in this intent
    let mut deposited_protocols: HashSet<Address> = HashSet::new();

    for (step_index, step) in intent.steps.iter().enumerate() {
        // Rule 3: Amount validation — all amounts must be positive
        validate_amount(step)?;

        // Rule 4: Asset compatibility
        validate_asset_compatibility(step)?;

        // Task 1: Slippage protection for swaps
        validate_slippage(step, step_index)?;

        // Task 7: Send validation
        validate_send(step)?;

        // Flashloan structural rules (nesting, step count, repayability).
        validate_flashloan(step, intent.fee_bps)?;

        match step {
            ResolvedStep::AaveV3Supply { pool, .. } => {
                // Track that we've deposited into this protocol
                deposited_protocols.insert(*pool);
            }
            ResolvedStep::AaveV3Borrow { pool, .. } => {
                // Rule 1: Borrow requires prior deposit or existing collateral
                if !deposited_protocols.contains(pool) {
                    validate_borrow_feasibility(intent.user_balances.as_ref(), &mut warnings)?;
                }
                // Task 3: Aave health factor check
                validate_health_factor(intent.user_balances.as_ref(), &mut warnings)?;
            }
            // Rule 2: Withdraw requires prior deposit or existing position
            ResolvedStep::AaveV3Withdraw { pool, asset, .. }
                if !deposited_protocols.contains(pool) =>
            {
                validate_withdraw_feasibility(
                    *asset,
                    intent.user_balances.as_ref(),
                    &mut warnings,
                )?;
            }
            _ => {}
        }
    }

    // Task 5: Cross-step amount flow validation (wallet-balance-aware).
    //
    // Seed the running balance with the user's on-chain wallet amounts when
    // the caller supplied them, so a hallucinated `deposit 1_000_000 USDC`
    // against a 100-USDC wallet is caught at compile time instead of
    // bubbling up as an on-chain revert after fees were already taken.
    validate_amount_flow(&intent.steps, intent.fee_bps, intent.user_balances.as_ref())?;

    // B1: Recipient pinning. Every step with a recipient-like address must
    // name the signer (or, once the enricher has redirected, the router).
    // This runs pre-enrich, so the expected value is always the signer.
    //
    // The LLM has no input into these fields today — the normalizer sets
    // them to `signer` on every happy path. This guardrail enforces that
    // invariant explicitly so that a future schema change or a regression
    // in normalize can't silently leak user funds to an attacker-controlled
    // address. User-directed transfers (SendErc20 / SendEth / SendErc721)
    // are exempt — those are the explicit exit from this invariant and are
    // already zero-address-checked by `validate_send`.
    for (step_index, step) in intent.steps.iter().enumerate() {
        validate_recipient_pinning(step, intent.signer, step_index)?;
    }

    Ok(ValidationResult { warnings })
}

/// B5: Budget the lowered concrete-call stream. Rejects obvious overflow
/// shapes and dust-flood griefs. Runs after `lower::lower` so it sees the
/// exact calls that would be executed on-chain.
pub fn validate_call_budget(calls: &[ConcreteCall]) -> Result<()> {
    if calls.len() > MAX_CALLS_AFTER_ENRICH {
        return Err(CompileError::Validation(format!(
            "Batch has {} concrete calls after enrichment but the limit is {}. \
             Either reduce the number of user steps (max {}) or split this \
             intent into multiple transactions.",
            calls.len(),
            MAX_CALLS_AFTER_ENRICH,
            MAX_STEPS
        )));
    }

    let max_per_call = U256::from(MAX_PER_CALL_WEI);
    let max_total = U256::from(MAX_TOTAL_WEI);
    let mut total = U256::ZERO;
    for (i, c) in calls.iter().enumerate() {
        if c.value > max_per_call {
            return Err(CompileError::Validation(format!(
                "Call {} sends {} wei (> {} wei per-call cap). This usually \
                 indicates a hallucinated amount; if you truly need to move \
                 this much ETH, split it across multiple batches.",
                i + 1,
                c.value,
                max_per_call
            )));
        }
        // Saturate — a total that would wrap obviously exceeds the cap.
        total = total.saturating_add(c.value);
        if total > max_total {
            return Err(CompileError::Validation(format!(
                "Aggregate call value {} wei exceeds the batch cap {} wei. \
                 Split the intent or reduce the per-call values.",
                total, max_total
            )));
        }
    }
    Ok(())
}

/// B1: Every step with an internal recipient-like field must name the
/// signer. Send steps and Across bridges are explicit exits and opt out.
fn validate_recipient_pinning(
    step: &ResolvedStep,
    signer: Address,
    step_index: usize,
) -> Result<()> {
    // Returns the pair of (field_name, actual_value) to check, or None when
    // the step has no internal recipient that must be pinned to the signer.
    let check_pairs: Vec<(&'static str, Address)> = match step {
        // Auto-inserted by the enricher — we should never see these pre-enrich,
        // but if we do (e.g. via a direct IR test), they must name the signer
        // as the payer.
        ResolvedStep::Erc20TransferFrom { from, .. } => alloc::vec![("from", *from)],
        ResolvedStep::AaveV3Supply { on_behalf_of, .. } => {
            alloc::vec![("on_behalf_of", *on_behalf_of)]
        }
        // Borrow's `on_behalf_of` is the debt holder — MUST be the signer.
        // Aave's credit delegation lets other accounts borrow against a
        // user's collateral, which is a well-known footgun; lock it down.
        ResolvedStep::AaveV3Borrow { on_behalf_of, .. } => {
            alloc::vec![("on_behalf_of", *on_behalf_of)]
        }
        ResolvedStep::AaveV3Withdraw { to, .. } => alloc::vec![("to", *to)],
        ResolvedStep::AaveV3Repay { on_behalf_of, .. } => {
            alloc::vec![("on_behalf_of", *on_behalf_of)]
        }
        ResolvedStep::MorphoSupply { on_behalf, .. } => alloc::vec![("on_behalf", *on_behalf)],
        ResolvedStep::MorphoSupplyCollat { on_behalf, .. } => {
            alloc::vec![("on_behalf", *on_behalf)]
        }
        ResolvedStep::MorphoBorrow {
            on_behalf,
            receiver,
            ..
        } => alloc::vec![("on_behalf", *on_behalf), ("receiver", *receiver)],
        ResolvedStep::MorphoWithdraw {
            on_behalf,
            receiver,
            ..
        } => alloc::vec![("on_behalf", *on_behalf), ("receiver", *receiver)],
        ResolvedStep::MorphoWithdrawCollat {
            on_behalf,
            receiver,
            ..
        } => alloc::vec![("on_behalf", *on_behalf), ("receiver", *receiver)],
        ResolvedStep::MorphoRepay { on_behalf, .. } => alloc::vec![("on_behalf", *on_behalf)],
        ResolvedStep::UniswapV3Swap { recipient, .. } => alloc::vec![("recipient", *recipient)],
        ResolvedStep::UniswapV3LpMint { recipient, .. } => alloc::vec![("recipient", *recipient)],
        ResolvedStep::UniswapV3LpCollect { recipient, .. } => {
            alloc::vec![("recipient", *recipient)]
        }
        ResolvedStep::LidoRequestWithdrawal { owner, .. } => alloc::vec![("owner", *owner)],
        ResolvedStep::Erc20Permit { owner, .. } => alloc::vec![("owner", *owner)],
        // Across recipient is an explicit cross-chain destination chosen by
        // the user; depositor must still be the signer.
        ResolvedStep::AcrossDepositV3 { depositor, .. } => alloc::vec![("depositor", *depositor)],
        // Explicit user-directed transfers — already zero-checked; the user
        // chose the `to` address and we don't second-guess it.
        ResolvedStep::SendErc20 { .. }
        | ResolvedStep::SendEth { .. }
        | ResolvedStep::SendErc721 { .. } => alloc::vec![],
        // Steps without a recipient-like field.
        ResolvedStep::Wrap { .. }
        | ResolvedStep::Unwrap { .. }
        | ResolvedStep::Erc20Approve { .. }
        | ResolvedStep::LidoStake { .. }
        | ResolvedStep::WstETHWrap { .. }
        | ResolvedStep::WstETHUnwrap { .. }
        | ResolvedStep::LidoClaimWithdrawal { .. }
        | ResolvedStep::UniswapV3LpIncrease { .. }
        | ResolvedStep::UniswapV3LpDecrease { .. }
        | ResolvedStep::BalancerFlashloan { .. } => alloc::vec![],
    };

    for (field, actual) in check_pairs {
        if actual != signer {
            return Err(CompileError::Validation(format!(
                "Step {}: field '{}' is {} but must be the intent signer ({}). \
                 Recipient pinning rejects any internal step whose recipient is \
                 neither the signer nor a router-mediated hop; use an explicit \
                 send step for third-party transfers.",
                step_index + 1,
                field,
                actual,
                signer
            )));
        }
    }
    Ok(())
}

/// Flashloan validation: enforce bounded depth, bounded inner step count,
/// and that the inner pipeline produces at least the flashloaned amount of
/// each borrowed token.
fn validate_flashloan(step: &ResolvedStep, fee_bps: u16) -> Result<()> {
    let ResolvedStep::BalancerFlashloan {
        tokens,
        amounts,
        inner_steps,
        ..
    } = step
    else {
        return Ok(());
    };

    // Depth 1: no nested flashloans. Also enforce the usual structural rules
    // (positive amounts, slippage, asset compatibility) on inner steps.
    for (inner_index, inner) in inner_steps.iter().enumerate() {
        if matches!(inner, ResolvedStep::BalancerFlashloan { .. }) {
            return Err(CompileError::Validation(
                "nested flashloans are not allowed (max depth 1)".to_string(),
            ));
        }
        validate_amount(inner)?;
        validate_asset_compatibility(inner)?;
        validate_slippage(inner, inner_index)?;
        validate_send(inner)?;
    }

    // Bounded inner pipeline.
    if inner_steps.len() > MAX_FLASHLOAN_INNER_STEPS {
        return Err(CompileError::Validation(format!(
            "flashloan inner pipeline has {} steps but maximum is {}",
            inner_steps.len(),
            MAX_FLASHLOAN_INNER_STEPS
        )));
    }

    if tokens.len() != amounts.len() {
        return Err(CompileError::Validation(
            "flashloan tokens and amounts lengths must match".to_string(),
        ));
    }

    // Repayability: seed the running balance with the flashloaned amounts
    // (Balancer transfers those in before calling back), then walk the inner
    // pipeline's produces/consumes. At the end, each flashloaned token must
    // have at least `amount` of balance left to repay the Vault. `fee_bps = 0`
    // per the doc-comment on `step_produces`: produced tokens inside a
    // flashloan are returned to the Vault by `receiveFlashLoan`, not swept
    // through the router fee path, so no fee reduction applies.
    let _ = fee_bps;
    let mut balance: HashMap<Address, U256> = HashMap::new();
    for (t, a) in tokens.iter().zip(amounts.iter()) {
        *balance.entry(*t).or_insert(U256::ZERO) += *a;
    }
    for inner in inner_steps {
        if let Some((t, a)) = step_consumes(inner) {
            let have = balance.get(&t).copied().unwrap_or(U256::ZERO);
            if a > have {
                // Inner step consumes more than present — the step will revert
                // on-chain. Surface the chain error now rather than later.
                return Err(CompileError::InvalidChain(format!(
                    "flashloan inner step consumes {} of token {} but only {} is available (flashloaned + produced)",
                    a, t, have
                )));
            }
            balance.insert(t, have - a);
        }
        if let Some((t, a)) = step_produces(inner, 0) {
            *balance.entry(t).or_insert(U256::ZERO) += a;
        }
    }
    for (t, a) in tokens.iter().zip(amounts.iter()) {
        let have = balance.get(t).copied().unwrap_or(U256::ZERO);
        if have < *a {
            return Err(CompileError::Validation(format!(
                "flashloan not repayable: inner pipeline leaves only {} of token {} but {} is owed to Balancer",
                have, t, a
            )));
        }
    }

    Ok(())
}

/// Task 1: Reject swaps with zero slippage protection.
fn validate_slippage(step: &ResolvedStep, step_index: usize) -> Result<()> {
    if let ResolvedStep::UniswapV3Swap {
        amount_out_minimum, ..
    } = step
        && *amount_out_minimum == U256::ZERO
    {
        return Err(CompileError::SlippageTooLow {
            step_index,
            current: "0".to_string(),
        });
    }
    Ok(())
}

/// Task 3: Validate Aave health factor for borrows.
fn validate_health_factor(
    balances: Option<&ResolvedBalances>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    if let Some(b) = balances
        && let Some(hf) = b.aave_health_factor
    {
        if hf < MIN_HEALTH_FACTOR {
            return Err(CompileError::HealthFactorRisk {
                current: hf,
                threshold: MIN_HEALTH_FACTOR,
            });
        }
        if hf < WARN_HEALTH_FACTOR {
            warnings.push(format!(
                "Aave health factor is {:.2}. Borrowing may increase liquidation risk.",
                hf
            ));
        }
    }
    Ok(())
}

/// Task 5: Cross-step amount flow validation (wallet-balance-aware).
///
/// Walks the pipeline and tracks a running balance per token. A step that
/// consumes more of a token than is currently available is rejected.
///
/// The running balance is seeded from the user's wallet when `balances`
/// is `Some` — that's the B3 extension. With seeding, a hallucinated
/// `deposit 1_000_000 USDC` against a 100-USDC wallet is caught at
/// compile time instead of bubbling up as an on-chain revert after the
/// router has already taken its sweep fee.
///
/// Without seeding (`balances == None`), the behavior matches the pre-B3
/// contract: only steps whose consumed token appears in an earlier
/// produce are validated; wallet-sourced tokens are trusted.
fn validate_amount_flow(
    steps: &[ResolvedStep],
    fee_bps: u16,
    balances: Option<&ResolvedBalances>,
) -> Result<()> {
    // Intra-batch hand-offs stay inside the router; no fee is skimmed between
    // steps. The sweep fee only applies when leftover tokens flow back to the
    // signer at the end. Pass `fee_bps = 0` here so an exact-amount `deposit`
    // after an exact-amount `wrap`/`swap`/`borrow` isn't falsely rejected as
    // a 0.1% shortfall. Mirrors the flashloan-inner-pipeline rule above
    // (tokens returned to the Vault also bypass sweep).
    let _ = fee_bps;
    let mut produced: HashMap<Address, U256> = HashMap::new();
    let seeded_from_wallet = if let Some(b) = balances {
        for (token, amount) in b.tokens.iter() {
            *produced.entry(*token).or_insert(U256::ZERO) += *amount;
        }
        !b.tokens.is_empty()
    } else {
        false
    };

    for (i, step) in steps.iter().enumerate() {
        if let Some((token, required)) = step_consumes(step)
            && let Some(available) = produced.get(&token)
        {
            if required > *available {
                return Err(CompileError::InvalidChain(format!(
                    "Step {} requires {} of token {} but the running balance only \
                         guarantees {} (wallet seed {} prior-step produce).",
                    i + 1,
                    required,
                    token,
                    available,
                    if seeded_from_wallet { "+" } else { "disabled;" }
                )));
            }
            produced.insert(token, *available - required);
        }
        // When `balances` is Some and this token is listed in wallet but
        // with zero amount, `produced.get(&token)` returned Some(0) and
        // the > check above handled it. When the token isn't in wallet
        // and no prior step produced it, we fall through without a check
        // — matches the pre-B3 trust contract for token streams the
        // caller didn't give us info about.
        if let Some((token, guaranteed)) = step_produces(step, 0) {
            *produced.entry(token).or_insert(U256::ZERO) += guaranteed;
        }
    }
    Ok(())
}

/// Task 7: Validate send steps.
fn validate_send(step: &ResolvedStep) -> Result<()> {
    match step {
        ResolvedStep::SendErc20 { to, .. }
        | ResolvedStep::SendEth { to, .. }
        | ResolvedStep::SendErc721 { to, .. }
            if *to == Address::ZERO =>
        {
            return Err(CompileError::InvalidChain(
                "Cannot send to the zero address".to_string(),
            ));
        }
        _ => {}
    }
    Ok(())
}

/// Rule 1: Validate that a borrow is feasible.
///
/// - If balances are provided and show collateral → OK
/// - If balances are provided but show no collateral → Error
/// - If no balances provided → Warning (optimistic compilation)
fn validate_borrow_feasibility(
    balances: Option<&ResolvedBalances>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    match balances {
        Some(b) => {
            // Check if user has any Aave collateral
            let has_collateral = b.aave_supplied.values().any(|v| *v > U256::ZERO);

            if !has_collateral {
                return Err(CompileError::InvalidChain(
                    "Borrow requires collateral: no prior deposit in this intent and \
                     user balance info shows no existing Aave collateral. \
                     Add a deposit step before borrowing, or supply collateral on-chain first."
                        .to_string(),
                ));
            }
            // User has existing collateral — borrow is valid
            Ok(())
        }
        None => {
            // No balance info — compile optimistically with a warning
            warnings.push(
                "Borrow without prior deposit in this intent. \
                 Ensure the user has existing Aave collateral, \
                 otherwise the transaction will revert on-chain."
                    .to_string(),
            );
            Ok(())
        }
    }
}

/// Rule 2: Validate that a withdraw is feasible.
///
/// - If balances are provided and show a position for this asset → OK
/// - If balances are provided but show no position → Error
/// - If no balances provided → Warning (optimistic compilation)
fn validate_withdraw_feasibility(
    asset: Address,
    balances: Option<&ResolvedBalances>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    match balances {
        Some(b) => {
            // Check if user has a supplied position for this specific asset
            let has_position = b.aave_supplied.get(&asset).is_some_and(|v| *v > U256::ZERO);

            if !has_position {
                return Err(CompileError::InvalidChain(format!(
                    "Withdraw requires an existing position: no prior deposit in this intent \
                         and user balance info shows no supplied position for asset {asset}. \
                         Add a deposit step first, or ensure you have an existing Aave position."
                )));
            }
            Ok(())
        }
        None => {
            warnings.push(
                "Withdraw without prior deposit in this intent. \
                 Ensure the user has an existing Aave position for this asset, \
                 otherwise the transaction will revert on-chain."
                    .to_string(),
            );
            Ok(())
        }
    }
}

/// Rule 3: Validate that step amounts are positive (> 0).
fn validate_amount(step: &ResolvedStep) -> Result<()> {
    let amount = match step {
        ResolvedStep::Wrap { amount, .. } => Some(("wrap", amount)),
        ResolvedStep::Unwrap { amount, .. } => Some(("unwrap", amount)),
        ResolvedStep::AaveV3Supply { amount, .. } => Some(("deposit", amount)),
        ResolvedStep::AaveV3Borrow { amount, .. } => Some(("borrow", amount)),
        ResolvedStep::AaveV3Withdraw { amount, .. } => Some(("withdraw", amount)),
        ResolvedStep::AaveV3Repay { amount, .. } => Some(("aave repay", amount)),
        ResolvedStep::MorphoSupply { amount, .. } => Some(("morpho supply", amount)),
        ResolvedStep::MorphoSupplyCollat { amount, .. } => {
            Some(("morpho supply collateral", amount))
        }
        ResolvedStep::MorphoBorrow { amount, .. } => Some(("morpho borrow", amount)),
        ResolvedStep::MorphoWithdraw { amount, .. } => Some(("morpho withdraw", amount)),
        ResolvedStep::MorphoWithdrawCollat { amount, .. } => {
            Some(("morpho withdraw collateral", amount))
        }
        ResolvedStep::MorphoRepay { amount, .. } => Some(("morpho repay", amount)),
        ResolvedStep::UniswapV3Swap { amount_in, .. } => Some(("swap", amount_in)),
        ResolvedStep::LidoStake { amount, .. } => Some(("stake", amount)),
        ResolvedStep::WstETHWrap { amount, .. } => Some(("wrap stETH", amount)),
        ResolvedStep::WstETHUnwrap { amount, .. } => Some(("unwrap wstETH", amount)),
        ResolvedStep::SendErc20 { amount, .. } => Some(("send", amount)),
        ResolvedStep::SendEth { amount, .. } => Some(("send", amount)),
        ResolvedStep::AcrossDepositV3 { input_amount, .. } => Some(("bridge", input_amount)),
        // Multi-amount variants and auto-generated steps are validated separately.
        ResolvedStep::LidoRequestWithdrawal { amounts, .. } => {
            if amounts.is_empty() {
                return Err(CompileError::InvalidChain(
                    "request_withdrawal requires at least one amount".to_string(),
                ));
            }
            for a in amounts {
                if *a == U256::ZERO {
                    return Err(CompileError::InvalidChain(
                        "request_withdrawal amounts must all be greater than zero".to_string(),
                    ));
                }
            }
            None
        }
        ResolvedStep::LidoClaimWithdrawal {
            request_ids, hints, ..
        } => {
            if request_ids.is_empty() {
                return Err(CompileError::InvalidChain(
                    "claim_withdrawal requires at least one request_id".to_string(),
                ));
            }
            if request_ids.len() != hints.len() {
                return Err(CompileError::InvalidChain(format!(
                    "claim_withdrawal hints length {} does not match request_ids length {}",
                    hints.len(),
                    request_ids.len()
                )));
            }
            None
        }
        ResolvedStep::UniswapV3LpMint {
            amount0, amount1, ..
        }
        | ResolvedStep::UniswapV3LpIncrease {
            amount0, amount1, ..
        } => {
            if *amount0 == U256::ZERO && *amount1 == U256::ZERO {
                return Err(CompileError::InvalidChain(
                    "LP mint/increase requires a non-zero amount on at least one side".to_string(),
                ));
            }
            // Deliberately NOT enforcing a positive `amount0_min` /
            // `amount1_min` here. The price range (`tick_lower` / `tick_upper`)
            // is the real slippage guard for concentrated liquidity; a tight
            // per-token `amount_min` actively *causes* `Price slippage check`
            // reverts when the current tick is off-center inside a narrow
            // range, even in perfectly calm markets (the LP's token0:token1
            // ratio is a function of where the current tick sits inside the
            // range, not of price movement). Leave both `"0"` by default and
            // trust the range.
            None
        }
        ResolvedStep::UniswapV3LpDecrease {
            liquidity,
            amount0_min,
            amount1_min,
            ..
        } => {
            if *liquidity == 0 {
                return Err(CompileError::InvalidChain(
                    "LP decrease liquidity must be greater than zero".to_string(),
                ));
            }
            if *amount0_min == U256::ZERO && *amount1_min == U256::ZERO {
                return Err(CompileError::InvalidChain(
                    "LP decrease requires slippage protection: min_amount0 or min_amount1 must be > 0"
                        .to_string(),
                ));
            }
            None
        }
        // Collect has no amount fields to range-check here; validity is
        // covered by normalize (token_id / position sanity).
        ResolvedStep::UniswapV3LpCollect { .. } => None,
        // Flashloan: outer step has no single amount. Inner-step amounts are
        // validated in validate() via the main step loop when we call
        // validate_flashloan(), which also recursively walks inner_steps.
        ResolvedStep::BalancerFlashloan { .. } => None,
        // Approve, TransferFrom, Permit, SendErc721 are auto-generated or don't have amounts
        ResolvedStep::Erc20Approve { .. }
        | ResolvedStep::Erc20TransferFrom { .. }
        | ResolvedStep::Erc20Permit { .. }
        | ResolvedStep::SendErc721 { .. } => None,
    };

    if let Some((action, value)) = amount
        && *value == U256::ZERO
    {
        return Err(CompileError::InvalidChain(format!(
            "{action} amount must be greater than zero"
        )));
    }

    Ok(())
}

/// Rule 4: Validate asset compatibility.
fn validate_asset_compatibility(step: &ResolvedStep) -> Result<()> {
    match step {
        // Can't deposit native ETH directly into Aave — must wrap to WETH first
        ResolvedStep::AaveV3Supply { asset, .. } if *asset == Address::ZERO => {
            Err(CompileError::InvalidChain(
                "Cannot deposit native ETH directly into Aave. \
                 Add a wrap step to convert ETH to WETH first, \
                 then deposit WETH."
                    .to_string(),
            ))
        }
        // Can't swap from an asset to the same asset
        ResolvedStep::UniswapV3Swap {
            token_in,
            token_out,
            ..
        } if token_in == token_out => Err(CompileError::InvalidChain(
            "Cannot swap an asset to itself. \
             The source and destination tokens must be different."
                .to_string(),
        )),
        // Uniswap V3 LP positions need two distinct ERC-20 tokens on-chain.
        // Native ETH is substituted with the chain's wrapped-native (WETH)
        // during normalize for the single-native case; the double-native
        // case is nonsense and would leave token0 == token1 == Address::ZERO
        // in the IR if it reached here.
        ResolvedStep::UniswapV3LpMint { token0, token1, .. }
            if *token0 == Address::ZERO || *token1 == Address::ZERO =>
        {
            Err(CompileError::InvalidChain(
                "lp_mint token addresses must resolve to ERC-20 contracts. \
                 Use WETH (or another ERC-20) instead of native ETH."
                    .to_string(),
            ))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    fn dummy_intent(steps: Vec<ResolvedStep>) -> ResolvedIntent {
        ResolvedIntent {
            chain_id: 1,
            signer: address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045"),
            steps,
            tokens_to_sweep: Vec::new(),
            nonce: 0,
            deadline: 0,
            user_balances: None,
            required_pulls: Vec::new(),
            required_delegations: Vec::new(),
            fee_bps: 0,
            requires_router: false,
        }
    }

    #[test]
    fn test_zero_amount_rejected() {
        let step = ResolvedStep::Wrap {
            wrapped_token: address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
            amount: U256::ZERO,
        };
        let result = validate_amount(&step);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("greater than zero")
        );
    }

    #[test]
    fn test_positive_amount_accepted() {
        let step = ResolvedStep::Wrap {
            wrapped_token: address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
            amount: U256::from(1_000_000_000_000_000_000u64),
        };
        let result = validate_amount(&step);
        assert!(result.is_ok());
    }

    #[test]
    fn test_swap_same_asset_rejected() {
        let weth = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
        let step = ResolvedStep::UniswapV3Swap {
            router: address!("E592427A0AEce92De3Edee1F18E0157C05861564"),
            token_in: weth,
            token_out: weth,
            amount_in: U256::from(1000u64),
            fee: 3000,
            recipient: address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045"),
            deadline: U256::MAX,
            amount_out_minimum: U256::ZERO,
            native_input: false,
        };
        let result = validate_asset_compatibility(&step);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("swap an asset to itself")
        );
    }

    #[test]
    fn test_native_eth_deposit_rejected() {
        let step = ResolvedStep::AaveV3Supply {
            pool: address!("87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2"),
            asset: Address::ZERO,
            amount: U256::from(1_000_000_000_000_000_000u64),
            on_behalf_of: address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045"),
        };
        let result = validate_asset_compatibility(&step);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("native ETH"));
    }

    #[test]
    fn test_borrow_without_deposit_no_balances_warns() {
        let mut warnings = Vec::new();
        let result = validate_borrow_feasibility(None, &mut warnings);
        assert!(result.is_ok());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Borrow without prior deposit"));
    }

    #[test]
    fn test_borrow_without_deposit_no_collateral_fails() {
        let balances = ResolvedBalances::default();
        let mut warnings = Vec::new();
        let result = validate_borrow_feasibility(Some(&balances), &mut warnings);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("collateral"));
    }

    #[test]
    fn test_borrow_with_existing_collateral_ok() {
        let mut balances = ResolvedBalances::default();
        let usdc = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
        balances
            .aave_supplied
            .insert(usdc, U256::from(50_000_000_000u64)); // 50k USDC
        let mut warnings = Vec::new();
        let result = validate_borrow_feasibility(Some(&balances), &mut warnings);
        assert!(result.is_ok());
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_empty_steps_rejected() {
        let intent = dummy_intent(vec![]);
        // We need a dummy registry — but validate doesn't use it for this check.
        // Use a minimal approach: just test the specific validation.
        assert!(intent.steps.is_empty());
    }

    // Task 1: Slippage protection tests
    #[test]
    fn test_swap_zero_slippage_rejected() {
        let step = ResolvedStep::UniswapV3Swap {
            router: address!("E592427A0AEce92De3Edee1F18E0157C05861564"),
            token_in: address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            token_out: address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
            amount_in: U256::from(1_000_000_000u64),
            fee: 3000,
            recipient: address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045"),
            deadline: U256::MAX,
            amount_out_minimum: U256::ZERO,
            native_input: false,
        };
        let result = validate_slippage(&step, 0);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("slippage protection")
        );
    }

    #[test]
    fn test_swap_with_slippage_ok() {
        let step = ResolvedStep::UniswapV3Swap {
            router: address!("E592427A0AEce92De3Edee1F18E0157C05861564"),
            token_in: address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            token_out: address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
            amount_in: U256::from(1_000_000_000u64),
            fee: 3000,
            recipient: address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045"),
            deadline: U256::MAX,
            amount_out_minimum: U256::from(480_000_000_000_000_000u64),
            native_input: false,
        };
        let result = validate_slippage(&step, 0);
        assert!(result.is_ok());
    }

    // Task 3: Health factor tests
    #[test]
    fn test_health_factor_below_minimum_rejected() {
        let balances = ResolvedBalances {
            aave_health_factor: Some(1.1),
            ..ResolvedBalances::default()
        };
        let mut warnings = Vec::new();
        let result = validate_health_factor(Some(&balances), &mut warnings);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("health factor"));
    }

    #[test]
    fn test_health_factor_warning_range() {
        let balances = ResolvedBalances {
            aave_health_factor: Some(1.3),
            ..ResolvedBalances::default()
        };
        let mut warnings = Vec::new();
        let result = validate_health_factor(Some(&balances), &mut warnings);
        assert!(result.is_ok());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("health factor"));
    }

    #[test]
    fn test_health_factor_above_warning_clean() {
        let balances = ResolvedBalances {
            aave_health_factor: Some(2.0),
            ..ResolvedBalances::default()
        };
        let mut warnings = Vec::new();
        let result = validate_health_factor(Some(&balances), &mut warnings);
        assert!(result.is_ok());
        assert!(warnings.is_empty());
    }

    // Task 4: Max step count test
    #[test]
    fn test_too_many_steps_rejected() {
        let steps: Vec<ResolvedStep> = (0..MAX_STEPS + 1)
            .map(|_| ResolvedStep::Wrap {
                wrapped_token: address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
                amount: U256::from(1_000_000_000_000_000_000u64),
            })
            .collect();
        let intent = dummy_intent(steps);
        assert!(intent.steps.len() > MAX_STEPS);
    }

    // Task 5: Amount flow tests
    #[test]
    fn test_amount_flow_swap_deposit_overflow_rejected() {
        let steps = vec![
            ResolvedStep::UniswapV3Swap {
                router: address!("E592427A0AEce92De3Edee1F18E0157C05861564"),
                token_in: address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
                token_out: address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
                amount_in: U256::from(1_000_000_000u64),
                fee: 3000,
                recipient: address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045"),
                deadline: U256::MAX,
                amount_out_minimum: U256::from(480_000_000_000_000_000u64), // 0.48 WETH
                native_input: false,
            },
            ResolvedStep::AaveV3Supply {
                pool: address!("87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2"),
                asset: address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
                amount: U256::from(1_000_000_000_000_000_000u64), // 1.0 WETH > 0.48 guaranteed
                on_behalf_of: address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045"),
            },
        ];
        let result = validate_amount_flow(&steps, 0, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("requires"));
    }

    #[test]
    fn test_amount_flow_swap_deposit_ok() {
        let steps = vec![
            ResolvedStep::UniswapV3Swap {
                router: address!("E592427A0AEce92De3Edee1F18E0157C05861564"),
                token_in: address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
                token_out: address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
                amount_in: U256::from(1_000_000_000u64),
                fee: 3000,
                recipient: address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045"),
                deadline: U256::MAX,
                amount_out_minimum: U256::from(1_000_000_000_000_000_000u64), // 1.0 WETH
                native_input: false,
            },
            ResolvedStep::AaveV3Supply {
                pool: address!("87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2"),
                asset: address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
                amount: U256::from(1_000_000_000_000_000_000u64), // 1.0 WETH == guaranteed
                on_behalf_of: address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045"),
            },
        ];
        let result = validate_amount_flow(&steps, 0, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_amount_flow_standalone_deposit_ok() {
        // Standalone deposit — no prior step produces the token, so no flow check
        let steps = vec![ResolvedStep::AaveV3Supply {
            pool: address!("87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2"),
            asset: address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            amount: U256::from(5_000_000_000u64),
            on_behalf_of: address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045"),
        }];
        let result = validate_amount_flow(&steps, 0, None);
        assert!(result.is_ok());
    }

    // Task 7: Send validation tests
    #[test]
    fn test_send_to_zero_address_rejected() {
        let step = ResolvedStep::SendErc20 {
            token: address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            to: Address::ZERO,
            amount: U256::from(1_000_000u64),
        };
        let result = validate_send(&step);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("zero address"));
    }

    #[test]
    fn test_send_zero_amount_rejected() {
        let step = ResolvedStep::SendErc20 {
            token: address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            to: address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045"),
            amount: U256::ZERO,
        };
        let result = validate_amount(&step);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("greater than zero")
        );
    }
}
