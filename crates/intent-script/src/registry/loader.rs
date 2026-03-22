//! Registry loader — reads JSON config files and builds lookup tables.

use std::collections::HashMap;
use std::path::Path;

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
    /// Load registry context for a given network from config files.
    ///
    /// Expects the following file structure under `config_dir`:
    /// - `chains.json`
    /// - `assets/{network}.json`
    /// - `protocols/{network}.json`
    pub fn load(config_dir: &Path, network: &str) -> Result<Self> {
        // Load chains.json
        let chains_path = config_dir.join("chains.json");
        let chains_data = std::fs::read_to_string(&chains_path).map_err(|e| {
            CompileError::Config(format!("Failed to read {}: {}", chains_path.display(), e))
        })?;
        let chains: HashMap<String, ChainConfig> = serde_json::from_str(&chains_data)?;

        let chain = chains
            .get(network)
            .cloned()
            .ok_or_else(|| CompileError::UnknownNetwork(network.to_string()))?;

        // Load assets/{network}.json
        let assets_path = config_dir.join("assets").join(format!("{network}.json"));
        let assets_data = std::fs::read_to_string(&assets_path).map_err(|e| {
            CompileError::Config(format!("Failed to read {}: {}", assets_path.display(), e))
        })?;
        let assets: HashMap<String, AssetConfig> = serde_json::from_str(&assets_data)?;

        // Load protocols/{network}.json
        let protocols_path = config_dir.join("protocols").join(format!("{network}.json"));
        let protocols_data = std::fs::read_to_string(&protocols_path).map_err(|e| {
            CompileError::Config(format!(
                "Failed to read {}: {}",
                protocols_path.display(),
                e
            ))
        })?;
        let protocols: HashMap<String, ProtocolConfig> = serde_json::from_str(&protocols_data)?;

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
}
