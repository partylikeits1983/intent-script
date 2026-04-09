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

/// Protocol configuration from protocols/{network}.json
#[derive(Debug, Clone, Deserialize)]
pub struct ProtocolConfig {
    #[serde(rename = "type")]
    pub protocol_type: String,
    pub version: String,
    pub contracts: HashMap<String, String>,
}

/// Combined registry context for a specific network.
#[derive(Debug, Clone)]
pub struct RegistryContext {
    pub network: String,
    pub chain: ChainConfig,
    pub assets: HashMap<String, AssetConfig>,
    pub protocols: HashMap<String, ProtocolConfig>,
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
}
