//! Runtime context — the per-request facts the system prompt expects
//! ("do not fabricate"): wallet, network, balances, prices, timestamp,
//! positions. Mirrors the `ctx` argument of `buildSystemPrompt()` in
//! `intentOS-ui/lib/system-prompt.ts`.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use eyre::{Result, eyre};

/// The `--context` JSON file. Every field is optional so a bare
/// `{ "wallet": "0x..." }` is valid.
#[derive(serde::Deserialize, Default, Debug)]
pub struct ContextFile {
    pub wallet: Option<String>,
    pub network: Option<String>,
    /// symbol → human-readable amount, e.g. `{ "ETH": "10", "USDC": "5000" }`.
    #[serde(default)]
    pub balances: BTreeMap<String, String>,
    /// symbol → spot USD price, e.g. `{ "ETH": "3500", "USDC": "1.00" }`.
    #[serde(default)]
    pub prices: BTreeMap<String, String>,
    /// Pre-rendered `## Your Positions` markdown block, verbatim.
    pub positions: Option<String>,
}

impl ContextFile {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| eyre!("failed to read context file {}: {e}", path.display()))?;
        serde_json::from_str(&raw).map_err(|e| eyre!("failed to parse context file: {e}"))
    }
}

/// Context resolved into the exact strings the prompt template wants.
pub struct RuntimeContext {
    pub wallet: Option<String>,
    pub network: String,
    /// `"10 ETH, 5000 USDC"` or `None`.
    pub balances_summary: Option<String>,
    /// `"ETH $3500, USDC $1.00"` or `None`.
    pub prices_summary: Option<String>,
    /// Unix seconds — injected so the model never fabricates `current_timestamp`.
    pub timestamp: u64,
    pub positions: Option<String>,
}

impl RuntimeContext {
    /// Build the resolved context from a context file and a network override.
    pub fn resolve(ctx: &ContextFile, network: &str) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            wallet: ctx.wallet.clone(),
            network: network.to_string(),
            balances_summary: summarize_balances(&ctx.balances),
            prices_summary: summarize_prices(&ctx.prices),
            timestamp,
            positions: ctx
                .positions
                .clone()
                .filter(|s| !s.trim().is_empty()),
        }
    }
}

/// `{ "ETH": "10", "USDC": "5000" }` → `"10 ETH, 5000 USDC"`.
fn summarize_balances(balances: &BTreeMap<String, String>) -> Option<String> {
    if balances.is_empty() {
        return None;
    }
    Some(
        balances
            .iter()
            .map(|(sym, amt)| format!("{amt} {sym}"))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

/// `{ "ETH": "3500", "USDC": "1.00" }` → `"ETH $3500, USDC $1.00"`.
fn summarize_prices(prices: &BTreeMap<String, String>) -> Option<String> {
    if prices.is_empty() {
        return None;
    }
    Some(
        prices
            .iter()
            .map(|(sym, px)| format!("{sym} ${px}"))
            .collect::<Vec<_>>()
            .join(", "),
    )
}
