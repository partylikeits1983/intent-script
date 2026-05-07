//! Generate calldata fixture files for Foundry tests.
//!
//! These tests compile intent JSON and write the resulting calldata
//! to files that Foundry tests can read and execute against the router.
//!
//! Run with:
//!   cargo test -p intent-script --test generate_calldata -- --nocapture

use std::path::{Path, PathBuf};

use intent_script::{CompileOutput, CompileResult, compile};

/// Get the path to the config directory at the workspace root.
fn config_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("config")
}

fn load_config() -> (String, String, String) {
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

fn do_compile(input: &str) -> Result<CompileResult, intent_script::error::CompileError> {
    let (c, a, p) = load_config();
    let input = inject_default_timestamp_if_missing(input);
    compile(&input, &c, &a, &p)
}

/// Get the path to the Foundry fixtures directory.
fn fixtures_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("contracts")
        .join("test")
        .join("fixtures")
}

/// Write calldata to a fixture file as a hex string (with 0x prefix).
/// Also writes a companion {name}_value.txt with the ETH value in wei,
/// and {name}_to.txt with the target address.
fn write_calldata(name: &str, result: &CompileResult) {
    let dir = fixtures_dir();
    std::fs::create_dir_all(&dir).expect("create fixtures dir");

    // Extract the transaction to write — for Eip712Intent, use the direct_tx
    let tx = match &result.output {
        CompileOutput::SingleTx(tx) => tx,
        CompileOutput::Eip712Intent(intent) => &intent.direct_tx,
        CompileOutput::TxSequence(txs) => {
            for (i, tx) in txs.iter().enumerate() {
                let hex = format!("0x{}", hex_encode(&tx.data));
                let path = dir.join(format!("{name}_{i}.txt"));
                std::fs::write(&path, &hex).expect("write calldata file");
                println!(
                    "Wrote calldata to {}: {} bytes",
                    path.display(),
                    tx.data.len()
                );
            }
            return;
        }
        CompileOutput::RequiresExecutor { reason } => {
            panic!("Cannot generate calldata: {reason}");
        }
    };

    let hex = format!("0x{}", hex_encode(&tx.data));
    let path = dir.join(format!("{name}.txt"));
    std::fs::write(&path, &hex).expect("write calldata file");

    // Write value file
    let value_path = dir.join(format!("{name}_value.txt"));
    std::fs::write(&value_path, tx.value.to_string()).expect("write value file");

    // Write target address file
    let to_path = dir.join(format!("{name}_to.txt"));
    std::fs::write(&to_path, format!("{}", tx.to)).expect("write to file");

    println!(
        "Wrote calldata to {}: {} bytes",
        path.display(),
        tx.data.len()
    );
    println!("  to: {}", tx.to);
    println!("  value: {}", tx.value);
    println!("  calldata: {hex}");
}

fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn generate_wrap_eth_calldata() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1.0" } }
        ]
    }"#;

    let output = do_compile(input).expect("compile should succeed");
    write_calldata("wrap_eth", &output);

    println!("Generated calldata for wrap ETH: {:?}", output);
}

#[test]
fn generate_aave_deposit_calldata() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let output = do_compile(input).expect("compile should succeed");
    write_calldata("aave_deposit_usdc", &output);
}

#[test]
fn generate_swap_usdc_weth_calldata() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "deadline": 9999999999,
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "min_amount_out": "0.1" } }
        ]
    }"#;

    let output = do_compile(input).expect("compile should succeed");
    write_calldata("swap_usdc_weth", &output);
}

#[test]
fn generate_deposit_borrow_calldata() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } },
            { "borrow": { "asset": "DAI", "amount": "2000", "from": "aave" } }
        ]
    }"#;

    let output = do_compile(input).expect("compile should succeed");
    write_calldata("deposit_borrow", &output);
}

#[test]
fn generate_swap_deposit_borrow_calldata() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "5000", "to": "WETH", "min_amount_out": "2.0" } },
            { "deposit": { "asset": "WETH", "amount": "all", "into": "aave" } },
            { "borrow": { "asset": "DAI", "amount": "1000", "from": "aave" } }
        ]
    }"#;

    let output = do_compile(input).expect("compile should succeed");
    write_calldata("swap_deposit_borrow", &output);
}

#[test]
fn generate_stake_eth_lido_calldata() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "stake": { "asset": "ETH", "amount": "10.0", "into": "lido" } }
        ]
    }"#;

    let output = do_compile(input).expect("compile should succeed");
    write_calldata("stake_eth_lido", &output);
}

#[test]
fn generate_aave_withdraw_calldata() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "withdraw": { "asset": "USDC", "amount": "5000", "from": "aave" } }
        ]
    }"#;

    let output = do_compile(input).expect("compile should succeed");
    write_calldata("aave_withdraw_usdc", &output);
}

#[test]
fn generate_complex_defi_calldata() {
    // Read from the actual example file — the canonical complex DeFi intent
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path = std::path::Path::new(manifest_dir).join("examples/complex_defi.json");
    let input =
        std::fs::read_to_string(&example_path).expect("should read complex_defi.json example file");

    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("complex_defi", &output);
}

#[test]
fn generate_stake_lido_wsteth_calldata() {
    // Read from the actual example file
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path = std::path::Path::new(manifest_dir).join("examples/stake_lido_wsteth.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read stake_lido_wsteth.json example file");

    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("stake_lido_wsteth", &output);
}

#[test]
fn generate_lido_request_withdrawal_calldata() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path =
        std::path::Path::new(manifest_dir).join("examples/lido_request_withdrawal.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read lido_request_withdrawal.json example file");

    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("lido_request_withdrawal", &output);
}

#[test]
fn generate_lp_mint_usdc_weth_calldata() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path = std::path::Path::new(manifest_dir).join("examples/lp_mint_usdc_weth.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read lp_mint_usdc_weth.json example file");

    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("lp_mint_usdc_weth", &output);
}

#[test]
fn generate_morpho_supply_collateral_and_borrow_calldata() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path =
        std::path::Path::new(manifest_dir).join("examples/morpho_collateral_borrow.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read morpho_collateral_borrow.json example file");

    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("morpho_borrow_usdc_against_weth", &output);
}

#[test]
fn generate_bridge_usdc_arbitrum_calldata() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path =
        std::path::Path::new(manifest_dir).join("examples/bridge_usdc_arbitrum.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read bridge_usdc_arbitrum.json example file");

    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("bridge_usdc_to_arbitrum", &output);
}

#[test]
fn generate_lp_mint_usdc_weth_wide_calldata() {
    // Wide tick range LP mint for the fork lifecycle test. The range covers
    // ETH ~ $1200..$5900 so the mint succeeds at any realistic fork block.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path =
        std::path::Path::new(manifest_dir).join("examples/lp_mint_usdc_weth_wide.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read lp_mint_usdc_weth_wide.json example file");

    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("lp_mint_usdc_weth_wide", &output);
}

#[test]
fn generate_long_wsteth_leverage_calldata() {
    // 1.5x long wstETH via Balancer flashloan + Aave V3 supply/borrow +
    // Uniswap V3 swap. wstETH is used instead of WETH because Aave V3 set
    // WETH LTV to 0 on mainnet, so borrowing against WETH reverts. 1.5x
    // (vs 2x) gives a comfortable LTV margin plus swap slippage buffer
    // against current pool-vs-oracle divergence on the USDC/wstETH pair.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path = std::path::Path::new(manifest_dir).join("examples/long_wsteth_1_5x.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read long_wsteth_1_5x.json example file");

    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("long_wsteth_1_5x", &output);
}

#[test]
fn generate_flashloan_aave_loop_calldata() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path = std::path::Path::new(manifest_dir).join("examples/flashloan_aave_loop.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read flashloan_aave_loop.json example file");

    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("loop_aave_3x_weth", &output);
}

#[test]
fn generate_aave_deposit_usdt_borrow_weth_calldata() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path =
        std::path::Path::new(manifest_dir).join("examples/aave_deposit_usdt_borrow_weth.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read aave_deposit_usdt_borrow_weth.json example file");
    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("aave_deposit_usdt_borrow_weth", &output);
}

#[test]
fn generate_aave_deposit_usdc_borrow_usdt_calldata() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path =
        std::path::Path::new(manifest_dir).join("examples/aave_deposit_usdc_borrow_usdt.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read aave_deposit_usdc_borrow_usdt.json example file");
    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("aave_deposit_usdc_borrow_usdt", &output);
}

#[test]
fn generate_aave_deposit_wsteth_borrow_usdc_calldata() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path =
        std::path::Path::new(manifest_dir).join("examples/aave_deposit_wsteth_borrow_usdc.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read aave_deposit_wsteth_borrow_usdc.json example file");
    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("aave_deposit_wsteth_borrow_usdc", &output);
}

#[test]
fn generate_stake_wrap_deposit_borrow_calldata() {
    // The "complex DeFi" UX flow: 100 ETH → Lido stETH → wstETH → Aave V3
    // collateral → 20k USDC borrow, all in a single signed intent. Exercises
    // the wstETH conversion in `step_produces` — without it the deposit's
    // "all" resolves to the stETH amount (~99.8) and the on-chain
    // transferFrom reverts because the router holds ~stETH/rate wstETH.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path =
        std::path::Path::new(manifest_dir).join("examples/stake_wrap_deposit_borrow.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read stake_wrap_deposit_borrow.json example file");
    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("stake_wrap_deposit_borrow", &output);
}

#[test]
fn generate_long_eth_3x_balancer_calldata() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path =
        std::path::Path::new(manifest_dir).join("examples/long_eth_3x_balancer.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read long_eth_3x_balancer.json example file");
    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("long_eth_3x_balancer", &output);
}

#[test]
fn generate_short_eth_1x_calldata() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path = std::path::Path::new(manifest_dir).join("examples/short_eth_1x.json");
    let input =
        std::fs::read_to_string(&example_path).expect("should read short_eth_1x.json example file");
    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("short_eth_1x", &output);
}

#[test]
fn generate_lp_mint_usdc_usdt_0p95_1p05_calldata() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path =
        std::path::Path::new(manifest_dir).join("examples/lp_mint_usdc_usdt_0p95_1p05.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read lp_mint_usdc_usdt_0p95_1p05.json example file");
    let output = do_compile(&input).expect("compile should succeed");
    write_calldata("lp_mint_usdc_usdt_0p95_1p05", &output);
}

#[test]
fn generate_morpho_supply_usdc_calldata() {
    // Single-step loan-side supply: exercises the MorphoSupply variant
    // with no `as: "collateral"` discriminator.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "morpho",
                           "market": "USDC-WETH-86" } }
        ]
    }"#;
    let output = do_compile(input).expect("compile should succeed");
    write_calldata("morpho_supply_usdc", &output);
}
