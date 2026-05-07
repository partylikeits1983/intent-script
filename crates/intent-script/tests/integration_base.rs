//! Integration tests for the Base chain config (chain id 8453).
//!
//! These tests are deliberately separate from `integration.rs` (which is
//! pinned to the L1 anvil config) so that the existing 128 L1 tests stay
//! frozen — Base support is additive. When a behavior should hold on both
//! chains, prefer mirroring a slimmed-down case here over parameterizing the
//! L1 file.

use std::path::{Path, PathBuf};

use intent_script::output::CompileOutputJson;
use intent_script::{CompileOutput, CompileResult, compile};

fn config_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("config")
}

fn load_base_config() -> (String, String, String) {
    let dir = config_dir();
    let chains = std::fs::read_to_string(dir.join("chains.json")).unwrap();
    let assets = std::fs::read_to_string(dir.join("assets/base.json")).unwrap();
    let protocols = std::fs::read_to_string(dir.join("protocols/base.json")).unwrap();
    (chains, assets, protocols)
}

fn load_l1_config() -> (String, String, String) {
    let dir = config_dir();
    let chains = std::fs::read_to_string(dir.join("chains.json")).unwrap();
    let assets = std::fs::read_to_string(dir.join("assets/anvil.json")).unwrap();
    let protocols = std::fs::read_to_string(dir.join("protocols/anvil.json")).unwrap();
    (chains, assets, protocols)
}

const TEST_DEFAULT_CURRENT_TIMESTAMP: u64 = 1_712_344_000;
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

fn compile_base(input: &str) -> Result<CompileResult, intent_script::error::CompileError> {
    let (c, a, p) = load_base_config();
    let input = inject_default_timestamp_if_missing(input);
    compile(&input, &c, &a, &p)
}

fn compile_l1(input: &str) -> Result<CompileResult, intent_script::error::CompileError> {
    let (c, a, p) = load_l1_config();
    let input = inject_default_timestamp_if_missing(input);
    compile(&input, &c, &a, &p)
}

// ──────────────────────────────────────────────────────────────────────────
// Plain primitives
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn test_base_swap_weth_to_usdc() {
    let input = r#"{
        "network": "base",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "WETH", "to": "USDC", "amount": "0.5", "price": "3000", "slippage": "0.5" } }
        ]
    }"#;
    let result = compile_base(input).expect("base swap should compile");
    let json = serde_json::to_string(&CompileOutputJson::from(&result)).unwrap();
    // Uniswap V3 SwapRouter02 on Base.
    assert!(
        json.to_lowercase()
            .contains("0x2626664c2603336e57b271c5c0b26f421741e481"),
        "expected Base SwapRouter02 in output: {json}"
    );
    // Single-tx output carries chain_id directly.
    if let CompileOutput::SingleTx(tx) = &result.output {
        assert_eq!(tx.chain_id, 8453);
    }
}

/// Pull the encoded `fee` field out of a SwapRouter02 `exactInputSingle`
/// calldata. Layout: 4-byte selector + 7 × 32-byte fields, with `fee`
/// (uint24, padded) at field index 2 → bytes `[4 + 64 .. 4 + 96]`. The
/// last 3 bytes of that 32-byte slot hold the actual value.
fn extract_swap_fee_from_calldata(data: &[u8]) -> u32 {
    let chunk = &data[4 + 64..4 + 96];
    u32::from_be_bytes([chunk[28], chunk[29], chunk[30], chunk[31]])
}

/// Locate the `exactInputSingle` call inside a compiled output and return
/// its fee tier. Handles both `SingleTx` (standalone swap, native input)
/// and `Eip712Intent` (router-batched, the default for ERC-20 swaps).
/// Searches by the SwapRouter02 selector `0x04e45aaf`.
fn extract_swap_fee(result: &CompileResult) -> u32 {
    const SELECTOR: [u8; 4] = [0x04, 0xe4, 0x5a, 0xaf];
    match &result.output {
        CompileOutput::SingleTx(tx) => {
            assert!(tx.data.starts_with(&SELECTOR), "expected exactInputSingle selector");
            extract_swap_fee_from_calldata(&tx.data)
        }
        CompileOutput::Eip712Intent(intent) => {
            for call in &intent.intent_batch.calls {
                if call.call_data.starts_with(&SELECTOR) {
                    return extract_swap_fee_from_calldata(&call.call_data);
                }
            }
            panic!("no exactInputSingle call found in Eip712Intent batch")
        }
        other => panic!("unexpected compile output: {other:?}"),
    }
}

#[test]
fn test_base_swap_usdc_to_usdt_defaults_to_fee_100() {
    // Stable-stable swaps must auto-default to fee tier 100 (the canonical
    // 0.01% pool) — fee 3000 has no liquidity for USDC/USDT on Base and
    // would silently revert with empty data when `exactInputSingle`
    // calls into a non-existent pool.
    let input = r#"{
        "network": "base",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "to": "USDT", "amount": "100", "price": "1.0", "slippage": "0.5" } }
        ]
    }"#;
    let result = compile_base(input).expect("usdc→usdt swap should compile");
    let fee = extract_swap_fee(&result);
    assert_eq!(
        fee, 100,
        "stable-stable swap should default to fee tier 100; got {fee}"
    );
}

#[test]
fn test_base_swap_usdc_to_weth_defaults_to_fee_3000() {
    // Volatile pair — keep the historical 3000 default; the stable-stable
    // optimization must not over-reach.
    let input = r#"{
        "network": "base",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "to": "WETH", "amount": "100", "price": "0.0003", "slippage": "0.5" } }
        ]
    }"#;
    let result = compile_base(input).expect("usdc→weth swap should compile");
    let fee = extract_swap_fee(&result);
    assert_eq!(
        fee, 3000,
        "volatile pair should keep the 3000 default; got {fee}"
    );
}

#[test]
fn test_base_swap_usdc_to_usdt_explicit_fee_3000_respected() {
    // User-supplied `fee: "3000"` MUST win over the stable-stable default
    // — even though the call is doomed (3000-tier USDC/USDT pool doesn't
    // exist), the compiler shouldn't second-guess explicit input.
    let input = r#"{
        "network": "base",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "to": "USDT", "amount": "100", "fee": "3000", "price": "1.0", "slippage": "0.5" } }
        ]
    }"#;
    let result = compile_base(input).expect("explicit-fee swap should compile");
    let fee = extract_swap_fee(&result);
    assert_eq!(
        fee, 3000,
        "explicit fee should override the stable-stable default; got {fee}"
    );
}

#[test]
fn test_base_aave_deposit_usdc() {
    let input = r#"{
        "network": "base",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "1000", "into": "aave" } }
        ]
    }"#;
    let result = compile_base(input).expect("base aave deposit should compile");
    let json = serde_json::to_string(&CompileOutputJson::from(&result)).unwrap();
    assert!(
        json.to_lowercase()
            .contains("0xa238dd80c259a72e81d7e4664a9801593f98d1c5"),
        "expected Base Aave V3 Pool in output: {json}"
    );
}

#[test]
fn test_base_lido_unsupported() {
    // Lido is L1-only; Base has no `lido` protocol entry, so a `stake` step
    // must surface a clear `UnknownProtocol` error.
    let input = r#"{
        "network": "base",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "stake": { "asset": "ETH", "amount": "1.0", "into": "lido" } }
        ]
    }"#;
    let err = compile_base(input).unwrap_err().to_string();
    assert!(
        err.to_lowercase().contains("lido") || err.to_lowercase().contains("unknown protocol"),
        "expected Lido-not-configured error, got: {err}"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// Leverage on Base — defaults to Aave (no Balancer on Base)
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn test_base_long_defaults_to_aave_flashloan() {
    let input = r#"{
        "network": "base",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "long": {
                "collateral": "WETH",
                "borrow":     "USDC",
                "amount":     "1.0",
                "leverage":   "3",
                "slippage":   "50",
                "price":      "3000"
            } }
        ]
    }"#;
    let result = compile_base(input).expect("3x long on Base should compile via Aave flashloan");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            // The outer call should target the Aave V3 Pool with the
            // `flashLoanSimple` selector (0x42b0b77c).
            let aave_pool = "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5".to_lowercase();
            let any_aave_outer = intent.intent_batch.calls.iter().any(|c| {
                format!("{}", c.target).to_lowercase() == aave_pool
                    && c.call_data.starts_with(&[0x42, 0xb0, 0xb7, 0x7c])
            });
            assert!(
                any_aave_outer,
                "expected an outer Aave V3 flashLoanSimple call to {aave_pool}"
            );

            // No Balancer call should appear (Balancer isn't on Base).
            let balancer_vault = "0xBA12222222228d8Ba445958a75a0704d566BF2C8".to_lowercase();
            let any_balancer = intent
                .intent_batch
                .calls
                .iter()
                .any(|c| format!("{}", c.target).to_lowercase() == balancer_vault);
            assert!(
                !any_balancer,
                "did not expect a Balancer Vault call on Base"
            );
        }
        other => panic!("expected Eip712Intent, got {:?}", other),
    }
}

#[test]
fn test_base_long_with_explicit_balancer_rejects() {
    // Balancer V2 is not deployed on Base — the compiler must error instead
    // of silently falling back.
    let input = r#"{
        "network": "base",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "long": {
                "collateral": "WETH",
                "amount":     "1.0",
                "leverage":   "2",
                "via":        "balancer",
                "price":      "3000"
            } }
        ]
    }"#;
    let err = compile_base(input).unwrap_err().to_string();
    assert!(
        err.to_lowercase().contains("balancer") && err.to_lowercase().contains("not available"),
        "expected balancer-not-on-base error, got: {err}"
    );
}

#[test]
fn test_base_close_position_uses_aave() {
    let input = r#"{
        "network": "base",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "close_position": {
                "collateral": "WETH",
                "borrow": "USDC",
                "current_debt": "2000",
                "current_collateral": "1.0",
                "slippage": "100"
            } }
        ]
    }"#;
    let result = compile_base(input)
        .expect("close_position on Base should compile via Aave flashloan");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            let aave_pool = "0xA238Dd80C259a72e81d7e4664a9801593F98d1c5".to_lowercase();
            assert!(
                intent.intent_batch.calls.iter().any(|c| {
                    format!("{}", c.target).to_lowercase() == aave_pool
                        && c.call_data.starts_with(&[0x42, 0xb0, 0xb7, 0x7c])
                }),
                "expected an Aave flashLoanSimple call in close_position output",
            );
        }
        other => panic!("expected Eip712Intent, got {:?}", other),
    }
}

// ──────────────────────────────────────────────────────────────────────────
// L1 sanity: explicit `via: aave` produces an Aave flashloan there too,
// proving the new path is genuinely additive (Balancer remains the default).
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn test_l1_long_via_aave_emits_aave_flashloan() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "long": {
                "collateral": "WETH",
                "borrow":     "USDC",
                "amount":     "1.0",
                "leverage":   "3",
                "via":        "aave",
                "slippage":   "50",
                "price":      "3200"
            } }
        ]
    }"#;
    let result = compile_l1(input).expect("L1 via=aave leverage should compile");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            let aave_pool = "0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2".to_lowercase();
            assert!(
                intent.intent_batch.calls.iter().any(|c| {
                    format!("{}", c.target).to_lowercase() == aave_pool
                        && c.call_data.starts_with(&[0x42, 0xb0, 0xb7, 0x7c])
                }),
                "expected an Aave flashLoanSimple call on L1 when via=aave",
            );
            let balancer_vault = "0xBA12222222228d8Ba445958a75a0704d566BF2C8".to_lowercase();
            assert!(
                !intent
                    .intent_batch
                    .calls
                    .iter()
                    .any(|c| format!("{}", c.target).to_lowercase() == balancer_vault),
                "Balancer Vault should NOT appear when via=aave was requested",
            );
        }
        other => panic!("expected Eip712Intent, got {:?}", other),
    }
}

#[test]
fn test_l1_long_default_still_balancer() {
    // Back-compat guard: when `via` is omitted on L1, the default stays
    // Balancer (no premium) so existing flows aren't perturbed.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "long": {
                "collateral": "WETH",
                "borrow":     "USDC",
                "amount":     "1.0",
                "leverage":   "3",
                "slippage":   "50",
                "price":      "3200"
            } }
        ]
    }"#;
    let result = compile_l1(input).expect("L1 default leverage compiles");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            let balancer_vault = "0xBA12222222228d8Ba445958a75a0704d566BF2C8".to_lowercase();
            assert!(
                intent
                    .intent_batch
                    .calls
                    .iter()
                    .any(|c| format!("{}", c.target).to_lowercase() == balancer_vault),
                "expected the default L1 leverage path to still call Balancer Vault",
            );
        }
        other => panic!("expected Eip712Intent, got {:?}", other),
    }
}
