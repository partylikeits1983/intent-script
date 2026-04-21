extern crate alloc;

use alloc::format;
use alloc::string::String;

use wasm_bindgen::prelude::*;

/// Compile an intent-script JSON string into unsigned EVM transactions.
///
/// Arguments are all JSON strings:
/// - `json_input`: the intent JSON (network, from, steps, etc.)
/// - `chains_json`: chain config (chain_id, native_asset, wrapped_native)
/// - `assets_json`: asset registry for the target network
/// - `protocols_json`: protocol registry for the target network
///
/// Returns a JSON string containing the compiled output (CompileOutputJson).
/// On error, returns a JS error with the error message.
#[wasm_bindgen]
pub fn compile(
    json_input: &str,
    chains_json: &str,
    assets_json: &str,
    protocols_json: &str,
) -> Result<String, JsError> {
    let result = intent_script::compile(json_input, chains_json, assets_json, protocols_json)
        .map_err(|e| JsError::new(&format!("{e}")))?;

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
    .map_err(|e| JsError::new(&format!("{e}")))?;

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

/// Log to the browser console via web_sys-free approach.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn warn(s: &str);
}

fn web_log(msg: &str) {
    warn(msg);
}
