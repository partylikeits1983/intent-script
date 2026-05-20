//! Config loading — mirrors `load_config` in `src/main.rs`, but takes the
//! network explicitly (the advisor knows the network before it has an intent).

use std::collections::BTreeMap;
use std::path::Path;

use eyre::{Result, eyre};

/// The three config blobs the compiler needs, as raw JSON strings.
pub struct ConfigBundle {
    pub chains: String,
    pub assets: String,
    pub protocols: String,
}

/// One entry of `config/assets/<network>.json`.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct AssetInfo {
    /// Token contract address, or the literal `"native"` for the gas token.
    pub address: String,
    pub decimals: u8,
}

/// symbol → asset metadata.
pub type AssetMap = BTreeMap<String, AssetInfo>;

/// Load `chains.json` + `assets/<network>.json` + `protocols/<network>.json`.
pub fn load_config(config_dir: &Path, network: &str) -> Result<ConfigBundle> {
    let read = |p: std::path::PathBuf| {
        std::fs::read_to_string(&p).map_err(|e| eyre!("failed to read {}: {e}", p.display()))
    };
    Ok(ConfigBundle {
        chains: read(config_dir.join("chains.json"))?,
        assets: read(config_dir.join("assets").join(format!("{network}.json")))?,
        protocols: read(config_dir.join("protocols").join(format!("{network}.json")))?,
    })
}

/// Parse `assets/<network>.json` into a symbol → metadata map.
pub fn parse_assets(assets_json: &str) -> Result<AssetMap> {
    serde_json::from_str(assets_json).map_err(|e| eyre!("failed to parse assets config: {e}"))
}

/// Look up the numeric `chain_id` for a network in `chains.json`.
pub fn chain_id(chains_json: &str, network: &str) -> Result<u64> {
    let v: serde_json::Value =
        serde_json::from_str(chains_json).map_err(|e| eyre!("failed to parse chains.json: {e}"))?;
    v.get(network)
        .and_then(|n| n.get("chain_id"))
        .and_then(|c| c.as_u64())
        .ok_or_else(|| eyre!("no chain_id for network '{network}' in chains.json"))
}
