//! Generate fixture files for the IntentForkIntegration fork test suite.
//!
//! For each of the 8 integration scenarios, this:
//!   1. Reads the DSL JSON from `examples/<name>.json`.
//!   2. Compiles via the real `intent_script::compile` entrypoint.
//!   3. Writes two fixture files into `contracts/test/fixtures/`:
//!        - `<name>_batch.json` -- human-readable mirror of the EIP-712 batch
//!        - `<name>_batch.bin`  -- hex-encoded `abi.encode(IntentBatch)` for
//!          Solidity-side `abi.decode` consumption (sidesteps nested-struct
//!          JSON parsing pain).
//!
//! The two `lp_close_usdc_usdt` / `lp_collect_usdc_usdt` examples carry a
//! `{{POSITION_ID}}` placeholder. By default we substitute `1` so the
//! generator runs at all; the Solidity side reuses these as templates and
//! patches the position id in-place using the tokenId minted at runtime.
//!
//! Run with:
//!   cargo test -p intent-script --test generate_integration_fixtures -- --nocapture

use std::path::{Path, PathBuf};

use alloy_primitives::{Address, Bytes as AlloyBytes, U256};
use alloy_sol_types::{SolValue, sol};
use intent_script::output::CompileOutputJson;
use intent_script::{CompileOutput, CompileResult, compile};

// ─── Solidity struct mirrors for abi.encode / abi.decode round-trip ────────
//
// These must stay in lockstep with the matching structs in
// contracts/src/IntentRouter.sol (`Call` and `IntentBatch`). When Solidity
// reads <name>_batch.bin it does:
//
//     bytes memory raw = vm.parseBytes(vm.readFile(...));
//     IntentRouter.IntentBatch memory b = abi.decode(raw, (IntentRouter.IntentBatch));
//
// For that round-trip to work, the field order, types, and dynamic/static
// classification must match Solidity exactly.

sol! {
    struct SolCall {
        address target;
        bytes callData;
        uint256 value;
    }

    struct SolIntentBatch {
        address signer;
        SolCall[] calls;
        address[] tokensToSweep;
        uint256 nonce;
        uint256 deadline;
        uint256 totalValue;
    }

    /// Mirrors Solidity-side decoding for SingleTx scenarios:
    ///   (address to, uint256 value, bytes data) = abi.decode(raw, (address, uint256, bytes));
    struct SolSingleTx {
        address to;
        uint256 value;
        bytes data;
    }
}

fn config_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("config")
}

fn examples_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir).join("examples")
}

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

/// Load the ethereum mainnet config (assets + protocols). The integration
/// fork tests etch IntentRouter at the address baked into
/// `protocols/ethereum.json` (intent_router.router), so the calldata in
/// each compiled batch must reference that same address. Loading the anvil
/// config here would produce calldata pointing at the *anvil* router
/// address (different), and the etched router at the ethereum address
/// would never receive the transferFrom'd tokens.
fn load_eth_config() -> (String, String, String) {
    let dir = config_dir();
    let chains = std::fs::read_to_string(dir.join("chains.json")).unwrap();
    let assets = std::fs::read_to_string(dir.join("assets/ethereum.json")).unwrap();
    let protocols = std::fs::read_to_string(dir.join("protocols/ethereum.json")).unwrap();
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

/// Read `examples/<name>.json` and apply any necessary template substitutions.
/// Currently only `{{POSITION_ID}}` is recognized — for the LP close/collect
/// templates. Solidity overrides via the `INTEGRATION_POSITION_ID` env var
/// when calling this generator from inside a test; otherwise we fall back to
/// `"1"` so `make generate-fixtures` runs cleanly.
fn read_dsl(name: &str) -> String {
    let path = examples_dir().join(format!("{name}.json"));
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let position_id = std::env::var("INTEGRATION_POSITION_ID").unwrap_or_else(|_| "1".to_string());
    raw.replace("{{POSITION_ID}}", &position_id)
}

fn do_compile(input: &str) -> Result<CompileResult, intent_script::error::CompileError> {
    let (c, a, p) = load_eth_config();
    let input = inject_default_timestamp_if_missing(input);
    compile(&input, &c, &a, &p)
}

fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

/// Write `<name>_batch.json` + `<name>_batch.bin`.
///
/// `_batch.bin` is the canonical input for the Solidity test suite — Solidity
/// reads it, `abi.decode`s into `IntentRouter.IntentBatch`, re-derives the
/// EIP-712 digest against the etched router's `DOMAIN_SEPARATOR()`, signs,
/// and submits via `executeSigned`.
///
/// The `.json` mirror is for human inspection / debugging only.
fn write_integration_fixture(name: &str, result: &CompileResult) {
    let dir = fixtures_dir();
    std::fs::create_dir_all(&dir).expect("create fixtures dir");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            // ── Solidity-decodable bin ──
            let calls: Vec<SolCall> = intent
                .intent_batch
                .calls
                .iter()
                .map(|c| SolCall {
                    target: c.target,
                    callData: AlloyBytes::from(c.call_data.to_vec()),
                    value: c.value,
                })
                .collect();
            let sweep: Vec<Address> = intent.intent_batch.tokens_to_sweep.clone();

            let sol_batch = SolIntentBatch {
                signer: intent.intent_batch.signer,
                calls,
                tokensToSweep: sweep,
                nonce: U256::from(intent.intent_batch.nonce),
                deadline: U256::from(intent.intent_batch.deadline),
                totalValue: intent.intent_batch.total_value,
            };
            let encoded = sol_batch.abi_encode();
            let bin_path = dir.join(format!("{name}_batch.bin"));
            std::fs::write(&bin_path, format!("0x{}", hex_encode(&encoded)))
                .expect("write batch bin");
            println!(
                "Wrote {} ({} bytes encoded)",
                bin_path.display(),
                encoded.len()
            );

            // ── Human-readable mirror ──
            let calls_json: Vec<serde_json::Value> = intent
                .intent_batch
                .calls
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "target":   format!("{}", c.target),
                        "callData": format!("0x{}", hex_encode(&c.call_data)),
                        "value":    c.value.to_string(),
                    })
                })
                .collect();
            let tokens_json: Vec<String> = intent
                .intent_batch
                .tokens_to_sweep
                .iter()
                .map(|a| format!("{}", a))
                .collect();
            let mirror = serde_json::json!({
                "signer":         format!("{}", intent.intent_batch.signer),
                "calls":          calls_json,
                "tokensToSweep":  tokens_json,
                "nonce":          intent.intent_batch.nonce,
                "deadline":       intent.intent_batch.deadline,
                "totalValue":     intent.intent_batch.total_value.to_string(),
                "routerAddress":  format!("{}", intent.direct_tx.to),
                "chainId":        intent.domain.chain_id,
                "description":    &intent.description,
            });
            let json_path = dir.join(format!("{name}_batch.json"));
            std::fs::write(
                &json_path,
                serde_json::to_string_pretty(&mirror).expect("serialize mirror"),
            )
            .expect("write batch json");

            // Also dump the typed-data JSON for wallet-style debugging.
            let typed = CompileOutputJson::from(result);
            let typed_path = dir.join(format!("{name}_eip712.json"));
            std::fs::write(
                &typed_path,
                serde_json::to_string_pretty(&typed).expect("serialize typed data"),
            )
            .expect("write eip712 json");

            println!(
                "  {name}: signer={} calls={} sweep={} totalValue={}",
                intent.intent_batch.signer,
                intent.intent_batch.calls.len(),
                intent.intent_batch.tokens_to_sweep.len(),
                intent.intent_batch.total_value
            );
        }
        CompileOutput::SingleTx(tx) => {
            // Single-call DSLs (e.g. a standalone lp_collect) are emitted by
            // the compiler as a direct user-to-target tx with no router
            // wrapping. We dump them as `_single.bin` so the Solidity test
            // can `vm.prank(user); target.call{value: ...}(data);` instead
            // of going through executeSigned. The signing path doesn't
            // apply — the user's own EOA tx is the authorization.
            let s = SolSingleTx {
                to: tx.to,
                value: tx.value,
                data: AlloyBytes::from(tx.data.to_vec()),
            };
            let encoded = s.abi_encode();
            let bin_path = dir.join(format!("{name}_single.bin"));
            std::fs::write(&bin_path, format!("0x{}", hex_encode(&encoded)))
                .expect("write single bin");
            println!(
                "Wrote {} (SingleTx, to={} value={} data={}b)",
                bin_path.display(),
                tx.to,
                tx.value,
                tx.data.len()
            );
        }
        CompileOutput::TxSequence(_) => {
            panic!("integration fixture {name}: expected Eip712Intent, got TxSequence");
        }
        CompileOutput::RequiresExecutor { reason } => {
            panic!("integration fixture {name}: RequiresExecutor: {reason}");
        }
    }
}

/// Compile `examples/<name>.json` and write fixtures. Wraps the boilerplate
/// of the per-scenario tests below.
fn build(name: &str) {
    let input = read_dsl(name);
    let result = do_compile(&input).unwrap_or_else(|e| panic!("compile {name}: {e:?}"));
    if !result.warnings.is_empty() {
        println!("warnings for {name}:");
        for w in &result.warnings {
            println!("  - {w}");
        }
    }
    write_integration_fixture(name, &result);
}

// ─── One #[test] per scenario so they appear individually in test output ──

#[test]
fn build_lp_mint_usdc_usdt_5pct() {
    build("lp_mint_usdc_usdt_5pct");
}

#[test]
fn build_lp_close_usdc_usdt() {
    // Position id is patched at runtime in Solidity for the live test path;
    // here we just verify the DSL compiles with the placeholder substitution.
    build("lp_close_usdc_usdt");
}

#[test]
fn build_lp_collect_usdc_usdt() {
    build("lp_collect_usdc_usdt");
}

#[test]
fn build_swap_eth_wbtc_supply_borrow_usdt() {
    build("swap_eth_wbtc_supply_borrow_usdt");
}

#[test]
fn build_lp_mint_eth_wbtc_10pct() {
    build("lp_mint_eth_wbtc_10pct");
}

#[test]
fn build_lp_mint_eth_usdc() {
    build("lp_mint_eth_usdc");
}

#[test]
fn build_long_eth_4x() {
    build("long_eth_4x");
}

#[test]
fn build_short_eth_3x() {
    build("short_eth_3x");
}
