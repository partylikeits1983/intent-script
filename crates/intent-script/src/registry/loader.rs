//! Registry loader — parses JSON config data and builds lookup tables.

use alloc::string::{String, ToString};

use alloy_primitives::Address;
use hashbrown::HashMap;
use serde::Deserialize;

use crate::error::{CompileError, Result};

/// Chain configuration from chains.json
#[derive(Debug, Clone, Deserialize)]
pub struct ChainConfig {
    pub chain_id: u64,
    pub native_asset: String,
    pub wrapped_native: String,
}

/// Asset configuration from assets/{network}.json
#[derive(Debug, Clone, Deserialize)]
pub struct AssetConfig {
    /// "native" for the chain's native asset, or a hex address
    pub address: String,
    pub decimals: u8,
}

/// Per-market configuration for Morpho Blue markets (keyed by a human alias).
///
/// The `id` is `keccak256(abi.encode(MarketParams))` — pre-computed at
/// config-authoring time so the compiler stays deterministic and offline.
/// Adapters reconstruct the `MarketParams` struct from the other fields when
/// building calldata.
#[derive(Debug, Clone, Deserialize)]
pub struct MorphoMarketConfig {
    /// Loan-asset alias, e.g. "USDC".
    pub loan: String,
    /// Collateral-asset alias, e.g. "WETH".
    pub collateral: String,
    /// Oracle contract address (hex).
    pub oracle: String,
    /// Interest rate model address (hex).
    pub irm: String,
    /// Liquidation LTV as a decimal uint256 string (18-decimal fixed point).
    pub lltv: String,
    /// keccak256(abi.encode(MarketParams)) as a 32-byte hex string with 0x prefix.
    pub id: String,
}

/// Protocol configuration from protocols/{network}.json
#[derive(Debug, Clone, Deserialize)]
pub struct ProtocolConfig {
    #[serde(rename = "type")]
    pub protocol_type: String,
    pub version: String,
    pub contracts: HashMap<String, String>,
    /// Router-only: basis-points skim applied at sweep time.
    /// Absent or `None` for non-router protocols.
    #[serde(default)]
    pub fee_bps: Option<u16>,
    /// Lending-protocol-only: keyed market parameter table (Morpho Blue).
    /// Absent or `None` for protocols without market-id addressing.
    #[serde(default)]
    pub markets: Option<HashMap<String, MorphoMarketConfig>>,
    /// Aave-only: per-asset loan-to-value ratio in basis points (8000 = 80%).
    /// Used by the leverage-sugar expander to cap `long`/`short` multipliers.
    /// Values are frozen in config because the compiler has no RPC — stale
    /// values only make the compiler more conservative, never less.
    #[serde(default)]
    pub ltv_bps: Option<HashMap<String, u16>>,
    /// Aave-only: safety margin subtracted from the theoretical max leverage,
    /// in bps (300 = 3%). `0` means honor the raw LTV floor; callers opt into
    /// slack via an explicit `safety_margin_bps` on the leverage step.
    #[serde(default)]
    pub max_leverage_safety_margin_bps: Option<u16>,
}

/// Combined registry context for a specific network.
#[derive(Debug, Clone)]
pub struct RegistryContext {
    pub network: String,
    pub chain: ChainConfig,
    pub assets: HashMap<String, AssetConfig>,
    pub protocols: HashMap<String, ProtocolConfig>,
    /// Full chains map (all chains loaded from `chains.json`), used by bridge
    /// steps to resolve `to_chain` aliases into their numeric `chain_id`.
    pub all_chains: HashMap<String, ChainConfig>,
}

impl RegistryContext {
    /// Load registry context for a given network from pre-loaded JSON strings.
    ///
    /// The caller (CLI binary or frontend) is responsible for reading the files
    /// and passing the raw JSON strings. This keeps the library no-std compatible.
    ///
    /// Arguments:
    /// - `chains_json`: Contents of `chains.json`
    /// - `assets_json`: Contents of `assets/{network}.json`
    /// - `protocols_json`: Contents of `protocols/{network}.json`
    /// - `network`: Network name (e.g., "ethereum")
    pub fn load(
        chains_json: &str,
        assets_json: &str,
        protocols_json: &str,
        network: &str,
    ) -> Result<Self> {
        let chains: HashMap<String, ChainConfig> = serde_json::from_str(chains_json)?;

        let chain = chains
            .get(network)
            .cloned()
            .ok_or_else(|| CompileError::UnknownNetwork(network.to_string()))?;

        let assets: HashMap<String, AssetConfig> = serde_json::from_str(assets_json)?;

        let protocols: HashMap<String, ProtocolConfig> = serde_json::from_str(protocols_json)?;

        Ok(RegistryContext {
            network: network.to_string(),
            chain,
            assets,
            protocols,
            all_chains: chains,
        })
    }

    /// Check if an asset alias refers to the native asset (e.g. "ETH").
    pub fn is_native(&self, alias: &str) -> bool {
        if alias == self.chain.native_asset {
            return true;
        }
        self.assets
            .get(alias)
            .is_some_and(|a| a.address == "native")
    }

    /// Check if an asset alias refers to the wrapped native asset (e.g. "WETH").
    pub fn is_wrapped_native(&self, alias: &str) -> bool {
        alias == self.chain.wrapped_native
    }

    /// Router fee in basis points (applied at sweep/refund time on-chain).
    ///
    /// Returns 0 when no `intent_router` protocol is configured or when its
    /// `fee_bps` field is absent. Clamped to <= 10_000 to keep
    /// `step_produces` reduction math safe.
    pub fn fee_bps(&self) -> u16 {
        self.protocols
            .get("intent_router")
            .and_then(|p| p.fee_bps)
            .unwrap_or(0)
            .min(10_000)
    }

    /// Look up the IntentRouter address for this network, if configured.
    ///
    /// Returns `None` if no router is configured or the address is the zero address.
    pub fn router_address(&self) -> Option<Address> {
        let router_config = self.protocols.get("intent_router")?;
        let addr_str = router_config.contracts.get("router")?;
        let addr: Address = addr_str.parse().ok()?;
        if addr == Address::ZERO {
            None
        } else {
            Some(addr)
        }
    }

    /// Resolve an on-chain address back to its symbol alias.
    ///
    /// Returns the native asset alias for the zero address, the asset alias for
    /// known contract addresses, or a truncated hex string fallback (e.g.
    /// "0xA0b8...eB48") for addresses not in the registry.
    pub fn symbol_for_address(&self, address: &Address) -> alloc::string::String {
        use alloc::format;

        if *address == Address::ZERO {
            return self.chain.native_asset.clone();
        }

        let target = format!("{:?}", address).to_lowercase();
        for (alias, config) in &self.assets {
            if config.address.to_lowercase() == target {
                return alias.clone();
            }
        }

        let full = format!("{:?}", address);
        if full.len() >= 42 {
            format!("{}...{}", &full[..6], &full[full.len() - 4..])
        } else {
            full
        }
    }

    /// Look up decimals for an on-chain address. Defaults to 18 if unknown.
    pub fn decimals_for_address(&self, address: &Address) -> u8 {
        use alloc::format;

        if *address == Address::ZERO {
            // Native asset — fall back through the alias.
            if let Some(native) = self.assets.get(&self.chain.native_asset) {
                return native.decimals;
            }
            return 18;
        }

        let target = format!("{:?}", address).to_lowercase();
        for config in self.assets.values() {
            if config.address.to_lowercase() == target {
                return config.decimals;
            }
        }
        18
    }
}
