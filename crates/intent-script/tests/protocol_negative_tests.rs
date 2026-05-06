mod common;

use common::compile_anvil;

#[test]
fn depositing_native_eth_into_aave_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "ETH", "amount": "1", "into": "aave" } }
        ]
    }"#;

    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(err.contains("native ETH"));
    assert!(err.contains("WETH"));
}

#[test]
fn swapping_a_token_to_itself_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "swap": { "from": "USDC", "amount": "100", "to": "USDC", "min_amount_out": "100" } }
        ]
    }"#;

    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(err.contains("swap an asset to itself"));
}

#[test]
fn all_on_first_step_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "all", "into": "aave" } }
        ]
    }"#;

    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(err.contains("Cannot use 'all'"));
}

#[test]
fn morpho_borrow_without_market_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "borrow": { "asset": "USDC", "amount": "100", "from": "morpho" } }
        ]
    }"#;

    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(
        err.contains("require an explicit `market` field")
            || err.contains("Morpho borrow requires a 'market' field"),
        "expected missing-market error, got: {err}",
    );
}

#[test]
fn invalid_uniswap_v3_lp_tick_spacing_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            {
                "lp_mint": {
                    "protocol": "uniswap",
                    "token0": "USDC",
                    "token1": "WETH",
                    "fee": "3000",
                    "tick_lower": -887221,
                    "tick_upper": 887220,
                    "amount0": "1000",
                    "amount1": "0.3",
                    "min_amount0": "990",
                    "min_amount1": "0.297"
                }
            }
        ]
    }"#;

    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(err.contains("tick spacing"));
}

// ─── B3: wallet-balance-aware amount flow ────────────────────────────────
//
// When the caller supplies `balances.tokens` the compiler treats those
// amounts as the starting ledger for each wallet-sourced token. A step
// whose consume exceeds that seed (plus anything a prior step produced)
// is rejected at compile time instead of reverting on-chain after the
// router has already taken its sweep fee.

#[test]
fn deposit_exceeds_wallet_balance_when_balances_provided_rejects() {
    // User has 100 USDC, tries to deposit 500 USDC. Without wallet-aware
    // flow validation this would compile fine and revert in the Aave pool.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "balances": {
            "tokens": { "USDC": "100.0" }
        },
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "500", "into": "aave" } }
        ]
    }"#;

    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(
        err.contains("running balance") || err.contains("guarantees"),
        "expected wallet-balance rejection, got: {err}"
    );
}

#[test]
fn sum_of_spends_across_steps_exceeds_wallet_balance_rejects() {
    // Two 60-USDC spends off a 100-USDC wallet — the second step drains
    // more than is left after the first.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "balances": {
            "tokens": { "USDC": "100.0" }
        },
        "steps": [
            { "swap": { "from": "USDC", "amount": "60", "to": "WETH", "min_amount_out": "0.02" } },
            { "deposit": { "asset": "USDC", "amount": "60", "into": "aave" } }
        ]
    }"#;

    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(
        err.contains("running balance") || err.contains("guarantees"),
        "expected wallet-balance rejection, got: {err}"
    );
}

#[test]
fn spend_exactly_equal_to_wallet_balance_ok() {
    // Boundary: exact match must compile.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "balances": {
            "tokens": { "USDC": "100.0" }
        },
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "100", "into": "aave" } }
        ]
    }"#;

    compile_anvil(input).expect("exact-balance spend should compile");
}

#[test]
fn deposit_matches_wallet_balance_exactly_for_all() {
    // `amount: "all"` uses prior-step production, so the wallet seed is
    // untouched by this path — just a sanity check that seeding doesn't
    // over-reject when no wallet consume is involved.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "balances": {
            "tokens": { "USDC": "100.0" }
        },
        "steps": [
            { "swap": { "from": "USDC", "amount": "50", "to": "WETH", "min_amount_out": "0.02" } },
            { "deposit": { "asset": "WETH", "amount": "all", "into": "aave" } }
        ]
    }"#;

    compile_anvil(input).expect("swap→deposit-all with sufficient wallet should compile");
}

#[test]
fn deposit_without_wallet_balances_still_accepted_backcompat() {
    // When the caller doesn't supply `balances`, the compiler trusts the
    // wallet-sourced amount (pre-B3 behavior). Required so UIs that don't
    // bother fetching balances still work.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "999999999", "into": "aave" } }
        ]
    }"#;

    compile_anvil(input).expect("without balances, wallet-sourced consume is not checked");
}

// WS-3D additions: protocol-specific rejections the advisor flow has to
// surface as a clear "no, that's not supported" instead of a generic
// compiler error. Each one asserts that compilation fails (the advisor
// must never present an unsafe intent for signing) and that the failure
// carries text the UI can match on.

#[test]
fn across_bridge_to_unsupported_chain_is_rejected() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "steps": [
            {
                "bridge": {
                    "via": "across",
                    "asset": "USDC",
                    "amount": "1000",
                    "to_chain": "definitely-not-a-chain",
                    "recipient": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
                    "relayer_fee_bps": "5"
                }
            }
        ]
    }"#;

    let err = compile_anvil(input)
        .expect_err("bridging to a fictional chain must reject")
        .to_string()
        .to_lowercase();
    assert!(
        err.contains("chain") || err.contains("destination") || err.contains("not supported"),
        "across-unsupported-chain error must mention the destination, got: {err}",
    );
}

#[test]
fn lp_mint_inverted_tick_range_is_rejected() {
    // tick_lower > tick_upper is structurally invalid for a Uniswap V3
    // position — the planner has to refuse before reaching the position
    // manager. Otherwise the user signs a tx that always reverts.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "current_timestamp": 1712344000,
        "balances": { "tokens": { "USDC": "10000", "WETH": "5" } },
        "steps": [
            {
                "lp_mint": {
                    "protocol": "uniswap",
                    "token0": "USDC",
                    "token1": "ETH",
                    "fee": "3000",
                    "tick_lower": 100,
                    "tick_upper": -100,
                    "amount0": "1000",
                    "amount1": "0.3",
                    "min_amount0": "990",
                    "min_amount1": "0.29"
                }
            }
        ]
    }"#;

    let err = compile_anvil(input)
        .expect_err("inverted tick range must reject")
        .to_string()
        .to_lowercase();
    assert!(
        err.contains("tick") || err.contains("range") || err.contains("price"),
        "inverted-tick rejection must mention tick/range/price, got: {err}",
    );
}

#[test]
fn unsafe_aave_health_factor_rejects_borrow() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "balances": {
            "aave_positions": {
                "supplied": { "USDC": "1000" },
                "health_factor": "1.10"
            }
        },
        "steps": [
            { "borrow": { "asset": "DAI", "amount": "100", "from": "aave" } }
        ]
    }"#;

    let err = compile_anvil(input).unwrap_err().to_string();
    assert!(err.contains("health factor"));
    assert!(err.contains("borrow rejected"));
}
