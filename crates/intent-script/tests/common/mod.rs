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

/// A stable default for `current_timestamp` used by the test harness when
/// an input intent doesn't provide one. Batched intents require a deadline
/// source at compile time — before that contract existed most tests simply
/// omitted the timestamp. Auto-injecting a default keeps those generic
/// tests focused on the behavior they care about. Deadline-specific tests
/// use [`compile_anvil_raw`] to bypass the injection.
pub const TEST_DEFAULT_CURRENT_TIMESTAMP: u64 = 1_712_344_000;

/// Inject `current_timestamp` into a JSON intent when none is present. Used
/// by the default `compile_anvil*` helpers so that adding a deadline
/// enforcement in the compiler doesn't force every pre-existing test to
/// specify a timestamp it doesn't care about.
fn inject_default_timestamp_if_missing(input: &str) -> String {
    let mut v: serde_json::Value = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(_) => return input.to_string(),
    };
    let Some(obj) = v.as_object_mut() else {
        return input.to_string();
    };
    let has_deadline = obj
        .get("deadline")
        .and_then(|d| d.as_u64())
        .is_some_and(|d| d > 0);
    let has_ts = obj.contains_key("current_timestamp");
    if !has_deadline && !has_ts {
        obj.insert(
            "current_timestamp".into(),
            serde_json::Value::Number(TEST_DEFAULT_CURRENT_TIMESTAMP.into()),
        );
    }
    serde_json::to_string(&v).unwrap_or_else(|_| input.to_string())
}

pub fn compile_anvil(input: &str) -> Result<CompileResult, CompileError> {
    let (chains, assets, protocols) = load_anvil_config();
    let input = inject_default_timestamp_if_missing(input);
    compile(&input, &chains, &assets, &protocols)
}

/// Compile without the default-timestamp injection. Use when the test
/// deliberately exercises deadline behavior (DeadlineMissing, DeadlineInPast,
/// etc.).
pub fn compile_anvil_raw(input: &str) -> Result<CompileResult, CompileError> {
    let (chains, assets, protocols) = load_anvil_config();
    compile(input, &chains, &assets, &protocols)
}

pub fn compile_anvil_with_allowances(
    input: &str,
    allowances_json: Option<&str>,
) -> Result<CompileResult, CompileError> {
    let (chains, assets, protocols) = load_anvil_config();
    let input = inject_default_timestamp_if_missing(input);
    compile_with_allowances(&input, &chains, &assets, &protocols, allowances_json)
}

pub fn compile_anvil_without_router(input: &str) -> Result<CompileResult, CompileError> {
    let (chains, assets, protocols) = load_anvil_config();
    let mut protocols_json: serde_json::Value = serde_json::from_str(&protocols).unwrap();
    protocols_json
        .as_object_mut()
        .unwrap()
        .remove("intent_router");
    let protocols = serde_json::to_string(&protocols_json).unwrap();
    let input = inject_default_timestamp_if_missing(input);
    compile(&input, &chains, &assets, &protocols)
}

pub fn max_allowance_decimal() -> &'static str {
    "115792089237316195423570985008687907853269984665640564039457584007913129639935"
}
