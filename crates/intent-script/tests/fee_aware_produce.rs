//! Phase 2: compiler fee-awareness tests.
//!
//! Verifies that `step_produces` applies the router's sweep-time fee so
//! downstream `"all"` chains resolve to the post-fee floor that will actually
//! be present in the router at runtime.

use std::path::{Path, PathBuf};

use alloy_primitives::U256;
use intent_script::compiler::normalize::normalize;
use intent_script::ir::ResolvedStep;
use intent_script::registry::RegistryContext;
use intent_script::schema::IntentScript;

fn config_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("config")
}

fn load_registry(network: &str, protocols_override: Option<&str>) -> RegistryContext {
    let dir = config_dir();
    let chains = std::fs::read_to_string(dir.join("chains.json")).unwrap();
    let assets_path = match network {
        // Sepolia has no intent_router entry — we use its asset table with a
        // custom protocols JSON to exercise the zero-fee path.
        "sepolia" => dir.join("assets/sepolia.json"),
        _ => dir.join("assets/anvil.json"),
    };
    let assets = std::fs::read_to_string(assets_path).unwrap();
    let protocols = match protocols_override {
        Some(p) => p.to_string(),
        None => std::fs::read_to_string(dir.join(format!("protocols/{}.json", network))).unwrap(),
    };
    // Registry uses the network name to look up the chain entry; anvil/sepolia
    // both exist in chains.json.
    RegistryContext::load(&chains, &assets, &protocols, network).unwrap()
}

fn resolve(input: &str, registry: &RegistryContext) -> Vec<ResolvedStep> {
    let script: IntentScript = serde_json::from_str(input).expect("parse IntentScript");
    let norm = normalize(&script, registry).expect("normalize");
    norm.intent.steps
}

fn supply_amount(step: &ResolvedStep) -> U256 {
    match step {
        ResolvedStep::AaveV3Supply { amount, .. } => *amount,
        other => panic!("expected AaveV3Supply, got {other:?}"),
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────

/// Anvil config sets `fee_bps=10`. A swap with `min_amount_out=1000 USDC`
/// followed by `deposit "all"` resolves the deposit to `1000 * 9990 / 10_000
/// = 999 USDC` in base units.
#[test]
fn test_fee_aware_all_deposit_reduces_by_10bps() {
    let registry = load_registry("anvil", None);
    assert_eq!(
        registry.fee_bps(),
        10,
        "anvil config should have fee_bps=10"
    );

    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "WETH", "amount": "1.0", "to": "USDC", "min_amount_out": "1000" } },
            { "deposit": { "asset": "USDC", "amount": "all", "into": "aave" } }
        ]
    }"#;

    let steps = resolve(input, &registry);
    let deposit = steps.last().expect("deposit step present");

    // 1000 USDC (6 decimals) = 1_000_000_000 base units.
    // After 10 bps fee: 1_000_000_000 * 9990 / 10_000 = 999_000_000.
    assert_eq!(supply_amount(deposit), U256::from(999_000_000u64));
}

/// A config without any `intent_router` entry (or with fee_bps absent) yields
/// `fee_bps = 0`, so `"all"` resolves to the raw produced amount.
#[test]
fn test_fee_aware_zero_fee_no_reduction() {
    // Custom anvil-shaped protocols JSON with NO intent_router entry.
    // Registry falls back to fee_bps = 0.
    let protocols_no_router = r#"{
        "aave": {
            "type": "lending", "version": "v3",
            "contracts": { "pool": "0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2" }
        },
        "uniswap": {
            "type": "dex", "version": "v3",
            "contracts": {
                "router": "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45",
                "quoter": "0xb27308f9F90D607463bb33eA1BeBb41C27CE5AB6"
            }
        }
    }"#;
    let registry = load_registry("anvil", Some(protocols_no_router));
    assert_eq!(registry.fee_bps(), 0, "no intent_router → fee_bps=0");

    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "WETH", "amount": "1.0", "to": "USDC", "min_amount_out": "1000" } },
            { "deposit": { "asset": "USDC", "amount": "all", "into": "aave" } }
        ]
    }"#;

    let steps = resolve(input, &registry);
    let deposit = steps.last().expect("deposit step present");

    // With no fee, "all" resolves to the raw min_amount_out.
    assert_eq!(supply_amount(deposit), U256::from(1_000_000_000u64));
}

/// Explicit amount strings bypass the fee discount — the user asked for a
/// literal 500 USDC, so that's what the resolved IR gets.
#[test]
fn test_fee_aware_does_not_affect_explicit_amount() {
    let registry = load_registry("anvil", None);
    assert_eq!(registry.fee_bps(), 10);

    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "500", "into": "aave" } }
        ]
    }"#;

    let steps = resolve(input, &registry);
    let deposit = &steps[0];

    // 500 USDC literal = 500_000_000 base units, unadjusted.
    assert_eq!(supply_amount(deposit), U256::from(500_000_000u64));
}

/// Anvil config literally contains `"fee_bps": 10`. Sanity-check the loader
/// picks it up so the other assertions here mean what they say.
#[test]
fn test_registry_fee_bps_reads_config() {
    let anvil = load_registry("anvil", None);
    assert_eq!(anvil.fee_bps(), 10);

    // sepolia.json has no intent_router → fee_bps defaults to 0.
    let sepolia = load_registry("sepolia", None);
    assert_eq!(sepolia.fee_bps(), 0);
}
