//! Shared on-chain helpers (Alloy): reading balances and formatting amounts.
//! Used both by `--fetch-balances` (Phase 2) and `--simulate` (Phase 3).

use std::collections::BTreeMap;

use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol;
use alloy_primitives::{Address, U256};
use eyre::{Result, eyre};

use crate::config::AssetMap;

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function balanceOf(address owner) external view returns (uint256);
    }
}

/// Read the native + every known ERC-20 balance for `who`, keyed by symbol.
/// Tokens whose `balanceOf` call fails (e.g. not deployed on this fork) are
/// silently skipped — a missing balance is not a fatal error.
pub async fn read_balances<P: Provider>(
    provider: &P,
    who: Address,
    assets: &AssetMap,
) -> Result<BTreeMap<String, U256>> {
    let mut out = BTreeMap::new();

    for (symbol, info) in assets {
        if info.address.eq_ignore_ascii_case("native") {
            let bal = provider.get_balance(who).await?;
            out.insert(symbol.clone(), bal);
            continue;
        }
        let Ok(addr) = info.address.parse::<Address>() else {
            continue;
        };
        let token = IERC20::new(addr, provider);
        if let Ok(bal) = token.balanceOf(who).call().await {
            out.insert(symbol.clone(), bal);
        }
    }

    Ok(out)
}

/// Connect to `rpc` and return the wallet's live balances as human-readable
/// strings (the shape the `--context` file's `balances` map uses).
pub async fn fetch_live_balances(
    rpc: &str,
    wallet: Address,
    assets: &AssetMap,
) -> Result<BTreeMap<String, String>> {
    let url = rpc
        .parse()
        .map_err(|e| eyre!("invalid --rpc URL '{rpc}': {e}"))?;
    let provider = ProviderBuilder::new().connect_http(url);

    let raw = read_balances(&provider, wallet, assets).await?;

    let mut out = BTreeMap::new();
    for (symbol, amount) in raw {
        if amount.is_zero() {
            continue;
        }
        let decimals = assets.get(&symbol).map(|a| a.decimals).unwrap_or(18);
        out.insert(symbol, fmt_units(amount, decimals));
    }
    Ok(out)
}

/// Format a base-unit `U256` as a human-readable decimal string, trimming
/// trailing fractional zeros (`1500000000000000000`, 18 → `"1.5"`).
pub fn fmt_units(raw: U256, decimals: u8) -> String {
    let d = decimals as usize;
    let digits = raw.to_string();
    if d == 0 {
        return digits;
    }
    let (int_part, frac_part) = if digits.len() <= d {
        ("0".to_string(), format!("{digits:0>d$}"))
    } else {
        let split = digits.len() - d;
        (digits[..split].to_string(), digits[split..].to_string())
    };
    let frac = frac_part.trim_end_matches('0');
    if frac.is_empty() {
        int_part
    } else {
        format!("{int_part}.{frac}")
    }
}
