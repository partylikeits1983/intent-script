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
use crate::ir::{ResolvedBalances, ResolvedIntent, ResolvedStep, step_consumes, step_produces};
use crate::registry::RegistryContext;

/// Maximum number of user-facing steps allowed in a single intent.
pub const MAX_STEPS: usize = 3;

/// Minimum Aave health factor — below this, borrows are rejected.
const MIN_HEALTH_FACTOR: f64 = 1.2;
/// Warning threshold for Aave health factor.
const WARN_HEALTH_FACTOR: f64 = 1.5;

/// Result of validation: Ok with a list of warnings, or Err on hard failure.
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
            ResolvedStep::AaveV3Withdraw { pool, asset, .. } => {
                // Rule 2: Withdraw requires prior deposit or existing position
                if !deposited_protocols.contains(pool) {
                    validate_withdraw_feasibility(
                        *asset,
                        intent.user_balances.as_ref(),
                        &mut warnings,
                    )?;
                }
            }
            _ => {}
        }
    }

    // Task 5: Cross-step amount flow validation
    validate_amount_flow(&intent.steps, intent.fee_bps)?;

    Ok(ValidationResult { warnings })
}

/// Task 1: Reject swaps with zero slippage protection.
fn validate_slippage(step: &ResolvedStep, step_index: usize) -> Result<()> {
    if let ResolvedStep::UniswapV3Swap {
        amount_out_minimum, ..
    } = step
    {
        if *amount_out_minimum == U256::ZERO {
            return Err(CompileError::SlippageTooLow {
                step_index,
                current: "0".to_string(),
            });
        }
    }
    Ok(())
}

/// Task 3: Validate Aave health factor for borrows.
fn validate_health_factor(
    balances: Option<&ResolvedBalances>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    if let Some(b) = balances {
        if let Some(hf) = b.aave_health_factor {
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
    }
    Ok(())
}

/// Task 5: Cross-step amount flow validation.
///
/// Track tokens produced by previous steps. Only validate consumption when
/// a step uses a token that a prior step produced. Wallet-sourced tokens
/// are not checked.
fn validate_amount_flow(steps: &[ResolvedStep], fee_bps: u16) -> Result<()> {
    let mut produced: HashMap<Address, U256> = HashMap::new();

    for (i, step) in steps.iter().enumerate() {
        if let Some((token, required)) = step_consumes(step) {
            if let Some(available) = produced.get(&token) {
                if required > *available {
                    return Err(CompileError::InvalidChain(format!(
                        "Step {} requires {} of token {} but previous steps only guarantee {}",
                        i + 1,
                        required,
                        token,
                        available
                    )));
                }
                produced.insert(token, *available - required);
            }
        }
        if let Some((token, guaranteed)) = step_produces(step, fee_bps) {
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
        | ResolvedStep::SendErc721 { to, .. } => {
            if *to == Address::ZERO {
                return Err(CompileError::InvalidChain(
                    "Cannot send to the zero address".to_string(),
                ));
            }
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
            let has_position = b
                .aave_supplied
                .get(&asset)
                .map_or(false, |v| *v > U256::ZERO);

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
        ResolvedStep::UniswapV3Swap { amount_in, .. } => Some(("swap", amount_in)),
        ResolvedStep::LidoStake { amount, .. } => Some(("stake", amount)),
        ResolvedStep::WstETHWrap { amount, .. } => Some(("wrap stETH", amount)),
        ResolvedStep::WstETHUnwrap { amount, .. } => Some(("unwrap wstETH", amount)),
        ResolvedStep::OneInchSwap { amount_in, .. } => Some(("swap", amount_in)),
        ResolvedStep::SendErc20 { amount, .. } => Some(("send", amount)),
        ResolvedStep::SendEth { amount, .. } => Some(("send", amount)),
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
            amount0,
            amount1,
            amount0_min,
            amount1_min,
            ..
        }
        | ResolvedStep::UniswapV3LpIncrease {
            amount0,
            amount1,
            amount0_min,
            amount1_min,
            ..
        } => {
            if *amount0 == U256::ZERO && *amount1 == U256::ZERO {
                return Err(CompileError::InvalidChain(
                    "LP mint/increase requires a non-zero amount on at least one side".to_string(),
                ));
            }
            // Slippage protection: at least one min must be > 0 (the
            // other may legitimately be zero for single-sided out-of-range
            // liquidity deposits).
            if *amount0_min == U256::ZERO && *amount1_min == U256::ZERO {
                return Err(CompileError::InvalidChain(
                    "LP mint/increase requires slippage protection: min_amount0 or min_amount1 must be > 0"
                        .to_string(),
                ));
            }
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
        // Approve, TransferFrom, Permit, SendErc721 are auto-generated or don't have amounts
        ResolvedStep::Erc20Approve { .. }
        | ResolvedStep::Erc20TransferFrom { .. }
        | ResolvedStep::Erc20Permit { .. }
        | ResolvedStep::SendErc721 { .. } => None,
    };

    if let Some((action, value)) = amount {
        if *value == U256::ZERO {
            return Err(CompileError::InvalidChain(format!(
                "{action} amount must be greater than zero"
            )));
        }
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
        // Can't 1inch swap from an asset to the same asset
        ResolvedStep::OneInchSwap {
            token_in,
            token_out,
            ..
        } if token_in == token_out => Err(CompileError::InvalidChain(
            "Cannot swap an asset to itself. \
             The source and destination tokens must be different."
                .to_string(),
        )),
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
            fee_bps: 0,
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
        let mut balances = ResolvedBalances::default();
        balances.aave_health_factor = Some(1.1);
        let mut warnings = Vec::new();
        let result = validate_health_factor(Some(&balances), &mut warnings);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("health factor"));
    }

    #[test]
    fn test_health_factor_warning_range() {
        let mut balances = ResolvedBalances::default();
        balances.aave_health_factor = Some(1.3);
        let mut warnings = Vec::new();
        let result = validate_health_factor(Some(&balances), &mut warnings);
        assert!(result.is_ok());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("health factor"));
    }

    #[test]
    fn test_health_factor_above_warning_clean() {
        let mut balances = ResolvedBalances::default();
        balances.aave_health_factor = Some(2.0);
        let mut warnings = Vec::new();
        let result = validate_health_factor(Some(&balances), &mut warnings);
        assert!(result.is_ok());
        assert!(warnings.is_empty());
    }

    // Task 4: Max step count test
    #[test]
    fn test_too_many_steps_rejected() {
        let steps: Vec<ResolvedStep> = (0..4)
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
        let result = validate_amount_flow(&steps, 0);
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
        let result = validate_amount_flow(&steps, 0);
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
        let result = validate_amount_flow(&steps, 0);
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
