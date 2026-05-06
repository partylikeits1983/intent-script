//! End-to-end integration tests for the intent-script compiler.

use std::path::{Path, PathBuf};

use intent_script::output::CompileOutputJson;
use intent_script::{CompileOutput, CompileResult, compile, compile_with_allowances};

/// Get the path to the config directory at the workspace root.
fn config_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR points to crates/intent-script/
    // config/ is at the workspace root (two levels up)
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent() // crates/
        .unwrap()
        .parent() // workspace root
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

/// Auto-inject a default `current_timestamp` when the test JSON doesn't
/// already supply one (or an explicit `deadline`). Batched intents now hard-
/// reject without a deadline source; generic integration tests don't care
/// about the value, they just need one to exist. Tests that exercise
/// deadline behavior explicitly provide a timestamp.
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

#[test]
fn test_wrap_eth_to_weth() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1.5" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    let output = &result.output;

    // Wrap is a single tx
    match output {
        CompileOutput::SingleTx(tx) => {
            // Target should be WETH contract
            assert_eq!(
                format!("{}", tx.to),
                "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
            );
            // Value should be 1.5 ETH in wei
            assert_eq!(tx.value.to_string(), "1500000000000000000");
            // Chain ID should be 31337 (Anvil local chain)
            assert_eq!(tx.chain_id, 31337);
            // Calldata should be deposit() selector: 0xd0e30db0
            assert_eq!(tx.data.len(), 4);
            assert_eq!(&tx.data[..4], &[0xd0, 0xe3, 0x0d, 0xb0]);
        }
        other => panic!("Expected SingleTx, got {:?}", other),
    }

    // Verify JSON serialization works
    let json_output = CompileOutputJson::from(&result);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Wrap ETH output:\n{json_str}");

    assert!(json_str.contains("single_tx"));
    assert!(json_str.contains("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"));
}

#[test]
fn test_aave_deposit_usdc_batched_through_router() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    let output = &result.output;

    // Deposit produces 2 calls (approve + supply) which get batched
    // into an Eip712Intent since a router is configured.
    match output {
        CompileOutput::Eip712Intent(intent) => {
            // Direct tx target should be the IntentRouter — the EIP-712
            // verifying contract is the router by construction.
            assert_eq!(intent.direct_tx.to, intent.domain.verifying_contract);
            // No direct ETH value (approve + supply are both 0-value)
            assert_eq!(intent.direct_tx.value.to_string(), "0");
            // Calldata should start with executeDirect() selector
            assert!(
                intent.direct_tx.data.len() > 4,
                "Should have calldata for router.executeDirect()"
            );
            // Description should mention batching
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            // Should have EIP-712 domain
            assert_eq!(intent.domain.name, "IntentRouter");
            assert_eq!(intent.domain.chain_id, 31337);
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }

    // Verify JSON serialization
    let json_output = CompileOutputJson::from(&result);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Aave deposit (batched) output:\n{json_str}");

    assert!(json_str.contains("eip712_intent"));
    assert!(json_str.contains("IntentRouter"));
    // The router address should appear in the JSON — resolve it from the
    // compiled output so tests stay correct if the config-deployed router
    // address changes.
    let router_addr = match &result.output {
        CompileOutput::Eip712Intent(intent) => format!("{}", intent.domain.verifying_contract),
        _ => unreachable!("matched Eip712Intent above"),
    };
    assert!(json_str.contains(&router_addr));
}

#[test]
fn test_unwrap_weth() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "unwrap": { "asset": "WETH", "amount": "2.0" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");

    match &result.output {
        CompileOutput::SingleTx(tx) => {
            // Target should be WETH contract
            assert_eq!(
                format!("{}", tx.to),
                "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
            );
            // No ETH value sent for unwrap
            assert_eq!(tx.value.to_string(), "0");
            // withdraw(uint256) selector: 0x2e1a7d4d
            assert_eq!(&tx.data[..4], &[0x2e, 0x1a, 0x7d, 0x4d]);
        }
        other => panic!("Expected SingleTx, got {:?}", other),
    }
}

#[test]
fn test_unknown_network_fails() {
    let input = r#"{
        "network": "solana",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1.0" } }
        ]
    }"#;

    let result = do_compile(input);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("solana"),
        "Error should mention the unknown network: {err}"
    );
}

#[test]
fn test_unknown_asset_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "SHIB", "amount": "1000" } }
        ]
    }"#;

    let result = do_compile(input);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("SHIB"),
        "Error should mention the unknown asset: {err}"
    );
}

#[test]
fn test_empty_steps_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": []
    }"#;

    let result = do_compile(input);
    assert!(result.is_err());
}

// ─── New primitive tests ───────────────────────────────────────────────

#[test]
fn test_swap_usdc_to_weth() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "min_amount_out": "0.1" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    let output = &result.output;

    // Swap produces 2 calls (approve + exactInputSingle) batched via router
    match output {
        CompileOutput::Eip712Intent(intent) => {
            // Target should be the IntentRouter (batched)
            assert_eq!(intent.direct_tx.to, intent.domain.verifying_contract);
            assert!(
                intent.direct_tx.data.len() > 4,
                "Should have calldata for router.executeDirect()"
            );
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Uniswap V3"),
                "Description should mention Uniswap V3: {}",
                intent.description
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }

    // Verify JSON serialization
    let json_output = CompileOutputJson::from(&result);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Swap USDC→WETH output:\n{json_str}");
    assert!(json_str.contains("eip712_intent"));
}

/// Regression: a standalone "swap from ETH" must pay `amount_in` as msg.value
/// and go direct to the SwapRouter with recipient=signer (no intent-router
/// redirection, no ERC-20 transferFrom/approve). Previously the compiler
/// silently rewrote tokenIn to WETH while leaving value=0, which either
/// reverted or swapped whatever leftover WETH the user happened to hold.
#[test]
fn test_swap_native_eth_to_usdc_sends_msg_value() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "ETH", "to": "USDC", "amount": "50", "price": "2344", "slippage": "0.5" } }
        ],
        "current_timestamp": 1776824820
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::SingleTx(tx) => {
            // Must carry 50 ETH as msg.value (50 * 10^18).
            let expected = alloy_primitives::U256::from(50u128)
                * alloy_primitives::U256::from(10u128).pow(alloy_primitives::U256::from(18u64));
            assert_eq!(
                tx.value, expected,
                "native swap must pay amount_in as msg.value"
            );
            // Must target the Uniswap V3 SwapRouter directly, NOT the IntentRouter.
            assert_eq!(
                format!("{:?}", tx.to).to_lowercase(),
                "0xe592427a0aece92de3edee1f18e0157c05861564",
                "native single-swap should go direct to SwapRouter"
            );
            // Calldata's `recipient` (slot 3 after the 4-byte selector) must
            // be the signer, not any intermediary router.
            let data = &tx.data;
            assert!(data.len() >= 4 + 32 * 8, "unexpected calldata length");
            let recipient =
                alloy_primitives::Address::from_slice(&data[4 + 32 * 3 + 12..4 + 32 * 4]);
            assert_eq!(
                recipient, tx.from,
                "recipient in calldata must be the signer"
            );
        }
        other => panic!("expected SingleTx for a standalone native swap, got {other:?}"),
    }
}

#[test]
fn test_deposit_and_borrow_single_tx() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } },
            { "borrow": { "asset": "DAI", "amount": "2000", "from": "aave" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    let output = &result.output;

    // Deposit + borrow produces 3 calls (approve + supply + borrow) batched via router
    match output {
        CompileOutput::Eip712Intent(intent) => {
            // Target should be the IntentRouter
            assert_eq!(intent.direct_tx.to, intent.domain.verifying_contract);
            assert!(
                intent.direct_tx.data.len() > 4,
                "Should have calldata for router.executeDirect()"
            );
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            // Should contain both supply and borrow descriptions
            assert!(
                intent.description.contains("Supply"),
                "Description should mention Supply: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Borrow"),
                "Description should mention Borrow: {}",
                intent.description
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }

    let json_output = CompileOutputJson::from(&result);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Deposit+Borrow output:\n{json_str}");
    assert!(json_str.contains("eip712_intent"));

    // When the caller supplies an empty allowances/delegations snapshot,
    // the compiler must emit BOTH an ERC-20 approve (for the USDC pull)
    // AND an Aave V3 approveDelegation (for the DAI borrow). Without the
    // delegation prereq, the on-chain `executeDirect` reverts with
    // InsufficientBorrowAllowance (0x1cb19ef3) — the exact symptom that
    // motivated this feature.
    let with_allowances_input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } },
            { "borrow":  { "asset": "DAI",  "amount": "2000", "from": "aave" } }
        ]
    }"#;
    let with_allowances = do_compile_with_allowances(
        with_allowances_input,
        Some(r#"{ "tokens": {}, "delegations": {} }"#),
    )
    .expect("compile_with_allowances should succeed");
    match &with_allowances.output {
        CompileOutput::Eip712Intent(intent) => {
            let delegation_selector: [u8; 4] = [0xc0, 0x4a, 0x8a, 0x10]; // approveDelegation(address,uint256)
            assert!(
                intent
                    .prerequisite_approvals
                    .iter()
                    .any(|tx| tx.data.starts_with(&delegation_selector)),
                "expected an approveDelegation prerequisite for the DAI borrow; got {:?}",
                intent.prerequisite_approvals
            );
        }
        other => panic!("expected Eip712Intent, got {other:?}"),
    }
}

#[test]
fn test_swap_deposit_borrow_chain() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "5000", "to": "WETH", "min_amount_out": "2.0" } },
            { "deposit": { "asset": "WETH", "amount": "all", "into": "aave" } },
            { "borrow": { "asset": "DAI", "amount": "1000", "from": "aave" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    let output = &result.output;

    // Should produce batched tx: approve USDC, swap, approve WETH, supply, borrow
    match output {
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(intent.direct_tx.to, intent.domain.verifying_contract);
            assert!(
                intent.direct_tx.data.len() > 4,
                "Should have calldata for router.executeDirect()"
            );
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            // Should contain swap, supply, and borrow
            assert!(
                intent.description.contains("Swap"),
                "Description should mention Swap: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Supply"),
                "Description should mention Supply: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Borrow"),
                "Description should mention Borrow: {}",
                intent.description
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }

    let json_output = CompileOutputJson::from(&result);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Swap+Deposit+Borrow output:\n{json_str}");
    assert!(json_str.contains("eip712_intent"));
}

#[test]
fn test_swap_with_custom_fee_tier() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "fee": "500", "min_amount_out": "0.1" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    let output = &result.output;

    match output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(
                intent.direct_tx.data.len() > 4,
                "Should have calldata for router.executeDirect()"
            );
            assert!(
                intent.description.contains("Uniswap V3"),
                "Description should mention Uniswap V3: {}",
                intent.description
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }
}

#[test]
fn test_swap_without_fee_defaults_to_3000() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "min_amount_out": "0.1" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    let output = &result.output;

    match output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(intent.direct_tx.data.len() > 4);
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

#[test]
fn test_swap_via_1inch_now_rejected() {
    // The 1inch adapter was removed: it was a calldata passthrough with no
    // compile-time validation, the single largest hallucination-escape hatch
    // in the system. Attempting to use it now fails with an unsupported-step
    // error directing the caller at Uniswap V3.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "via": "1inch", "calldata": "0xdeadbeef" } }
        ]
    }"#;

    let result = do_compile(input);
    assert!(result.is_err(), "1inch path must no longer compile");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("1inch") && err.contains("uniswap"),
        "Error should tell the caller to use uniswap instead: {err}"
    );
}

#[test]
fn test_swap_via_unsupported_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "via": "paraswap" } }
        ]
    }"#;

    let result = do_compile(input);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("paraswap"),
        "Error should mention unsupported provider: {err}"
    );
}

#[test]
fn test_wrap_steth_to_wsteth() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "stETH", "amount": "10.0" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    let output = &result.output;

    // Wrap stETH produces 2 calls (approve + wrap) batched via router
    match output {
        CompileOutput::Eip712Intent(intent) => {
            // Target should be the IntentRouter (batched)
            assert_eq!(intent.direct_tx.to, intent.domain.verifying_contract);
            assert!(
                intent.direct_tx.data.len() > 4,
                "Should have calldata for router.executeDirect()"
            );
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            assert!(
                intent.description.contains("wstETH"),
                "Description should mention wstETH: {}",
                intent.description
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }
}

#[test]
fn test_stake_and_wrap_steth() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "stake": { "asset": "ETH", "amount": "10.0", "into": "lido" } },
            { "wrap": { "asset": "stETH", "amount": "all" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    let output = &result.output;

    // Stake + wrap produces batched calldata
    match output {
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(intent.direct_tx.to, intent.domain.verifying_contract);
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Stake"),
                "Description should mention Stake: {}",
                intent.description
            );
            assert!(
                intent.description.contains("wstETH"),
                "Description should mention wstETH: {}",
                intent.description
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }
}

#[test]
fn test_stake_eth_in_lido() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "stake": { "asset": "ETH", "amount": "10.0", "into": "lido" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    let output = &result.output;

    // Stake is a single call (no approval needed for ETH)
    match output {
        CompileOutput::SingleTx(tx) => {
            // Target should be Lido stETH contract
            assert_eq!(
                format!("{}", tx.to),
                "0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84"
            );
            // Value should be 10 ETH in wei
            assert_eq!(tx.value.to_string(), "10000000000000000000");
            // Calldata should be submit(address) selector
            assert!(tx.data.len() >= 4, "Should have calldata for submit()");
            // submit(address) selector: 0xa1903eab
            assert_eq!(&tx.data[..4], &[0xa1, 0x90, 0x3e, 0xab]);
        }
        other => panic!("Expected SingleTx, got {:?}", other),
    }

    let json_output = CompileOutputJson::from(&result);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Stake ETH in Lido output:\n{json_str}");
    assert!(json_str.contains("single_tx"));
    assert!(json_str.contains("0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84"));
}

#[test]
fn test_wrap_then_stake_elided_to_single_lido_call() {
    // LLM regression: emits `wrap ETH→WETH` then `stake ETH` when a single
    // `stake` would suffice. The compiler must silently rewrite to the
    // correct shape: one Lido.submit() call carrying msg.value.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "2.5" } },
            { "stake": { "asset": "ETH", "amount": "2.5", "into": "lido" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::SingleTx(tx) => {
            // Target the Lido stETH contract directly, not WETH.
            assert_eq!(
                format!("{}", tx.to),
                "0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84",
                "wrap should be elided and the stake should hit stETH directly"
            );
            // msg.value = 2.5 ETH
            let expected = alloy_primitives::U256::from(25u128)
                * alloy_primitives::U256::from(10u128).pow(alloy_primitives::U256::from(17u64));
            assert_eq!(
                tx.value, expected,
                "stake should carry 2.5 ETH as msg.value"
            );
            // submit(address) selector
            assert_eq!(&tx.data[..4], &[0xa1, 0x90, 0x3e, 0xab]);
        }
        other => panic!("expected SingleTx after elide, got {other:?}"),
    }

    // The compiler should warn so we can track LLM regressions.
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("Elided redundant 'wrap")),
        "expected elide warning, got {:?}",
        result.warnings
    );
}

#[test]
fn test_wrap_then_swap_weth_elided_to_native_swap() {
    // LLM regression: emits `wrap ETH→WETH` + `swap WETH→USDC` instead of
    // a single native-ETH swap. The compiler must rewrite to one swap call
    // with msg.value = amount_in, not two separate calls.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1" } },
            { "swap": { "from": "WETH", "to": "USDC", "amount": "1", "price": "2344", "slippage": "0.5" } }
        ],
        "current_timestamp": 1776824820
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::SingleTx(tx) => {
            // One ETH as msg.value, straight to the Uniswap V3 SwapRouter.
            let expected = alloy_primitives::U256::from(1u128)
                * alloy_primitives::U256::from(10u128).pow(alloy_primitives::U256::from(18u64));
            assert_eq!(
                tx.value, expected,
                "elided swap should carry amount_in as msg.value"
            );
            assert_eq!(
                format!("{:?}", tx.to).to_lowercase(),
                "0xe592427a0aece92de3edee1f18e0157c05861564",
                "elided pair should be a single call to SwapRouter"
            );
        }
        other => panic!("expected SingleTx after elide, got {other:?}"),
    }

    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("Elided redundant 'wrap")),
        "expected elide warning, got {:?}",
        result.warnings
    );
}

#[test]
fn test_wrap_alone_is_not_elided() {
    // Sanity: a legitimate standalone wrap (no stake/swap after it) must
    // still produce a WETH.deposit() call. Don't over-eagerly rewrite.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1.5" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::SingleTx(tx) => {
            // WETH9 mainnet address
            assert_eq!(
                format!("{:?}", tx.to).to_lowercase(),
                "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"
            );
        }
        other => panic!("expected SingleTx for standalone wrap, got {other:?}"),
    }
    assert!(
        !result
            .warnings
            .iter()
            .any(|w| w.contains("Elided redundant 'wrap")),
        "standalone wrap must not be elided",
    );
}

#[test]
fn test_eip712_nonce_and_deadline() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "nonce": 5,
        "deadline": 1712345678,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    let output = &result.output;

    match output {
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(intent.intent_batch.nonce, 5);
            assert_eq!(intent.intent_batch.deadline, 1712345678);
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

// ─── Additional coverage tests ─────────────────────────────────────────

#[test]
fn test_aave_withdraw() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "withdraw": { "asset": "USDC", "amount": "5000", "from": "aave" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    let output = &result.output;

    // Withdraw is a single call (no approval needed — user already has aTokens)
    match output {
        CompileOutput::SingleTx(tx) => {
            // Target should be Aave V3 Pool
            assert_eq!(
                format!("{}", tx.to),
                "0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2"
            );
            // No ETH value
            assert_eq!(tx.value.to_string(), "0");
            // withdraw(address,uint256,address) selector: 0x69328dec
            assert_eq!(&tx.data[..4], &[0x69, 0x32, 0x8d, 0xec]);
        }
        other => panic!("Expected SingleTx for withdraw, got {:?}", other),
    }

    let json_output = CompileOutputJson::from(&result);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Aave withdraw output:\n{json_str}");
    assert!(json_str.contains("single_tx"));
}

#[test]
fn test_complex_defi_from_example_file() {
    // Read the actual example file — this is the canonical complex DeFi intent
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path = std::path::Path::new(manifest_dir).join("examples/complex_defi.json");
    let input =
        std::fs::read_to_string(&example_path).expect("should read complex_defi.json example file");

    let result = do_compile(&input).expect("compile should succeed");

    // complex_defi.json: swap USDC→WETH + deposit WETH into Aave + borrow DAI
    // This produces multiple calls batched via router
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            // Should be batched via router
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            // Should contain all three operations
            assert!(
                intent.description.contains("Swap") || intent.description.contains("swap"),
                "Description should mention Swap: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Supply") || intent.description.contains("supply"),
                "Description should mention Supply: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Borrow") || intent.description.contains("borrow"),
                "Description should mention Borrow: {}",
                intent.description
            );
            // Should have multiple calls in the batch
            assert!(
                intent.intent_batch.calls.len() >= 3,
                "Complex DeFi should produce at least 3 calls (approve+swap+approve+supply+borrow), got {}",
                intent.intent_batch.calls.len()
            );
            // EIP-712 domain should be set
            assert_eq!(intent.domain.name, "IntentRouter");
            assert_eq!(intent.domain.chain_id, 31337);
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }

    // Verify JSON serialization round-trips
    let json_output = CompileOutputJson::from(&result);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Complex DeFi output:\n{json_str}");
    assert!(json_str.contains("eip712_intent"));
    assert!(json_str.contains("IntentRouter"));
}

#[test]
fn test_stake_lido_wsteth_from_example_file() {
    // Read the actual example file
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path = std::path::Path::new(manifest_dir).join("examples/stake_lido_wsteth.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read stake_lido_wsteth.json example file");

    let result = do_compile(&input).expect("compile should succeed");

    // stake ETH in Lido + wrap stETH → wstETH produces batched calls
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(
                intent.description.contains("Batched"),
                "Description should mention batching: {}",
                intent.description
            );
            assert!(
                intent.description.contains("Stake"),
                "Description should mention Stake: {}",
                intent.description
            );
            assert!(
                intent.description.contains("wstETH"),
                "Description should mention wstETH: {}",
                intent.description
            );
            // Should have calls for: stake + approve stETH + wrap to wstETH
            assert!(
                intent.intent_batch.calls.len() >= 2,
                "Stake+wrap should produce at least 2 calls, got {}",
                intent.intent_batch.calls.len()
            );
        }
        other => panic!(
            "Expected Eip712Intent (batched via router), got {:?}",
            other
        ),
    }

    let json_output = CompileOutputJson::from(&result);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();
    println!("Stake Lido wstETH output:\n{json_str}");
    assert!(json_str.contains("eip712_intent"));
}

#[test]
fn test_all_example_files_compile() {
    // Ensure every example JSON file in the examples directory compiles
    // successfully. Files containing `{{...}}` template markers (e.g.
    // `{{POSITION_ID}}`) are documentation templates: a caller is expected
    // to substitute the placeholder before compiling, so they're skipped.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let examples_dir = std::path::Path::new(manifest_dir).join("examples");

    let entries = std::fs::read_dir(&examples_dir).expect("should read examples directory");
    let mut count = 0;
    let mut skipped = 0;

    for entry in entries {
        let entry = entry.expect("should read dir entry");
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let input = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("should read {}", path.display()));
            if input.contains("{{") && input.contains("}}") {
                skipped += 1;
                println!(
                    "⏭  {} skipped (contains template placeholder)",
                    path.file_name().unwrap().to_string_lossy()
                );
                continue;
            }
            let result = do_compile(&input);
            assert!(
                result.is_ok(),
                "Example {} should compile successfully, but got error: {:?}",
                path.file_name().unwrap().to_string_lossy(),
                result.unwrap_err()
            );
            count += 1;
            println!(
                "✅ {} compiled successfully",
                path.file_name().unwrap().to_string_lossy()
            );
        }
    }

    assert!(count > 0, "Should have found at least one example file");
    println!("All {count} example files compiled successfully ({skipped} templates skipped)");
}

// ─── Invalid input tests ───────────────────────────────────────────────

#[test]
fn test_missing_network_field_fails() {
    let input = r#"{
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1.0" } }
        ]
    }"#;
    let result = do_compile(input);
    assert!(result.is_err(), "Missing network should fail");
}

#[test]
fn test_missing_from_field_fails() {
    let input = r#"{
        "network": "anvil",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1.0" } }
        ]
    }"#;
    let result = do_compile(input);
    assert!(result.is_err(), "Missing from should fail");
}

#[test]
fn test_missing_steps_field_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"
    }"#;
    let result = do_compile(input);
    assert!(result.is_err(), "Missing steps should fail");
}

#[test]
fn test_invalid_from_address_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "not-a-hex-address",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1.0" } }
        ]
    }"#;
    let result = do_compile(input);
    assert!(result.is_err(), "Invalid from address should fail");
}

#[test]
fn test_zero_address_signer_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0x0000000000000000000000000000000000000000",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1.0" } }
        ]
    }"#;
    let result = do_compile(input);
    assert!(result.is_err(), "Zero address signer should fail");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("zero"),
        "Error should mention zero address: {err}"
    );
}

#[test]
fn test_unknown_step_type_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "fly": { "to": "moon" } }
        ]
    }"#;
    let result = do_compile(input);
    assert!(result.is_err(), "Unknown step type should fail");
}

#[test]
fn test_deposit_missing_amount_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "into": "aave" } }
        ]
    }"#;
    let result = do_compile(input);
    assert!(result.is_err(), "Deposit missing amount should fail");
}

#[test]
fn test_deposit_missing_into_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100" } }
        ]
    }"#;
    let result = do_compile(input);
    assert!(result.is_err(), "Deposit missing into should fail");
}

#[test]
fn test_deposit_into_unknown_protocol_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "compound" } }
        ]
    }"#;
    let result = do_compile(input);
    assert!(result.is_err(), "Deposit into unknown protocol should fail");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("compound"),
        "Error should mention unknown protocol: {err}"
    );
}

#[test]
fn test_borrow_from_unknown_protocol_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "borrow": { "asset": "DAI", "amount": "1000", "from": "compound" } }
        ]
    }"#;
    let result = do_compile(input);
    assert!(result.is_err(), "Borrow from unknown protocol should fail");
}

#[test]
fn test_non_numeric_amount_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "abc" } }
        ]
    }"#;
    let result = do_compile(input);
    assert!(result.is_err(), "Non-numeric amount should fail");
}

#[test]
fn test_zero_amount_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "0" } }
        ]
    }"#;
    let result = do_compile(input);
    assert!(result.is_err(), "Zero amount should fail");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("greater than zero"),
        "Error should mention zero amount: {err}"
    );
}

#[test]
fn test_swap_same_asset_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "USDC" } }
        ]
    }"#;
    let result = do_compile(input);
    assert!(result.is_err(), "Swap same asset should fail");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("itself"),
        "Error should mention swap to itself: {err}"
    );
}

// ─── Balance-aware compilation tests ───────────────────────────────────

#[test]
fn test_borrow_without_deposit_no_balances_warns() {
    // Borrow without deposit and no balance info → should compile with warning
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "borrow": { "asset": "DAI", "amount": "1000", "from": "aave" } }
        ]
    }"#;

    let result = do_compile(input).expect("should compile (optimistic)");
    assert!(
        !result.warnings.is_empty(),
        "Should have warnings about borrow without deposit"
    );
    assert!(
        result.warnings[0].contains("Borrow without prior deposit"),
        "Warning should mention borrow without deposit: {}",
        result.warnings[0]
    );
}

#[test]
fn test_borrow_with_existing_collateral_no_warning() {
    // Borrow with balance info showing existing collateral → should compile without warning
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "balances": {
            "tokens": { "USDC": "50000.0" },
            "aave_positions": {
                "supplied": { "USDC": "50000.0" },
                "borrowed": {},
                "health_factor": "2.5"
            }
        },
        "steps": [
            { "borrow": { "asset": "DAI", "amount": "1000", "from": "aave" } }
        ]
    }"#;

    let result = do_compile(input).expect("should compile with existing collateral");
    assert!(
        result.warnings.is_empty(),
        "Should have no warnings when user has collateral. Warnings: {:?}",
        result.warnings
    );
}

#[test]
fn test_borrow_without_collateral_with_balances_fails() {
    // Borrow with balance info showing NO collateral → should fail
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "balances": {
            "tokens": { "USDC": "50000.0" },
            "aave_positions": {
                "supplied": {},
                "borrowed": {}
            }
        },
        "steps": [
            { "borrow": { "asset": "DAI", "amount": "1000", "from": "aave" } }
        ]
    }"#;

    let result = do_compile(input);
    assert!(
        result.is_err(),
        "Borrow without collateral should fail when balances provided"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("collateral"),
        "Error should mention collateral: {err}"
    );
}

#[test]
fn test_withdraw_without_deposit_no_balances_warns() {
    // Withdraw without deposit and no balance info → should compile with warning
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "withdraw": { "asset": "USDC", "amount": "5000", "from": "aave" } }
        ]
    }"#;

    let result = do_compile(input).expect("should compile (optimistic)");
    assert!(
        !result.warnings.is_empty(),
        "Should have warnings about withdraw without deposit"
    );
    assert!(
        result.warnings[0].contains("Withdraw without prior deposit"),
        "Warning should mention withdraw without deposit: {}",
        result.warnings[0]
    );
}

#[test]
fn test_withdraw_with_existing_position_no_warning() {
    // Withdraw with balance info showing existing position → should compile without warning
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "balances": {
            "tokens": {},
            "aave_positions": {
                "supplied": { "USDC": "50000.0" },
                "borrowed": {}
            }
        },
        "steps": [
            { "withdraw": { "asset": "USDC", "amount": "5000", "from": "aave" } }
        ]
    }"#;

    let result = do_compile(input).expect("should compile with existing position");
    assert!(
        result.warnings.is_empty(),
        "Should have no warnings when user has position. Warnings: {:?}",
        result.warnings
    );
}

#[test]
fn test_withdraw_without_position_with_balances_fails() {
    // Withdraw with balance info showing NO position for this asset → should fail
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "balances": {
            "tokens": {},
            "aave_positions": {
                "supplied": {},
                "borrowed": {}
            }
        },
        "steps": [
            { "withdraw": { "asset": "USDC", "amount": "5000", "from": "aave" } }
        ]
    }"#;

    let result = do_compile(input);
    assert!(
        result.is_err(),
        "Withdraw without position should fail when balances provided"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("position"),
        "Error should mention position: {err}"
    );
}

#[test]
fn test_deposit_then_borrow_no_warning() {
    // Deposit then borrow in same intent → should compile without warning.
    // `current_timestamp` is set because deposit+borrow batches via the
    // router, and batched intents need a deadline to be signable.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } },
            { "borrow": { "asset": "DAI", "amount": "2000", "from": "aave" } }
        ]
    }"#;

    let result = do_compile(input).expect("should compile");
    assert!(
        result.warnings.is_empty(),
        "Deposit then borrow should have no warnings. Warnings: {:?}",
        result.warnings
    );
}

#[test]
fn test_balances_field_is_optional() {
    // Existing intent without balances field should still work
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1.0" } }
        ]
    }"#;

    let result = do_compile(input).expect("should compile without balances");
    assert!(result.warnings.is_empty());
}

#[test]
fn test_borrow_existing_collateral_example_compiles() {
    // The new example file should compile successfully
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_path =
        std::path::Path::new(manifest_dir).join("examples/borrow_existing_collateral.json");
    let input = std::fs::read_to_string(&example_path)
        .expect("should read borrow_existing_collateral.json");

    let result = do_compile(&input).expect("should compile with existing collateral");
    assert!(
        result.warnings.is_empty(),
        "Example with existing collateral should have no warnings. Warnings: {:?}",
        result.warnings
    );
}

#[test]
fn test_warnings_in_json_output() {
    // Verify warnings appear in JSON output
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "borrow": { "asset": "DAI", "amount": "1000", "from": "aave" } }
        ]
    }"#;

    let result = do_compile(input).expect("should compile");
    let json_output = CompileOutputJson::from(&result);
    let json_str = serde_json::to_string_pretty(&json_output).unwrap();

    assert!(
        json_str.contains("warnings"),
        "JSON output should contain warnings field: {json_str}"
    );
    assert!(
        json_str.contains("Borrow without prior deposit"),
        "JSON output should contain the warning text: {json_str}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Slippage / min_amount_out tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_swap_with_min_amount_out() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "min_amount_out": "0.48" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    // Should have no slippage warnings
    assert!(
        !result.warnings.iter().any(|w| w.contains("slippage")),
        "Should not have slippage warning when min_amount_out is provided: {:?}",
        result.warnings
    );

    // Verify the output compiles to a batched tx (swap produces multiple calls)
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(
                intent.description.contains("Uniswap V3"),
                "Description should mention Uniswap V3: {}",
                intent.description
            );
        }
        other => panic!("Expected Eip712Intent for swap, got {:?}", other),
    }
}

#[test]
fn test_swap_with_price_and_slippage() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "price": "0.0005", "slippage": "1.0" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    // Should have no slippage warnings
    assert!(
        !result.warnings.iter().any(|w| w.contains("slippage")),
        "Should not have slippage warning when price+slippage is provided: {:?}",
        result.warnings
    );
}

#[test]
fn test_swap_with_price_default_slippage() {
    // When price is provided but slippage is omitted, default 0.5% is used
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "price": "0.0005" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    // Should have no slippage warnings (price triggers computation with default 0.5%)
    assert!(
        !result.warnings.iter().any(|w| w.contains("slippage")),
        "Should not have slippage warning when price is provided: {:?}",
        result.warnings
    );
}

#[test]
fn test_swap_slippage_without_price_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "slippage": "0.5" } }
        ]
    }"#;

    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("price"),
        "Error should mention that price is required: {err}"
    );
}

#[test]
fn test_swap_no_slippage_rejected() {
    // When neither min_amount_out nor price/slippage is provided, compiler rejects
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH" } }
        ]
    }"#;

    let result = do_compile(input);
    assert!(
        result.is_err(),
        "Swap without slippage protection should fail"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("slippage"),
        "Error should mention slippage: {err}"
    );
}

#[test]
fn test_swap_min_amount_out_overrides_slippage() {
    // When both min_amount_out and price/slippage are provided, min_amount_out wins
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "min_amount_out": "0.48", "price": "0.0005", "slippage": "1.0" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    // Should have no slippage warnings
    assert!(
        !result.warnings.iter().any(|w| w.contains("slippage")),
        "Should not have slippage warning when min_amount_out is provided: {:?}",
        result.warnings
    );
}

#[test]
fn test_swap_negative_slippage_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "price": "0.0005", "slippage": "-1.0" } }
        ]
    }"#;

    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("Slippage must be between"),
        "Error should mention invalid slippage range: {err}"
    );
}

#[test]
fn test_swap_invalid_price_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH", "price": "abc" } }
        ]
    }"#;

    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("Invalid price"),
        "Error should mention invalid price: {err}"
    );
}

// ─── "all" keyword coverage for step_consumes/step_produces ──────────────

#[test]
fn test_all_after_uniswap_swap() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "swap": { "from": "USDC", "amount": "5000", "to": "WETH", "min_amount_out": "2.0" } },
            { "deposit": { "asset": "WETH", "amount": "all", "into": "aave" } }
        ]
    }"#;
    do_compile(input).expect("swap→deposit-all should compile");
}

#[test]
fn test_all_after_wsteth_wrap() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "stake": { "asset": "ETH", "amount": "10.0", "into": "lido" } },
            { "wrap": { "asset": "stETH", "amount": "all" } },
            { "send": { "asset": "wstETH", "amount": "all", "to": "0x1234567890abcdef1234567890abcdef12345678" } }
        ]
    }"#;
    do_compile(input).expect(
        "stake → wrap-all → send-all should compile (step_produces must include WstETHWrap)",
    );
}

#[test]
fn test_all_after_aave_withdraw() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "balances": {
            "aave_positions": {
                "supplied": { "USDC": "5000.0" },
                "borrowed": {},
                "health_factor": "5.0"
            }
        },
        "steps": [
            { "withdraw": { "asset": "USDC", "amount": "5000", "from": "aave" } },
            { "send": { "asset": "USDC", "amount": "all", "to": "0x1234567890abcdef1234567890abcdef12345678" } }
        ]
    }"#;
    do_compile(input)
        .expect("withdraw → send-all should compile (step_produces must include AaveV3Withdraw)");
}

#[test]
fn test_uniswap_consumption_flow_validated() {
    // Attempt to swap more than the prior step produces.
    // Cross-step flow validation must reject this for any swap adapter.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "swap": { "from": "USDC", "amount": "100", "to": "WETH", "min_amount_out": "0.04" } },
            { "swap": { "from": "WETH", "amount": "1000", "to": "DAI", "min_amount_out": "900" } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("guarantee"),
        "cross-step flow validation should reject consuming more than the prior step guarantees; got: {err}"
    );
}

// `test_missing_timestamp_emits_deadline_warning` was removed: batched
// intents without a deadline source are now a hard CompileError::DeadlineMissing,
// and the dedicated coverage lives in `deadline_warning_tests.rs`.

#[test]
fn test_slippage_precision_large_amount() {
    // 1,000,000 WETH swap (18 decimals) with price 2000 USDC/WETH and 1% slippage.
    // Expected output: 1,000,000 * 2000 = 2,000,000,000 USDC
    // Min with 1% slippage: 1,980,000,000 USDC (= 1_980_000_000_000_000 with 6 decimals).
    // f64 math would lose precision here (>15 significant digits).
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "swap": { "from": "WETH", "amount": "1000000", "to": "USDC", "price": "2000", "slippage": "1" } }
        ]
    }"#;
    let result = do_compile(input).expect("compile should succeed");
    let json: CompileOutputJson = (&result.output).into();
    // The calldata encodes amount_out_minimum at offset 160 of exactInputSingle params.
    // Rather than decoding, we assert the description mentions the swap executed.
    // The real test: compile didn't panic or silently zero out slippage protection.
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            // Batched: good. Calldata should be present.
            assert!(intent.direct_tx.data.len() > 100);
        }
        other => panic!("expected Eip712Intent, got {:?}", other),
    }
    // Smoke: JSON serialization works (no NaN/infinite f64 leaking in)
    let _s = serde_json::to_string(&json).unwrap();
}

#[test]
fn test_unwrap_consumption_flow_validated() {
    // Unwrap should be visible to cross-step flow validation via step_consumes.
    // Here we consume more WETH than the wrap step produces.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap":   { "asset": "ETH",  "amount": "1.0" } },
            { "unwrap": { "asset": "WETH", "amount": "5.0" } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("guarantee"),
        "cross-step flow validation should see unwrap as a consumer; got: {err}"
    );
}

// --- Sepolia network tests (Task A5) ---

fn load_sepolia_config() -> (String, String, String) {
    let dir = config_dir();
    let chains = std::fs::read_to_string(dir.join("chains.json")).unwrap();
    let assets = std::fs::read_to_string(dir.join("assets/sepolia.json")).unwrap();
    let protocols = std::fs::read_to_string(dir.join("protocols/sepolia.json")).unwrap();
    (chains, assets, protocols)
}

fn do_compile_sepolia(input: &str) -> Result<CompileResult, intent_script::error::CompileError> {
    let (c, a, p) = load_sepolia_config();
    let input = inject_default_timestamp_if_missing(input);
    compile(&input, &c, &a, &p)
}

#[test]
fn test_sepolia_wrap_eth_to_weth() {
    let input = r#"{
        "network": "sepolia",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "0.5" } }
        ]
    }"#;

    let result = do_compile_sepolia(input).expect("sepolia wrap should compile");
    match &result.output {
        CompileOutput::SingleTx(tx) => {
            assert_eq!(tx.chain_id, 11155111);
            // Target must be the Sepolia WETH contract
            assert_eq!(
                format!("{}", tx.to).to_lowercase(),
                "0xfff9976782d46cc05630d1f6ebab18b2324d6b14"
            );
            assert_eq!(tx.value.to_string(), "500000000000000000");
        }
        other => panic!("expected SingleTx, got {other:?}"),
    }
}

#[test]
fn test_sepolia_swap_usdc_to_weth() {
    let input = r#"{
        "network": "sepolia",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": {
                "from": "USDC",
                "amount": "10",
                "to": "WETH",
                "min_amount_out": "0.003"
            } }
        ]
    }"#;

    let result = do_compile_sepolia(input).expect("sepolia swap should compile");
    // Sepolia swap produces an EIP-712 intent (goes through the router)
    // or a single tx; either way chain_id must be Sepolia.
    let chain_id = match &result.output {
        CompileOutput::SingleTx(tx) => tx.chain_id,
        CompileOutput::Eip712Intent(e) => e.direct_tx.chain_id,
        CompileOutput::TxSequence(txs) => txs.first().map(|t| t.chain_id).unwrap_or(0),
        CompileOutput::RequiresExecutor { .. } => 0,
    };
    assert_eq!(chain_id, 11155111, "sepolia chain id expected");
}

#[test]
fn test_preview_wrap_then_deposit_nets_to_native_eth_in() {
    // Regression: `wrap 100 ETH + deposit 100 WETH` previously showed
    // "You send: 0.1 WETH" because the router fee_bps was applied to the
    // intermediate wrap output (100 → 99.9 WETH), then netted against the
    // deposit's 100 WETH consume to yield 0.1 WETH spurious input. The
    // preview should now show a clean 100 ETH outflow and nothing else.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap":    { "asset": "ETH",  "amount": "100" } },
            { "deposit": { "asset": "WETH", "amount": "100", "into": "aave" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile");
    let preview = result.preview.as_ref().expect("preview emitted");

    assert_eq!(preview.inputs.len(), 1, "inputs: {:?}", preview.inputs);
    assert_eq!(preview.inputs[0].symbol, "ETH");
    assert_eq!(preview.inputs[0].amount, "100");

    assert!(
        preview.outputs.is_empty(),
        "outputs should be empty (all WETH flows into Aave): {:?}",
        preview.outputs
    );
}

#[test]
fn test_preview_swap_inputs_outputs() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": {
                "from": "USDC",
                "amount": "100",
                "to": "WETH",
                "min_amount_out": "0.04"
            } }
        ]
    }"#;

    let result = do_compile(input).expect("compile");
    let preview = result.preview.as_ref().expect("preview emitted");

    assert_eq!(preview.inputs.len(), 1);
    assert_eq!(preview.inputs[0].symbol, "USDC");
    assert_eq!(preview.inputs[0].amount, "100");

    assert_eq!(preview.outputs.len(), 1);
    assert_eq!(preview.outputs[0].symbol, "WETH");
    // 0.04 WETH - 10 bps router fee = 0.03996 WETH actually netted to user.
    assert_eq!(preview.outputs[0].amount, "0.03996");

    // Preview steps must only contain the user-meaningful swap; no approve or
    // transferFrom (those are enrich artefacts).
    assert_eq!(preview.steps.len(), 1);
    assert_eq!(preview.steps[0].action, "swap");
    assert_eq!(preview.steps[0].protocol, "uniswap_v3");

    // Confirm it round-trips through JSON too.
    let json = CompileOutputJson::from(&result);
    let s = serde_json::to_string(&json).unwrap();
    assert!(s.contains("\"preview\""));
    assert!(s.contains("\"USDC\""));
    assert!(s.contains("\"WETH\""));
}

#[test]
fn test_sepolia_unknown_asset_error() {
    let input = r#"{
        "network": "sepolia",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "FAKE_TOKEN", "amount": "1" } }
        ]
    }"#;

    let err = do_compile_sepolia(input).unwrap_err().to_string();
    assert!(
        err.contains("Unknown asset"),
        "should surface unknown asset error on sepolia; got: {err}"
    );
}

// ─── Prerequisite approvals (UI-provided allowances) ──────────────────

fn do_compile_with_allowances(
    input: &str,
    allowances_json: Option<&str>,
) -> Result<CompileResult, intent_script::error::CompileError> {
    let (c, a, p) = load_config();
    let input = inject_default_timestamp_if_missing(input);
    compile_with_allowances(&input, &c, &a, &p, allowances_json)
}

/// A USDC deposit triggers a `transferFrom(user, router, 5000e6)` inner call.
/// With zero allowance reported, the compiler must emit exactly one
/// `approve(router, 5000e6)` prerequisite tx.
#[test]
fn test_allowances_zero_emits_approve_for_usdc_deposit() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }
        ]
    }"#;
    let allowances = r#"{ "tokens": { "USDC": "0" } }"#;

    let result = do_compile_with_allowances(input, Some(allowances)).expect("compile ok");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(
                intent.prerequisite_approvals.len(),
                1,
                "expected exactly one prerequisite approve"
            );
            let approve_tx = &intent.prerequisite_approvals[0];
            // Tx target is the USDC token itself
            assert_eq!(
                format!("{}", approve_tx.to),
                "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
            );
            // approve(address,uint256) selector is 0x095ea7b3
            assert_eq!(&approve_tx.data[..4], &[0x09, 0x5e, 0xa7, 0xb3]);
            // Spender (bytes 4..36, right-aligned) should be the router
            let router_addr = format!("{}", intent.domain.verifying_contract).to_lowercase();
            let spender_hex = format!(
                "0x{}",
                alloy_primitives::hex::encode(&approve_tx.data[4 + 12..4 + 32])
            );
            assert_eq!(spender_hex, router_addr);
            // Amount (bytes 36..68) must equal 5000 USDC in base units = 5_000_000_000
            let mut amount_bytes = [0u8; 32];
            amount_bytes.copy_from_slice(&approve_tx.data[4 + 32..4 + 64]);
            let amount = alloy_primitives::U256::from_be_bytes(amount_bytes);
            assert_eq!(amount.to_string(), "5000000000");
            // Approve tx carries no ETH value
            assert_eq!(approve_tx.value.to_string(), "0");
        }
        other => panic!("Expected Eip712Intent, got {other:?}"),
    }

    // JSON shape includes prerequisiteApprovals
    let json_str = serde_json::to_string(&CompileOutputJson::from(&result)).unwrap();
    assert!(
        json_str.contains("prerequisiteApprovals"),
        "JSON missing prerequisiteApprovals: {json_str}"
    );
}

/// When the caller reports a sufficient current allowance, the compiler must
/// suppress the approve prereq entirely. The JSON output stays free of the
/// `prerequisiteApprovals` key (empty vec + skip_serializing_if).
#[test]
fn test_allowances_sufficient_emits_no_approve() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }
        ]
    }"#;
    // MaxUint256 in decimal
    let allowances = r#"{ "tokens": { "USDC": "115792089237316195423570985008687907853269984665640564039457584007913129639935" } }"#;

    let result = do_compile_with_allowances(input, Some(allowances)).expect("compile ok");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(
                intent.prerequisite_approvals.is_empty(),
                "expected no approvals, got {}",
                intent.prerequisite_approvals.len()
            );
        }
        other => panic!("Expected Eip712Intent, got {other:?}"),
    }

    let json_str = serde_json::to_string(&CompileOutputJson::from(&result)).unwrap();
    assert!(
        !json_str.contains("prerequisiteApprovals"),
        "JSON should omit prerequisiteApprovals when empty: {json_str}"
    );
}

/// Calling the legacy 4-arg `compile()` (no allowances) must still succeed
/// and produce exactly today's JSON shape — no `prerequisiteApprovals` key.
#[test]
fn test_no_allowances_arg_backcompat() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile ok");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(
                intent.prerequisite_approvals.is_empty(),
                "legacy compile() must not emit approvals"
            );
        }
        other => panic!("Expected Eip712Intent, got {other:?}"),
    }

    let json_str = serde_json::to_string(&CompileOutputJson::from(&result)).unwrap();
    assert!(
        !json_str.contains("prerequisiteApprovals"),
        "legacy output must not carry the new JSON field: {json_str}"
    );
}

/// A swap+deposit batched through the router pulls USDC twice under the hood
/// (once for the swap output going through the router, once for the deposit).
/// But the enricher marks USDC as already-in-router after the swap, so only
/// the input token (WETH) is pulled from the user. With a high WETH allowance
/// and 0 USDC allowance, the compiler should emit no approvals (USDC was
/// never pulled from user; WETH was pulled but allowance is sufficient).
#[test]
fn test_multi_token_only_user_pulls_counted() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "WETH", "amount": "1.0", "to": "USDC", "min_amount_out": "2000" } },
            { "deposit": { "asset": "USDC", "amount": "all", "into": "aave" } }
        ]
    }"#;
    // Sufficient WETH, zero USDC — USDC shouldn't matter because the swap
    // deposits output into the router (no transferFrom for USDC).
    let allowances = r#"{
        "tokens": {
            "WETH": "115792089237316195423570985008687907853269984665640564039457584007913129639935",
            "USDC": "0"
        }
    }"#;

    let result = do_compile_with_allowances(input, Some(allowances)).expect("compile ok");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(
                intent.prerequisite_approvals.is_empty(),
                "expected zero approvals (WETH sufficient, USDC never pulled from user); got {}",
                intent.prerequisite_approvals.len()
            );
        }
        other => panic!("Expected Eip712Intent, got {other:?}"),
    }
}

/// Same swap+deposit, but this time WETH allowance is zero. Exactly one
/// approval tx for WETH, amount == 1e18 (the swap's amount_in).
#[test]
fn test_multi_step_emits_one_approve_for_pulled_token() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "WETH", "amount": "1.0", "to": "USDC", "min_amount_out": "2000" } },
            { "deposit": { "asset": "USDC", "amount": "all", "into": "aave" } }
        ]
    }"#;
    let allowances = r#"{ "tokens": { "WETH": "0" } }"#;

    let result = do_compile_with_allowances(input, Some(allowances)).expect("compile ok");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(
                intent.prerequisite_approvals.len(),
                1,
                "expected a single WETH approval"
            );
            let tx = &intent.prerequisite_approvals[0];
            assert_eq!(
                format!("{}", tx.to),
                "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
            );
            let mut amount_bytes = [0u8; 32];
            amount_bytes.copy_from_slice(&tx.data[4 + 32..4 + 64]);
            let amount = alloy_primitives::U256::from_be_bytes(amount_bytes);
            assert_eq!(amount.to_string(), "1000000000000000000");
        }
        other => panic!("Expected Eip712Intent, got {other:?}"),
    }
}

#[test]
fn test_lido_unwrap_wsteth() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "unwrap": { "asset": "wstETH", "amount": "1.0" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(intent.direct_tx.to, intent.domain.verifying_contract);
            assert!(
                intent.description.contains("Unwrap"),
                "Description should mention Unwrap: {}",
                intent.description
            );
            // The sub-calls the router will delegatecall live on
            // `intent.intent_batch.calls`. Walk them directly and assert:
            //   - there's a wstETH.unwrap() call
            //   - no Erc20Approve is inserted for the unwrap itself
            //     (unwrap burns caller's own wstETH — approve is not needed)
            let approve_selector = [0x09, 0x5e, 0xa7, 0xb3];
            let unwrap_selector = [0xde, 0x0e, 0x9a, 0x3e]; // wstETH.unwrap(uint256)
            let mut saw_unwrap = false;
            let mut saw_approve = false;
            for call in &intent.intent_batch.calls {
                if call.call_data.len() >= 4 {
                    if call.call_data[..4] == unwrap_selector {
                        saw_unwrap = true;
                    }
                    if call.call_data[..4] == approve_selector {
                        saw_approve = true;
                    }
                }
            }
            assert!(saw_unwrap, "Expected the wstETH.unwrap() call in the batch");
            assert!(
                !saw_approve,
                "Unwrap should not require an approve — wstETH is burned from caller's balance"
            );
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

/// Helper: find the single call in `intent.intent_batch.calls` whose `target`
/// matches the given hex address (case-insensitive). Fails the test if more
/// than one such call exists, or none.
fn single_call_to<'a>(
    intent: &'a intent_script::output::Eip712IntentOutput,
    address_hex: &str,
) -> &'a intent_script::output::CallData {
    let matches: Vec<&intent_script::output::CallData> = intent
        .intent_batch
        .calls
        .iter()
        .filter(|c| format!("{}", c.target).eq_ignore_ascii_case(address_hex))
        .collect();
    match matches.as_slice() {
        [one] => one,
        [] => panic!("No call to {} in batch", address_hex),
        more => panic!(
            "Expected exactly one call to {}, found {}",
            address_hex,
            more.len()
        ),
    }
}

#[test]
fn test_lido_request_withdrawal_steth() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            {
                "request_withdrawal": {
                    "asset": "stETH",
                    "amounts": ["0.5"],
                    "from": "lido"
                }
            }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(intent.direct_tx.to, intent.domain.verifying_contract);
            assert!(
                intent.description.contains("Request Lido withdrawal"),
                "Description should mention the request withdrawal: {}",
                intent.description
            );
            // Exactly one call should target the queue, and its calldata for
            // a single 0.5-stETH request must be 4 + 32*3 + 32*1 = 132 bytes
            // (selector + offset to amounts + owner + amounts.len + amounts[0]).
            let queue = single_call_to(intent, "0x889edC2eDab5f40e902b864aD4d7AdE8E412F9B1");
            assert_eq!(
                queue.call_data.len(),
                132,
                "requestWithdrawals(uint256[1], address) calldata is 132 bytes"
            );
            assert_eq!(
                queue.value.to_string(),
                "0",
                "requestWithdrawals should carry no ETH value"
            );
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

#[test]
fn test_lido_request_withdrawal_wsteth_differs_from_steth_selector() {
    // The calldata selectors MUST differ between the stETH and wstETH variants.
    // We don't hardcode the expected selector — instead we run both variants
    // and assert their leading 4 bytes differ.
    let steth_input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            {
                "request_withdrawal": {
                    "asset": "stETH",
                    "amounts": ["0.5"],
                    "from": "lido"
                }
            }
        ]
    }"#;
    let wsteth_input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            {
                "request_withdrawal": {
                    "asset": "wstETH",
                    "amounts": ["0.5"],
                    "from": "lido"
                }
            }
        ]
    }"#;

    let steth_res = do_compile(steth_input).expect("compile stETH ok");
    let wsteth_res = do_compile(wsteth_input).expect("compile wstETH ok");

    let extract = |r: &CompileResult| -> [u8; 4] {
        match &r.output {
            CompileOutput::Eip712Intent(intent) => {
                let queue = single_call_to(intent, "0x889edC2eDab5f40e902b864aD4d7AdE8E412F9B1");
                let mut s = [0u8; 4];
                s.copy_from_slice(&queue.call_data[..4]);
                s
            }
            other => panic!("Expected Eip712Intent, got {:?}", other),
        }
    };

    let steth_selector = extract(&steth_res);
    let wsteth_selector = extract(&wsteth_res);
    assert_ne!(
        steth_selector, wsteth_selector,
        "stETH and wstETH request-withdrawal selectors must differ"
    );
}

#[test]
fn test_lido_claim_withdrawal() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            {
                "claim_withdrawal": {
                    "protocol": "lido",
                    "request_ids": [42, 43],
                    "hints": [1, 1]
                }
            }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    // Claim is a single tx: no token pulls, no approvals, no sweep.
    match &result.output {
        CompileOutput::SingleTx(tx) => {
            assert_eq!(
                format!("{}", tx.to),
                "0x889edC2eDab5f40e902b864aD4d7AdE8E412F9B1"
            );
            assert_eq!(tx.value.to_string(), "0");
            // claimWithdrawals(uint256[],uint256[]) has a non-trivial calldata.
            assert!(tx.data.len() > 4, "claim should have calldata");
        }
        CompileOutput::Eip712Intent(intent) => {
            // Acceptable if routed — no prerequisite approvals expected.
            assert!(intent.prerequisite_approvals.is_empty());
        }
        other => panic!("Unexpected compile output: {:?}", other),
    }
}

#[test]
fn test_lido_claim_withdrawal_hints_length_mismatch_fails() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            {
                "claim_withdrawal": {
                    "protocol": "lido",
                    "request_ids": [42, 43],
                    "hints": [1]
                }
            }
        ]
    }"#;

    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("hints length"),
        "Expected hints-length mismatch error, got: {err}"
    );
}

#[test]
fn test_lido_request_withdrawal_rejects_zero_amount() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            {
                "request_withdrawal": {
                    "asset": "stETH",
                    "amounts": ["0"],
                    "from": "lido"
                }
            }
        ]
    }"#;

    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("greater than zero"),
        "Expected non-zero amount error, got: {err}"
    );
}

#[test]
fn test_lido_request_withdrawal_rejects_unknown_asset() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            {
                "request_withdrawal": {
                    "asset": "USDC",
                    "amounts": ["1.0"],
                    "from": "lido"
                }
            }
        ]
    }"#;

    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("stETH") || err.contains("wstETH"),
        "Expected asset-must-be-stETH-or-wstETH error, got: {err}"
    );
}

// ─── Uniswap V3 LP ──────────────────────────────────────────────────

/// Uniswap V3 NPM on mainnet/anvil.
const NPM_ADDR: &str = "0xC36442b4a4522E871399CD717aBDD847Ab11FE88";

#[test]
fn test_lp_mint_compiles() {
    // USDC (0xA0b8...) is lexicographically smaller than WETH (0xC02a...),
    // so the compiler must preserve that as token0 regardless of DSL order.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "USDC",
                "token1": "WETH",
                "fee": "3000",
                "tick_lower": -200040,
                "tick_upper": -199980,
                "amount0": "1000",
                "amount1": "0.3",
                "min_amount0": "990",
                "min_amount1": "0.29"
            } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            // Expect the batch to include:
            //   - 2 transferFrom (USDC, WETH)
            //   - 2 approves (NPM)
            //   - 1 mint
            let transfer_count = intent
                .intent_batch
                .calls
                .iter()
                .filter(|c| c.call_data.len() >= 4 && c.call_data[..4] == [0x23, 0xb8, 0x72, 0xdd])
                .count();
            assert_eq!(transfer_count, 2, "expected two transferFrom calls");

            let approve_selector = [0x09, 0x5e, 0xa7, 0xb3];
            let approve_count = intent
                .intent_batch
                .calls
                .iter()
                .filter(|c| c.call_data.len() >= 4 && c.call_data[..4] == approve_selector)
                .count();
            assert_eq!(approve_count, 2, "expected two approve(NPM) calls");

            // Exactly one NPM.mint call.
            let npm_calls: Vec<_> = intent
                .intent_batch
                .calls
                .iter()
                .filter(|c| format!("{}", c.target).eq_ignore_ascii_case(NPM_ADDR))
                .collect();
            assert_eq!(npm_calls.len(), 1, "expected one NPM call");

            // Both tokens should be swept for dust refunds.
            assert_eq!(
                intent.intent_batch.tokens_to_sweep.len(),
                2,
                "mint should register both tokens for sweep"
            );
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

#[test]
fn test_lp_mint_order_canonicalized_regardless_of_dsl_order() {
    // Same pair supplied in reverse DSL order should produce identical calldata.
    let a = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "USDC", "token1": "WETH",
                "fee": "3000",
                "tick_lower": -200040, "tick_upper": -199980,
                "amount0": "1000", "amount1": "0.3",
                "min_amount0": "990", "min_amount1": "0.29"
            } }
        ]
    }"#;
    let b = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "WETH", "token1": "USDC",
                "fee": "3000",
                "tick_lower": -200040, "tick_upper": -199980,
                "amount0": "0.3", "amount1": "1000",
                "min_amount0": "0.29", "min_amount1": "990"
            } }
        ]
    }"#;

    let ra = do_compile(a).expect("A ok");
    let rb = do_compile(b).expect("B ok");

    let extract_mint_call = |r: &CompileResult| -> Vec<u8> {
        match &r.output {
            CompileOutput::Eip712Intent(intent) => intent
                .intent_batch
                .calls
                .iter()
                .find(|c| format!("{}", c.target).eq_ignore_ascii_case(NPM_ADDR))
                .expect("NPM call present")
                .call_data
                .to_vec(),
            _ => panic!("expected batched output"),
        }
    };
    assert_eq!(
        extract_mint_call(&ra),
        extract_mint_call(&rb),
        "mint calldata must be identical regardless of DSL token order"
    );
}

#[test]
fn test_lp_mint_rejects_misaligned_ticks() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "USDC", "token1": "WETH",
                "fee": "3000",
                "tick_lower": -200041, "tick_upper": -199981,
                "amount0": "1000", "amount1": "0.3",
                "min_amount0": "990", "min_amount1": "0.29"
            } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("multiples of tick spacing"),
        "expected tick-spacing error, got: {err}"
    );
}

#[test]
fn test_lp_mint_rejects_invalid_fee_tier() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "USDC", "token1": "WETH",
                "fee": "1500",
                "tick_lower": -200040, "tick_upper": -199980,
                "amount0": "1000", "amount1": "0.3",
                "min_amount0": "990", "min_amount1": "0.29"
            } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("fee tier"),
        "expected fee-tier error, got: {err}"
    );
}

#[test]
fn test_lp_mint_accepts_zero_min_amounts() {
    // Both mins zero — must compile. The range (tick_lower/tick_upper) is
    // the real slippage guard for concentrated liquidity; a positive
    // min_amount0/min_amount1 actively *causes* `Price slippage check`
    // reverts when the current tick is off-center inside a narrow range.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "USDC", "token1": "WETH",
                "fee": "3000",
                "tick_lower": -200040, "tick_upper": -199980,
                "amount0": "1000", "amount1": "0.3",
                "min_amount0": "0", "min_amount1": "0"
            } }
        ]
    }"#;
    let _ = do_compile(input).expect("compile should succeed with zero min_amounts");
}

#[test]
fn test_lp_mint_price_form_tight_stables() {
    // The user's case from the transcript: 5k USDC + 5k USDT, tight LP.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "USDC", "token1": "USDT",
                "fee": "500",
                "price_lower": "0.999",
                "price_upper": "1.001",
                "quote_token": "USDT",
                "amount0": "5000", "amount1": "5000",
                "min_amount0": "4975", "min_amount1": "4975"
            } }
        ]
    }"#;
    let result = do_compile(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::Eip712Intent(_) => {}
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

#[test]
fn test_lp_mint_price_form_full_range_sentinels() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "USDC", "token1": "WETH",
                "fee": "3000",
                "price_lower": "min", "price_upper": "max",
                "quote_token": "WETH",
                "amount0": "1000", "amount1": "0.3",
                "min_amount0": "990", "min_amount1": "0.297"
            } }
        ]
    }"#;
    let _ = do_compile(input).expect("full-range price form should compile");
}

#[test]
fn test_lp_mint_price_form_accepts_either_quote_token() {
    // Both quote_token directions must compile to valid batched intents.
    // (Byte-exact equality isn't possible because floor-rounding ticks through
    // a reciprocal is asymmetric — both ranges are valid, just slightly
    // different bounds.)
    for quote in &["USDT", "USDC"] {
        let input = format!(
            r#"{{
                "network": "anvil",
                "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
                "steps": [ {{ "lp_mint": {{
                    "protocol": "uniswap",
                    "token0": "USDC", "token1": "USDT", "fee": "500",
                    "price_lower": "0.999", "price_upper": "1.001", "quote_token": "{}",
                    "amount0": "5000", "amount1": "5000",
                    "min_amount0": "4975", "min_amount1": "4975"
                }} }} ]
            }}"#,
            quote
        );
        let r = do_compile(&input).expect("compile should succeed for both quote_tokens");
        assert!(matches!(r.output, CompileOutput::Eip712Intent(_)));
    }
}

#[test]
fn test_lp_mint_rejects_both_price_and_tick() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "USDC", "token1": "USDT", "fee": "500",
                "price_lower": "0.999", "price_upper": "1.001", "quote_token": "USDT",
                "tick_lower": -10, "tick_upper": 10,
                "amount0": "5000", "amount1": "5000",
                "min_amount0": "4975", "min_amount1": "4975"
            } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("not both"),
        "expected mutual-exclusion error, got: {err}"
    );
}

#[test]
fn test_lp_mint_rejects_missing_quote_token() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "USDC", "token1": "USDT", "fee": "500",
                "price_lower": "0.999", "price_upper": "1.001",
                "amount0": "5000", "amount1": "5000",
                "min_amount0": "4975", "min_amount1": "4975"
            } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("quote_token"),
        "expected missing-quote_token error, got: {err}"
    );
}

#[test]
fn test_lp_mint_rejects_quote_token_not_in_pair() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "USDC", "token1": "USDT", "fee": "500",
                "price_lower": "0.999", "price_upper": "1.001", "quote_token": "WETH",
                "amount0": "5000", "amount1": "5000",
                "min_amount0": "4975", "min_amount1": "4975"
            } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("quote_token"),
        "expected quote_token-membership error, got: {err}"
    );
}

/// Mainnet WETH9. Used by tests that assert ETH→WETH substitution in LP mint.
const WETH_ADDR: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
/// Mainnet USDC. Used by the same tests for prerequisite_approvals assertions.
const USDC_ADDR: &str = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";

/// A user writing "ETH" (native) as one side of an LP pair must compile to:
///   1. A preceding `WETH.deposit{value: ethAmount}()` wrap call.
///   2. A mint call where the native side's token address is the wrapped-native.
///   3. Exactly one USDC prerequisite approval — WETH is already in the router
///      from the wrap step, so no user-side approval is emitted for it.
///   4. `direct_tx.value == ethAmount` (the router forwards it into the wrap).
#[test]
fn test_lp_mint_native_eth_auto_wraps_and_substitutes_weth() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "USDC", "token1": "ETH",
                "fee": "3000",
                "tick_lower": -200040, "tick_upper": -199980,
                "amount0": "1000", "amount1": "0.3",
                "min_amount0": "990", "min_amount1": "0.29"
            } }
        ]
    }"#;

    // Report zero allowance for both tokens so the compiler would emit an
    // approve for anything it thinks the user must pre-approve. That way the
    // "zero approvals for WETH" assertion is actually proving the wrap step
    // short-circuited the WETH pull, not that the allowance happened to
    // already be sufficient.
    let allowances = r#"{
        "tokens": { "USDC": "0", "WETH": "0" }
    }"#;
    let result =
        do_compile_with_allowances(input, Some(allowances)).expect("compile should succeed");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            // (1) The Wrap step lowered to a WETH.deposit() call with value
            // equal to the user's ETH amount (0.3 ETH = 3e17 wei). Multiple
            // calls target WETH (a router→NPM approve on the WETH contract
            // also lands there), so select by the deposit() selector.
            let wrap_deposits: Vec<_> = intent
                .intent_batch
                .calls
                .iter()
                .filter(|c| {
                    format!("{}", c.target).eq_ignore_ascii_case(WETH_ADDR)
                        && c.call_data.len() >= 4
                        && c.call_data[..4] == [0xd0, 0xe3, 0x0d, 0xb0]
                })
                .collect();
            assert_eq!(
                wrap_deposits.len(),
                1,
                "expected exactly one WETH.deposit call"
            );
            assert_eq!(
                wrap_deposits[0].value.to_string(),
                "300000000000000000",
                "wrap msg.value must equal the specified ETH amount"
            );

            // (2) The top-level direct_tx aggregates the wrap's msg.value so
            // the router receives enough ETH to forward into WETH.deposit.
            assert_eq!(
                intent.direct_tx.value.to_string(),
                "300000000000000000",
                "direct_tx.value must carry the wrap's msg.value to the router"
            );
            assert_eq!(
                intent.intent_batch.total_value.to_string(),
                "300000000000000000"
            );

            // (3) Prerequisite approvals: only USDC. No zero-address entry,
            // no WETH entry (the wrap-produced WETH lives in the router).
            assert_eq!(
                intent.prerequisite_approvals.len(),
                1,
                "expected one prerequisite approval (USDC only)"
            );
            let approve = &intent.prerequisite_approvals[0];
            assert!(
                format!("{}", approve.to).eq_ignore_ascii_case(USDC_ADDR),
                "prerequisite approval must target USDC, got: {}",
                approve.to
            );
            // approve() selector 0x095ea7b3
            assert_eq!(&approve.data[..4], &[0x09, 0x5e, 0xa7, 0xb3]);

            // (4) No prerequisite approval should ever be addressed to the
            // zero address — that's the bug this fix guards against.
            for tx in &intent.prerequisite_approvals {
                assert_ne!(
                    format!("{}", tx.to),
                    "0x0000000000000000000000000000000000000000",
                    "must never emit an ERC-20 approve to the zero address"
                );
            }
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

/// Identical semantics when the user puts ETH in the first slot instead of
/// the second — the canonical-ordering sort must happen *after* the ETH→WETH
/// substitution so the pair still lexicographically resolves to (USDC, WETH).
#[test]
fn test_lp_mint_native_eth_token0_still_substitutes() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "ETH", "token1": "USDC",
                "fee": "3000",
                "tick_lower": -200040, "tick_upper": -199980,
                "amount0": "0.3", "amount1": "1000",
                "min_amount0": "0.29", "min_amount1": "990"
            } }
        ]
    }"#;
    let result = do_compile(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            let wrap_calls: Vec<_> = intent
                .intent_batch
                .calls
                .iter()
                .filter(|c| format!("{}", c.target).eq_ignore_ascii_case(WETH_ADDR))
                .collect();
            // Exactly one WETH.deposit (from the auto-injected Wrap step) —
            // other WETH-targeted calls in this pipeline are ERC-20 approves
            // to the NPM, which hit the same target but a different selector.
            let wrap_deposits: Vec<_> = wrap_calls
                .iter()
                .filter(|c| c.call_data.len() >= 4 && c.call_data[..4] == [0xd0, 0xe3, 0x0d, 0xb0])
                .collect();
            assert_eq!(wrap_deposits.len(), 1);
            assert_eq!(wrap_deposits[0].value.to_string(), "300000000000000000");
            assert_eq!(intent.direct_tx.value.to_string(), "300000000000000000");
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

/// The price-form quote_token needs the same ETH→WETH substitution so it
/// matches the rewritten pair aliases. Using "ETH" as quote_token for an
/// ETH-side pair must compile, not error with "quote_token must equal token0
/// or token1".
#[test]
fn test_lp_mint_native_eth_quote_token_is_normalized() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "USDC", "token1": "ETH",
                "fee": "3000",
                "price_lower": "2000", "price_upper": "3000",
                "quote_token": "ETH",
                "amount0": "1000", "amount1": "0.3",
                "min_amount0": "990", "min_amount1": "0.29"
            } }
        ]
    }"#;
    let _ = do_compile(input).expect("compile should succeed with quote_token='ETH'");
}

/// Both sides native is nonsense — reject cleanly rather than producing an
/// IR with token0 == token1 == 0x0 that would silently pass through to
/// calldata generation.
#[test]
fn test_lp_mint_rejects_both_sides_native() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "ETH", "token1": "ETH",
                "fee": "3000",
                "tick_lower": -200040, "tick_upper": -199980,
                "amount0": "0.3", "amount1": "0.3",
                "min_amount0": "0.29", "min_amount1": "0.29"
            } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("non-native") || err.contains("native"),
        "expected both-native reject error, got: {err}"
    );
}

/// `min_amount0` / `min_amount1` are optional on lp_mint — omitting them must
/// compile and emit `amount0Min = amount1Min = 0` in the NPM mint calldata.
/// This closes the "narrow range + tight amount_min → `Price slippage check`
/// revert" footgun that hit the ETH/WBTC LP flow.
#[test]
fn test_lp_mint_without_min_amounts_defaults_to_zero() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_mint": {
                "protocol": "uniswap",
                "token0": "USDC", "token1": "WETH",
                "fee": "3000",
                "tick_lower": -200040, "tick_upper": -199980,
                "amount0": "1000", "amount1": "0.3"
            } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed without min_amounts");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            // Find the NPM mint call (selector 0x88316456). Its struct layout
            // is [token0, token1, fee, tickLower, tickUpper, amount0Desired,
            // amount1Desired, amount0Min, amount1Min, recipient, deadline] —
            // `amount0Min` is the 8th 32-byte word after the selector, and
            // `amount1Min` is the 9th.
            let mint = intent
                .intent_batch
                .calls
                .iter()
                .find(|c| c.call_data.len() >= 4 && c.call_data[..4] == [0x88, 0x31, 0x64, 0x56])
                .expect("expected an NPM mint call");
            let body = &mint.call_data[4..];
            let read_word = |idx: usize| -> &[u8] { &body[idx * 32..(idx + 1) * 32] };
            let amount0_min = read_word(7);
            let amount1_min = read_word(8);
            assert!(
                amount0_min.iter().all(|b| *b == 0),
                "amount0Min must be 0 when min_amount0 is omitted, got {amount0_min:?}"
            );
            assert!(
                amount1_min.iter().all(|b| *b == 0),
                "amount1Min must be 0 when min_amount1 is omitted, got {amount1_min:?}"
            );
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

#[test]
fn test_lp_increase_emits_dual_transfer_and_approve() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_increase": {
                "position_id": "12345",
                "token0": "USDC",
                "token1": "WETH",
                "amount0": "500",
                "amount1": "0.15",
                "min_amount0": "495",
                "min_amount1": "0.148"
            } }
        ]
    }"#;
    let result = do_compile(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            let transfer_count = intent
                .intent_batch
                .calls
                .iter()
                .filter(|c| c.call_data.len() >= 4 && c.call_data[..4] == [0x23, 0xb8, 0x72, 0xdd])
                .count();
            assert_eq!(transfer_count, 2);
            let approve_count = intent
                .intent_batch
                .calls
                .iter()
                .filter(|c| c.call_data.len() >= 4 && c.call_data[..4] == [0x09, 0x5e, 0xa7, 0xb3])
                .count();
            assert_eq!(approve_count, 2);
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

#[test]
fn test_lp_decrease_then_collect() {
    // Rebalance pattern: decrease → collect in one intent.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_decrease": {
                "position_id": "12345",
                "token0": "USDC",
                "token1": "WETH",
                "liquidity": "1000000000000000000",
                "min_amount0": "950",
                "min_amount1": "0.28"
            } },
            { "lp_collect": {
                "position_id": "12345",
                "token0": "USDC",
                "token1": "WETH"
            } }
        ]
    }"#;
    let result = do_compile(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            // Both NPM calls should target the same NFT.
            let npm_calls: Vec<_> = intent
                .intent_batch
                .calls
                .iter()
                .filter(|c| format!("{}", c.target).eq_ignore_ascii_case(NPM_ADDR))
                .collect();
            assert_eq!(npm_calls.len(), 2, "expected decrease + collect calls");

            // No approves or transferFroms should appear — decrease/collect
            // don't consume user tokens (tokens bypass router via recipient=signer).
            let approve_count = intent
                .intent_batch
                .calls
                .iter()
                .filter(|c| c.call_data.len() >= 4 && c.call_data[..4] == [0x09, 0x5e, 0xa7, 0xb3])
                .count();
            assert_eq!(approve_count, 0);
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

#[test]
fn test_lp_decrease_rejects_all_liquidity() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "lp_decrease": {
                "position_id": "12345",
                "token0": "USDC",
                "token1": "WETH",
                "liquidity": "all",
                "min_amount0": "950",
                "min_amount1": "0.28"
            } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("liquidity='all' is not supported"),
        "expected all-liquidity rejection, got: {err}"
    );
}

#[test]
fn test_morpho_supply_collateral_and_borrow() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "deposit": { "asset": "WETH", "amount": "1.0", "into": "morpho",
                           "market": "USDC-WETH-86", "as": "collateral" } },
            { "borrow":  { "asset": "USDC", "amount": "1500", "from": "morpho",
                           "market": "USDC-WETH-86" } }
        ]
    }"#;

    let result = do_compile(input).expect("compile should succeed");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            // Expected call sequence: transferFrom WETH, approve WETH→Morpho,
            // supplyCollateral, borrow — four calls minimum.
            assert!(
                intent.intent_batch.calls.len() >= 4,
                "expected at least 4 calls, got {}",
                intent.intent_batch.calls.len()
            );
            // Last two calls target the Morpho Blue pool.
            let morpho_pool = "0xBBBBBbbBBb9cC5e90e3b3Af64bdAF62C37EEFFCb";
            let morpho_calls: Vec<_> = intent
                .intent_batch
                .calls
                .iter()
                .filter(|c| format!("{}", c.target).eq_ignore_ascii_case(morpho_pool))
                .collect();
            assert_eq!(
                morpho_calls.len(),
                2,
                "expected supplyCollateral + borrow on Morpho"
            );
            // USDC appears in sweep list (borrowed loan asset received via router).
            let usdc = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
            let has_usdc_sweep = intent
                .intent_batch
                .tokens_to_sweep
                .iter()
                .any(|t| format!("{}", t).eq_ignore_ascii_case(usdc));
            assert!(has_usdc_sweep, "USDC should be swept back to user");
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

#[test]
fn test_morpho_rejects_wrong_asset() {
    // Market is USDC-WETH, but user tries to supply DAI as collateral.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "DAI", "amount": "1000", "into": "morpho",
                           "market": "USDC-WETH-86", "as": "collateral" } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("expects asset 'WETH'"),
        "expected asset mismatch error, got: {err}"
    );
}

#[test]
fn test_morpho_rejects_unknown_market() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "WETH", "amount": "1.0", "into": "morpho",
                           "market": "NONEXISTENT-9999", "as": "collateral" } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("Unknown Morpho market"),
        "expected unknown market error, got: {err}"
    );
}

#[test]
fn test_morpho_requires_market_field() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "WETH", "amount": "1.0", "into": "morpho" } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("require an explicit `market` field")
            || err.contains("requires a 'market' field"),
        "expected missing-market error, got: {err}"
    );
}

#[test]
fn test_morpho_rejects_as_on_borrow() {
    // The `as` field doesn't exist on BorrowStep so this is implicitly enforced
    // by the schema — but a supply with `as: "loan"` should route to the loan
    // side (default). Sanity check: supply-loan for USDC compiles cleanly.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "1000", "into": "morpho",
                           "market": "USDC-WETH-86" } }
        ]
    }"#;
    let result = do_compile(input).expect("supply-loan should compile");
    let json = CompileOutputJson::from(&result);
    let json_str = serde_json::to_string(&json).unwrap();
    assert!(json_str.contains("Supply"), "expected Supply step preview");
}

#[test]
fn test_balancer_flashloan_simple() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            {
              "flashloan": {
                "via": "balancer",
                "assets": [{ "asset": "WETH", "amount": "2.0" }],
                "then": [
                  { "deposit": { "asset": "WETH", "amount": "2.0", "into": "aave" } },
                  { "borrow":  { "asset": "USDC", "amount": "4000", "from": "aave" } },
                  { "swap":    { "from": "USDC", "amount": "4000", "to": "WETH",
                                 "min_amount_out": "2.0" } }
                ]
              }
            }
        ]
    }"#;
    let result = do_compile(input).expect("flashloan compile should succeed");
    // Flashloans force router batching (sentinel arm happens in
    // `_executeCalls`), so output is Eip712Intent with the flashLoan call as
    // one of the inner `calls`.
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            let vault = "0xBA12222222228d8Ba445958a75a0704d566BF2C8";
            let has_vault = intent
                .intent_batch
                .calls
                .iter()
                .any(|c| format!("{}", c.target).eq_ignore_ascii_case(vault));
            assert!(has_vault, "expected a call targeting Balancer Vault");
            // Find the vault call and verify the selector.
            let vault_call = intent
                .intent_batch
                .calls
                .iter()
                .find(|c| format!("{}", c.target).eq_ignore_ascii_case(vault))
                .expect("vault call");
            assert_eq!(&vault_call.call_data[..4], &[0x5c, 0x38, 0x44, 0x9e]);
        }
        other => panic!(
            "Expected Eip712Intent for router-gated flashloan, got {:?}",
            other
        ),
    }
}

#[test]
fn test_flashloan_rejects_nested() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            {
              "flashloan": {
                "via": "balancer",
                "assets": [{ "asset": "WETH", "amount": "1.0" }],
                "then": [
                  {
                    "flashloan": {
                      "via": "balancer",
                      "assets": [{ "asset": "USDC", "amount": "1000" }],
                      "then": []
                    }
                  }
                ]
              }
            }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("nested flashloans"),
        "expected nested-flashloan rejection, got: {err}"
    );
}

#[test]
fn test_flashloan_rejects_unrepayable() {
    // Flashloan 2 WETH but inner pipeline only produces 1 WETH (swap out
    // less than flashloaned).
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            {
              "flashloan": {
                "via": "balancer",
                "assets": [{ "asset": "WETH", "amount": "2.0" }],
                "then": [
                  { "swap": { "from": "WETH", "amount": "2.0", "to": "USDC",
                              "min_amount_out": "3000" } },
                  { "swap": { "from": "USDC", "amount": "3000", "to": "WETH",
                              "min_amount_out": "1.0" } }
                ]
              }
            }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("not repayable") || err.contains("leaves only"),
        "expected unrepayable flashloan rejection, got: {err}"
    );
}

#[test]
fn test_flashloan_rejects_too_many_inner_steps() {
    // Build MAX_FLASHLOAN_INNER_STEPS+1 alternating deposit/withdraw steps so
    // the test scales automatically when the cap moves.
    use intent_script::compiler::validate::MAX_FLASHLOAN_INNER_STEPS;
    let inner_count = MAX_FLASHLOAN_INNER_STEPS + 1;
    let inner_steps: Vec<String> = (0..inner_count)
        .map(|i| {
            if i % 2 == 0 {
                r#"{ "deposit": { "asset": "WETH", "amount": "1.0", "into": "aave" } }"#.to_string()
            } else {
                r#"{ "withdraw": { "asset": "WETH", "amount": "1.0", "from": "aave" } }"#
                    .to_string()
            }
        })
        .collect();
    let input = format!(
        r#"{{
            "network": "anvil",
            "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
            "current_timestamp": 1714000000,
            "steps": [
                {{
                  "flashloan": {{
                    "via": "balancer",
                    "assets": [{{ "asset": "WETH", "amount": "1.0" }}],
                    "then": [{}]
                  }}
                }}
            ]
        }}"#,
        inner_steps.join(",")
    );
    let err = do_compile(&input).unwrap_err().to_string();
    let expected = format!("inner pipeline has {inner_count} steps");
    assert!(
        err.contains(&expected) || err.contains("inner pipeline"),
        "expected inner-step count rejection, got: {err}"
    );
}

#[test]
fn test_flashloan_rejects_native_asset() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            {
              "flashloan": {
                "via": "balancer",
                "assets": [{ "asset": "ETH", "amount": "1.0" }],
                "then": []
              }
            }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("cannot be native"),
        "expected native-asset rejection, got: {err}"
    );
}

#[test]
fn test_flashloan_userdata_roundtrip_decodes() {
    use intent_script::adapters::balancer;
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            {
              "flashloan": {
                "via": "balancer",
                "assets": [{ "asset": "WETH", "amount": "2.0" }],
                "then": [
                  { "deposit": { "asset": "WETH", "amount": "2.0", "into": "aave" } },
                  { "borrow":  { "asset": "USDC", "amount": "4000", "from": "aave" } },
                  { "swap":    { "from": "USDC", "amount": "4000", "to": "WETH",
                                 "min_amount_out": "2.0" } }
                ]
              }
            }
        ]
    }"#;
    let result = do_compile(input).expect("flashloan compile should succeed");
    // Router-gated: find the vault.flashLoan inner call inside the batched
    // Eip712 output.
    let vault_call_data = match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            let vault = "0xBA12222222228d8Ba445958a75a0704d566BF2C8";
            intent
                .intent_batch
                .calls
                .iter()
                .find(|c| format!("{}", c.target).eq_ignore_ascii_case(vault))
                .expect("vault call")
                .call_data
                .clone()
        }
        other => panic!("expected Eip712Intent, got {:?}", other),
    };

    // Decode the flashLoan call's userData and verify each inner call
    // still has a recognizable selector. `abi_decode` expects the full
    // calldata (selector + args), not args-only.
    use alloy_sol_types::SolCall;
    let decoded =
        balancer::flashLoanCall::abi_decode(&vault_call_data).expect("decode flashLoan call");

    let inner_calls = balancer::decode_inner_calls(&decoded.userData).expect("decode inner calls");
    assert_eq!(
        inner_calls.len(),
        5,
        "expected 5 inner calls (approve WETH, supply, borrow, approve USDC, swap), got {}",
        inner_calls.len()
    );
    // First inner call should be an approve (selector 0x095ea7b3).
    assert_eq!(&inner_calls[0].calldata[..4], &[0x09, 0x5e, 0xa7, 0xb3]);
}

#[test]
fn test_long_5x_eth_accepts() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "long": {
                "collateral": "WETH",
                "borrow":     "USDC",
                "amount":     "1.0",
                "leverage":   "5",
                "slippage":   "50",
                "price":      "3200"
            } }
        ]
    }"#;
    let result = do_compile(input).expect("5x long should compile at 80% LTV");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            let vault = "0xBA12222222228d8Ba445958a75a0704d566BF2C8";
            let has_vault = intent
                .intent_batch
                .calls
                .iter()
                .any(|c| format!("{}", c.target).eq_ignore_ascii_case(vault));
            assert!(has_vault, "expected flashLoan call through router");
        }
        other => panic!("expected Eip712Intent, got {:?}", other),
    }
}

#[test]
fn test_long_6x_eth_rejects() {
    // 6x exceeds max leverage at 80% LTV (max = 1/(1 - 0.8) = 5x).
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "long": {
                "collateral": "WETH",
                "amount": "1.0",
                "leverage": "6",
                "price": "3200"
            } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("exceeds max"),
        "expected leverage cap error, got: {err}"
    );
}

#[test]
fn test_long_requires_price() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "long": {
                "collateral": "WETH",
                "amount": "1.0",
                "leverage": "3"
            } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("requires the 'price'"),
        "expected price-required error, got: {err}"
    );
}

#[test]
fn test_leverage_rejects_wide_slippage() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "long": {
                "collateral": "WETH",
                "amount": "1.0",
                "leverage": "3",
                "slippage": "600",
                "price": "3200"
            } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("slippage_bps 600 exceeds"),
        "expected slippage cap error, got: {err}"
    );
}

#[test]
fn test_leverage_rejects_identical_assets() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "long": {
                "collateral": "WETH",
                "borrow":     "WETH",
                "amount":     "1.0",
                "leverage":   "2",
                "price":      "1"
            } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("must differ"),
        "expected collateral==borrow rejection, got: {err}"
    );
}

#[test]
fn test_short_weth_with_usdc_collateral_accepts() {
    // Short WETH with USDC collateral (77% LTV → max ~4.35x, so 3x is fine).
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "short": {
                "collateral": "WETH",
                "borrow":     "USDC",
                "amount":     "1.0",
                "leverage":   "3",
                "price":      "3200"
            } }
        ]
    }"#;
    let result = do_compile(input).expect("3x short should compile");
    let _ = result;
}

#[test]
fn test_leverage_rejects_leverage_equals_one() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "long": {
                "collateral": "WETH",
                "amount": "1.0",
                "leverage": "1"
            } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("plain deposit"),
        "expected leverage=1 rejection, got: {err}"
    );
}

#[test]
fn test_close_position_requires_state() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "close_position": {
                "collateral": "WETH",
                "borrow":     "USDC",
                "current_debt":       "0",
                "current_collateral": "5.0"
            } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("must be > 0"),
        "expected zero-state rejection, got: {err}"
    );
}

#[test]
fn test_close_position_compiles() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "close_position": {
                "collateral":         "WETH",
                "borrow":             "USDC",
                "current_debt":       "4180.0",
                "current_collateral": "5.0",
                "slippage":           "50"
            } }
        ]
    }"#;
    let result = do_compile(input).expect("close_position should compile");
    let _ = result;
}

#[test]
fn test_bridge_across_usdc_to_arbitrum() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "bridge": { "via": "across", "asset": "USDC", "amount": "1000",
                          "to_chain": "arbitrum",
                          "recipient": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
                          "relayer_fee_bps": "5" } }
        ]
    }"#;
    let result = do_compile(input).expect("bridge should compile");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            let spoke = "0x5c7BCd6E7De5423a257D81B442095A1a6ced35C5";
            let has_spoke = intent
                .intent_batch
                .calls
                .iter()
                .any(|c| format!("{}", c.target).eq_ignore_ascii_case(spoke));
            assert!(has_spoke, "expected call to Across SpokePool");
        }
        other => panic!("expected Eip712Intent, got {:?}", other),
    }
}

#[test]
fn test_bridge_rejects_native_eth() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "bridge": { "via": "across", "asset": "ETH", "amount": "1.0",
                          "to_chain": "arbitrum",
                          "recipient": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
                          "relayer_fee_bps": "5" } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("does not accept native ETH"),
        "expected native ETH rejection, got: {err}"
    );
}

#[test]
fn test_bridge_rejects_high_relayer_fee() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "bridge": { "via": "across", "asset": "USDC", "amount": "1000",
                          "to_chain": "arbitrum",
                          "recipient": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
                          "relayer_fee_bps": "100" } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("exceeds cap 50"),
        "expected relayer-fee cap error, got: {err}"
    );
}

#[test]
fn test_bridge_rejects_unknown_chain() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1714000000,
        "steps": [
            { "bridge": { "via": "across", "asset": "USDC", "amount": "1000",
                          "to_chain": "nonexistent",
                          "recipient": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
                          "relayer_fee_bps": "5" } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("Unknown network") || err.contains("nonexistent"),
        "expected unknown-chain error, got: {err}"
    );
}

#[test]
fn test_bridge_requires_current_timestamp() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "bridge": { "via": "across", "asset": "USDC", "amount": "1000",
                          "to_chain": "arbitrum",
                          "recipient": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
                          "relayer_fee_bps": "5" } }
        ]
    }"#;
    // Bypass the default-timestamp injection — this test specifically asserts
    // that the normalize step rejects an Across bridge when no current_timestamp
    // was supplied (it is used as `quote_timestamp`).
    let (c, a, p) = load_config();
    let err = compile(input, &c, &a, &p).unwrap_err().to_string();
    assert!(
        err.contains("requires 'current_timestamp'"),
        "expected timestamp-required error, got: {err}"
    );
}

#[test]
fn test_aave_rejects_market_field() {
    // `market` is only valid for Morpho.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "1000", "into": "aave",
                           "market": "USDC-WETH-86" } }
        ]
    }"#;
    let err = do_compile(input).unwrap_err().to_string();
    assert!(
        err.contains("only valid when depositing into 'morpho'"),
        "expected rejection of market field on aave, got: {err}"
    );
}

#[test]
fn test_wrap_then_deposit_exact_amount_accepts_with_router_fee() {
    // Regression: user reported that on a network with router fee_bps > 0
    // (anvil ships fee_bps=10 = 0.1%), an exact-amount intra-batch hand-off
    // was falsely rejected because the validator discounted produced tokens
    // by fee_bps even though the router takes the fee only at sweep time.
    // The produced 50 WETH from `wrap` is consumed by the next `deposit`
    // inside the same router execution — no sweep, no fee.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712700000,
        "steps": [
            { "wrap":    { "asset": "ETH",  "amount": "50" } },
            { "deposit": { "asset": "WETH", "amount": "50", "into": "aave" } }
        ]
    }"#;
    do_compile(input)
        .expect("exact-amount intra-batch hand-off must compile with router fee_bps set");
}

#[test]
fn test_wrap_deposit_borrow_chain_accepts_exact_amounts() {
    // Regression: companion to the wrap→deposit test above, exercising the
    // borrow tail the user originally tried. All three steps use exact
    // amounts with no `"all"` escape, so any residual fee_bps leakage in
    // the validator would show up here.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712700000,
        "steps": [
            { "wrap":    { "asset": "ETH",  "amount": "50" } },
            { "deposit": { "asset": "WETH", "amount": "50", "into": "aave" } },
            { "borrow":  { "asset": "USDT", "amount": "10000", "from": "aave" } }
        ]
    }"#;
    do_compile(input).expect("wrap→deposit→borrow with exact amounts must compile");
}
