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
    assert!(err.contains("Morpho borrow requires a 'market' field"));
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
