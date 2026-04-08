//! Enricher edge case tests.
//!
//! Tests the enricher's token routing logic by compiling intents
//! and inspecting the output structure.

use std::path::{Path, PathBuf};

use intent_script::{CompileOutput, compile};

fn config_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("config")
}

// ─── Token routing: no duplicate transferFrom ──────────────────────────

#[test]
fn test_swap_then_deposit_no_duplicate_transfer() {
    // Swap USDC→WETH then deposit WETH into Aave.
    // WETH is already in the router from the swap output, so the enricher
    // should NOT insert a transferFrom for WETH.
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "5000", "to": "WETH" } },
            { "deposit": { "asset": "WETH", "amount": "2.0", "into": "aave" } }
        ]
    }"#;

    let result = compile(input, &config_dir()).expect("compile should succeed");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            // The batch should contain:
            // 1. transferFrom USDC (user → router)
            // 2. approve USDC for Uniswap
            // 3. swap USDC → WETH (recipient = router)
            // 4. approve WETH for Aave (NO transferFrom for WETH!)
            // 5. supply WETH to Aave
            //
            // Count how many calls target WETH with transferFrom selector
            let weth = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_lowercase();
            let transfer_from_selector = [0x23, 0xb8, 0x72, 0xdd]; // transferFrom(address,address,uint256)

            let weth_transfers: Vec<_> = intent
                .intent_batch
                .calls
                .iter()
                .filter(|c| {
                    format!("{}", c.target).to_lowercase() == weth
                        && c.call_data.len() >= 4
                        && c.call_data[..4] == transfer_from_selector
                })
                .collect();

            assert_eq!(
                weth_transfers.len(),
                0,
                "WETH should NOT have a transferFrom — it's already in the router from the swap. \
                 Found {} transferFrom calls for WETH",
                weth_transfers.len()
            );

            // But USDC should have a transferFrom
            let usdc = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_lowercase();
            let usdc_transfers: Vec<_> = intent
                .intent_batch
                .calls
                .iter()
                .filter(|c| {
                    format!("{}", c.target).to_lowercase() == usdc
                        && c.call_data.len() >= 4
                        && c.call_data[..4] == transfer_from_selector
                })
                .collect();

            assert_eq!(
                usdc_transfers.len(),
                1,
                "USDC should have exactly 1 transferFrom call"
            );
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

// ─── Multiple borrows: each borrowed asset in sweep ────────────────────

#[test]
fn test_deposit_and_borrow_sweep_tokens() {
    // Deposit USDC, borrow DAI — DAI should be in sweep tokens
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } },
            { "borrow": { "asset": "DAI", "amount": "2000", "from": "aave" } }
        ]
    }"#;

    let result = compile(input, &config_dir()).expect("compile should succeed");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            let dai = "0x6B175474E89094C44Da98b954EedeAC495271d0F".to_lowercase();
            let sweep_addrs: Vec<String> = intent
                .intent_batch
                .tokens_to_sweep
                .iter()
                .map(|a| format!("{}", a).to_lowercase())
                .collect();

            assert!(
                sweep_addrs.contains(&dai),
                "DAI should be in tokensToSweep (borrowed asset needs sweeping). \
                 Sweep tokens: {:?}",
                sweep_addrs
            );
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}

// ─── Single-step intents produce SingleTx ──────────────────────────────

#[test]
fn test_single_wrap_produces_single_tx() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1.0" } }
        ]
    }"#;

    let result = compile(input, &config_dir()).expect("compile should succeed");

    match &result.output {
        CompileOutput::SingleTx(_) => {
            // Correct — single wrap should be a SingleTx, not batched
        }
        other => panic!("Expected SingleTx for single wrap, got {:?}", other),
    }
}

#[test]
fn test_single_stake_produces_single_tx() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "stake": { "asset": "ETH", "amount": "1.0", "into": "lido" } }
        ]
    }"#;

    let result = compile(input, &config_dir()).expect("compile should succeed");

    match &result.output {
        CompileOutput::SingleTx(_) => {
            // Correct — single stake should be a SingleTx
        }
        other => panic!("Expected SingleTx for single stake, got {:?}", other),
    }
}

#[test]
fn test_single_unwrap_produces_single_tx() {
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "unwrap": { "asset": "WETH", "amount": "1.0" } }
        ]
    }"#;

    let result = compile(input, &config_dir()).expect("compile should succeed");

    match &result.output {
        CompileOutput::SingleTx(_) => {
            // Correct — single unwrap should be a SingleTx
        }
        other => panic!("Expected SingleTx for single unwrap, got {:?}", other),
    }
}

// ─── Swap output token in sweep ────────────────────────────────────────

#[test]
fn test_swap_output_token_in_sweep() {
    // Swap USDC→WETH — WETH should be in sweep tokens
    let input = r#"{
        "network": "ethereum",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "1000", "to": "WETH" } }
        ]
    }"#;

    let result = compile(input, &config_dir()).expect("compile should succeed");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            let weth = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_lowercase();
            let sweep_addrs: Vec<String> = intent
                .intent_batch
                .tokens_to_sweep
                .iter()
                .map(|a| format!("{}", a).to_lowercase())
                .collect();

            assert!(
                sweep_addrs.contains(&weth),
                "WETH should be in tokensToSweep (swap output stays in router). \
                 Sweep tokens: {:?}",
                sweep_addrs
            );
        }
        other => panic!("Expected Eip712Intent, got {:?}", other),
    }
}
