extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};

use intent_script::error::CompileError;
use wasm_bindgen::prelude::*;

/// Sentinel prefix for structured-error JSON on the WASM boundary.
/// Consumers split on this; anything before the prefix is prose (there
/// shouldn't be any today, but the prefix leaves room for future metadata).
const STRUCTURED_ERROR_PREFIX: &str = "INTENT_ERROR_V1::";

/// Compile an intent-script JSON string into unsigned EVM transactions.
///
/// Arguments are all JSON strings:
/// - `json_input`: the intent JSON (network, from, steps, etc.)
/// - `chains_json`: chain config (chain_id, native_asset, wrapped_native)
/// - `assets_json`: asset registry for the target network
/// - `protocols_json`: protocol registry for the target network
///
/// Returns a JSON string containing the compiled output (CompileOutputJson).
/// On error, returns a `JsError` whose message is
/// `INTENT_ERROR_V1::{structured-json}`. JS callers should split on the
/// prefix and JSON-parse the tail for machine-readable fields; falling
/// back to the whole string keeps the message human-readable.
#[wasm_bindgen]
pub fn compile(
    json_input: &str,
    chains_json: &str,
    assets_json: &str,
    protocols_json: &str,
) -> Result<String, JsError> {
    let result = intent_script::compile(json_input, chains_json, assets_json, protocols_json)
        .map_err(wrap_compile_error)?;

    emit_output(result)
}

/// Same as [`compile`] but with an extra `allowances_json` string describing
/// the user's current on-chain ERC-20 allowances for the router. When empty
/// or whitespace, behavior is identical to [`compile`]; otherwise the
/// compiler emits `prerequisiteApprovals` for any under-allowanced token.
///
/// Shape: `{ "tokens": { "<symbol>": "<base-units>", ... } }`.
#[wasm_bindgen]
pub fn compile_with_allowances(
    json_input: &str,
    chains_json: &str,
    assets_json: &str,
    protocols_json: &str,
    allowances_json: &str,
) -> Result<String, JsError> {
    let trimmed = allowances_json.trim();
    let allowances = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    };
    let result = intent_script::compile_with_allowances(
        json_input,
        chains_json,
        assets_json,
        protocols_json,
        allowances,
    )
    .map_err(wrap_compile_error)?;

    emit_output(result)
}

fn emit_output(result: intent_script::CompileResult) -> Result<String, JsError> {
    for warning in &result.warnings {
        web_log(&format!("⚠ intent-script warning: {warning}"));
    }

    let json_output = result.to_json();
    serde_json::to_string(&json_output)
        .map_err(|e| JsError::new(&format!("Serialization error: {e}")))
}

/// Build a `JsError` whose message is `INTENT_ERROR_V1::{json}` where
/// `{json}` is the serialized [`intent_script::error::StructuredError`].
/// If structured serialization itself fails (should never happen — the
/// struct is composed of plain strings and maps), we fall back to the
/// Display string so the caller still sees *something* readable.
fn wrap_compile_error(e: CompileError) -> JsError {
    let display = e.to_string();
    let structured = e.to_structured();
    match serde_json::to_string(&structured) {
        Ok(json) => JsError::new(&format!("{STRUCTURED_ERROR_PREFIX}{json}")),
        Err(_) => JsError::new(&display),
    }
}

/// Log to the browser console via web_sys-free approach.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn warn(s: &str);
}

fn web_log(msg: &str) {
    warn(msg);
}
