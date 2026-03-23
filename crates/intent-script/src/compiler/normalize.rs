//! Stage B: Normalize — convert public AST into canonical IR.
//!
//! Resolves aliases to addresses, parses human-readable amounts to U256,
//! and maps protocol names to concrete deployment addresses.

use alloy_primitives::{Address, U256};

use crate::error::{CompileError, Result};
use crate::ir::{ResolvedIntent, ResolvedStep};
use crate::registry::RegistryContext;
use crate::schema::{IntentScript, Step};

/// Normalize a parsed intent script into the canonical IR.
pub fn normalize(script: &IntentScript, registry: &RegistryContext) -> Result<ResolvedIntent> {
    let signer = parse_address(&script.from)?;

    let mut steps = Vec::new();
    for step in &script.steps {
        let resolved = normalize_step(step, signer, registry)?;
        steps.push(resolved);
    }

    Ok(ResolvedIntent {
        chain_id: registry.chain.chain_id,
        signer,
        steps,
        tokens_to_sweep: Vec::new(),
    })
}

fn normalize_step(
    step: &Step,
    signer: Address,
    registry: &RegistryContext,
) -> Result<ResolvedStep> {
    match step {
        Step::Wrap(w) => {
            // Wrap: asset should be native (ETH) or we wrap to the wrapped native
            let wrapped_token = resolve_asset_address(&registry.chain.wrapped_native, registry)?;
            let decimals = resolve_asset_decimals(&w.asset, registry)?;
            let amount = parse_amount(&w.amount, decimals)?;
            Ok(ResolvedStep::Wrap {
                wrapped_token,
                amount,
            })
        }
        Step::Unwrap(u) => {
            // Unwrap: asset should be WETH or the wrapped native
            let wrapped_token = resolve_asset_address(&u.asset, registry)?;
            let decimals = resolve_asset_decimals(&u.asset, registry)?;
            let amount = parse_amount(&u.amount, decimals)?;
            Ok(ResolvedStep::Unwrap {
                wrapped_token,
                amount,
            })
        }
        Step::Deposit(d) => {
            let asset = resolve_asset_address(&d.asset, registry)?;
            let decimals = resolve_asset_decimals(&d.asset, registry)?;
            let amount = parse_amount(&d.amount, decimals)?;

            let protocol =
                registry
                    .protocols
                    .get(&d.into)
                    .ok_or_else(|| CompileError::UnknownProtocol {
                        protocol: d.into.clone(),
                        network: registry.network.clone(),
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
            let amount = parse_amount(&b.amount, decimals)?;

            let protocol =
                registry
                    .protocols
                    .get(&b.from)
                    .ok_or_else(|| CompileError::UnknownProtocol {
                        protocol: b.from.clone(),
                        network: registry.network.clone(),
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
            let amount = parse_amount(&w.amount, decimals)?;

            let protocol =
                registry
                    .protocols
                    .get(&w.from)
                    .ok_or_else(|| CompileError::UnknownProtocol {
                        protocol: w.from.clone(),
                        network: registry.network.clone(),
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
        Step::Swap(_) => Err(CompileError::UnsupportedStep(
            "swap is not yet implemented in v1".to_string(),
        )),
        Step::Custom(_) => Err(CompileError::UnsupportedStep(
            "custom steps are not yet implemented in v1".to_string(),
        )),
    }
}

/// Resolve an asset alias to its on-chain address.
fn resolve_asset_address(alias: &str, registry: &RegistryContext) -> Result<Address> {
    let config = registry
        .assets
        .get(alias)
        .ok_or_else(|| CompileError::UnknownAsset {
            asset: alias.to_string(),
            network: registry.network.clone(),
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
        })?;
    Ok(config.decimals)
}

/// Parse a hex address string into an Address.
fn parse_address(s: &str) -> Result<Address> {
    s.parse::<Address>()
        .map_err(|_| CompileError::InvalidAddress(s.to_string()))
}

/// Parse a human-readable amount string (e.g. "1.5", "10000", "0.01") into U256
/// using the token's decimal places.
fn parse_amount(amount_str: &str, decimals: u8) -> Result<U256> {
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
