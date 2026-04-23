#![allow(dead_code)]

use std::path::{Path, PathBuf};

use intent_script::error::CompileError;
use intent_script::{CompileResult, compile, compile_with_allowances};

pub fn config_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("config")
}

pub fn load_anvil_config() -> (String, String, String) {
    let dir = config_dir();
    let chains = std::fs::read_to_string(dir.join("chains.json")).unwrap();
    let assets = std::fs::read_to_string(dir.join("assets/anvil.json")).unwrap();
    let protocols = std::fs::read_to_string(dir.join("protocols/anvil.json")).unwrap();
    (chains, assets, protocols)
}

pub fn compile_anvil(input: &str) -> Result<CompileResult, CompileError> {
    let (chains, assets, protocols) = load_anvil_config();
    compile(input, &chains, &assets, &protocols)
}

pub fn compile_anvil_with_allowances(
    input: &str,
    allowances_json: Option<&str>,
) -> Result<CompileResult, CompileError> {
    let (chains, assets, protocols) = load_anvil_config();
    compile_with_allowances(input, &chains, &assets, &protocols, allowances_json)
}

pub fn compile_anvil_without_router(input: &str) -> Result<CompileResult, CompileError> {
    let (chains, assets, protocols) = load_anvil_config();
    let mut protocols_json: serde_json::Value = serde_json::from_str(&protocols).unwrap();
    protocols_json
        .as_object_mut()
        .unwrap()
        .remove("intent_router");
    let protocols = serde_json::to_string(&protocols_json).unwrap();
    compile(input, &chains, &assets, &protocols)
}

pub fn max_allowance_decimal() -> &'static str {
    "115792089237316195423570985008687907853269984665640564039457584007913129639935"
}
