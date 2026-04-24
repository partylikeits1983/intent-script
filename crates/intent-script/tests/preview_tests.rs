mod common;

use common::compile_anvil;

#[test]
fn preview_wrap_shows_native_input_and_wrapped_output() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap": { "asset": "ETH", "amount": "1.5" } }
        ]
    }"#;

    let result = compile_anvil(input).expect("compile should succeed");
    let preview = result.preview.as_ref().expect("preview emitted");

    // Wrap now registers native ETH as an input so "You send" reflects the
    // user's actual outflow. The wrapped WETH is the produced output; as the
    // surviving output it gets the 10bps router sweep fee applied.
    assert_eq!(preview.inputs.len(), 1);
    assert_eq!(preview.inputs[0].symbol, "ETH");
    assert_eq!(preview.inputs[0].amount, "1.5");
    assert_eq!(preview.outputs.len(), 1);
    assert_eq!(preview.outputs[0].symbol, "WETH");
    assert_eq!(preview.outputs[0].amount, "1.4985");
    assert_eq!(preview.steps.len(), 1);
    assert_eq!(preview.steps[0].action, "wrap");
    assert_eq!(preview.steps[0].protocol, "weth");
    assert!(
        preview.steps[0]
            .description
            .contains("Wrap 1.5 ETH to WETH")
    );
}

#[test]
fn preview_swap_shows_net_send_and_net_receive_only() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "100", "to": "WETH", "min_amount_out": "0.04" } }
        ]
    }"#;

    let result = compile_anvil(input).expect("compile should succeed");
    let preview = result.preview.as_ref().expect("preview emitted");

    assert_eq!(preview.inputs.len(), 1);
    assert_eq!(preview.inputs[0].symbol, "USDC");
    assert_eq!(preview.inputs[0].amount, "100");
    assert_eq!(preview.outputs.len(), 1);
    assert_eq!(preview.outputs[0].symbol, "WETH");
    assert_eq!(preview.outputs[0].amount, "0.03996");
    assert_eq!(preview.steps.len(), 1);
    assert_eq!(preview.steps[0].action, "swap");
    assert_eq!(preview.steps[0].protocol, "uniswap_v3");
}

#[test]
fn preview_swap_then_deposit_omits_intermediate_weth() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "swap": { "from": "USDC", "amount": "5000", "to": "WETH", "min_amount_out": "2.0" } },
            { "deposit": { "asset": "WETH", "amount": "all", "into": "aave" } }
        ]
    }"#;

    let result = compile_anvil(input).expect("compile should succeed");
    let preview = result.preview.as_ref().expect("preview emitted");

    assert_eq!(preview.inputs.len(), 1);
    assert_eq!(preview.inputs[0].symbol, "USDC");
    assert!(
        preview.outputs.is_empty(),
        "intermediate WETH should net out"
    );
    assert_eq!(preview.steps.len(), 2);
    assert_eq!(preview.steps[0].action, "swap");
    assert_eq!(preview.steps[1].action, "deposit");
    assert!(
        preview
            .steps
            .iter()
            .all(|s| !s.description.contains("Approve") && !s.description.contains("transferFrom"))
    );
}

#[test]
fn preview_deposit_then_borrow_shows_input_and_output_assets() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } },
            { "borrow": { "asset": "DAI", "amount": "2000", "from": "aave" } }
        ]
    }"#;

    let result = compile_anvil(input).expect("compile should succeed");
    let preview = result.preview.as_ref().expect("preview emitted");

    assert_eq!(preview.inputs.len(), 1);
    assert_eq!(preview.inputs[0].symbol, "USDC");
    assert_eq!(preview.outputs.len(), 1);
    assert_eq!(preview.outputs[0].symbol, "DAI");
    assert_eq!(preview.outputs[0].amount, "1998");
    assert_eq!(preview.steps.len(), 2);
    assert_eq!(preview.steps[0].protocol, "aave_v3");
    assert_eq!(preview.steps[1].protocol, "aave_v3");
}
