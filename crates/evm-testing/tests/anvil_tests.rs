//! Integration tests that compile intent-script JSON and submit the resulting
//! transactions to a local Anvil instance forking Ethereum mainnet.
//!
//! These tests use Anvil's built-in accounts and cheatcodes to set balances.
//! No whale addresses are used.
//!
//! Run with:
//!   cargo test -p evm-testing -- --nocapture
//!
//! Override the RPC URL:
//!   ETH_RPC_URL=https://your-rpc-url cargo test -p evm-testing -- --nocapture

use alloy::node_bindings::Anvil;
use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol;
use alloy_primitives::{Address, B256, U256, address, keccak256};

use evm_testing::helpers::{compile_intent, extract_txs, to_alloy_tx};

sol! {
    #[sol(rpc)]
    interface IWETH {
        function balanceOf(address owner) external view returns (uint256);
        function deposit() external payable;
    }

    #[sol(rpc)]
    interface IERC20 {
        function balanceOf(address owner) external view returns (uint256);
        function allowance(address owner, address spender) external view returns (uint256);
    }
}

/// Get the fork RPC URL from the ETH_RPC_URL environment variable.
fn fork_url() -> String {
    std::env::var("ETH_RPC_URL")
        .unwrap_or_else(|_| "https://ethereum-rpc.publicnode.com".to_string())
}

/// Compute the storage slot for a mapping(address => uint256) at a given base slot.
/// Solidity stores mapping values at keccak256(abi.encode(key, slot)).
#[allow(dead_code)]
fn mapping_slot(key: Address, base_slot: U256) -> B256 {
    let mut buf = [0u8; 64];
    buf[12..32].copy_from_slice(key.as_slice()); // left-pad address to 32 bytes
    base_slot
        .to_be_bytes::<32>()
        .iter()
        .enumerate()
        .for_each(|(i, b)| buf[32 + i] = *b);
    keccak256(buf)
}

/// Set an ERC-20 token balance for an address using anvil_setStorageAt.
/// This works for standard ERC-20 tokens where balanceOf is at a known storage slot.
#[allow(dead_code)]
async fn set_erc20_balance<P: Provider>(
    provider: &P,
    token: Address,
    holder: Address,
    amount: U256,
    balance_slot: U256, // The base storage slot for the balanceOf mapping
) -> eyre::Result<()> {
    let slot = mapping_slot(holder, balance_slot);
    let mut value = [0u8; 32];
    amount
        .to_be_bytes::<32>()
        .iter()
        .enumerate()
        .for_each(|(i, b)| value[i] = *b);
    let _: bool = provider
        .raw_request(
            "anvil_setStorageAt".into(),
            (
                format!("{token}"),
                format!("{slot}"),
                format!("0x{}", hex::encode(&value)),
            ),
        )
        .await?;
    Ok(())
}

#[allow(dead_code)]
mod hex {
    pub fn encode(data: &[u8]) -> String {
        data.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Test: Wrap ETH → WETH on a local Anvil instance forking mainnet.
#[tokio::test]
async fn test_wrap_eth_on_anvil() -> eyre::Result<()> {
    // Pin the forked Anvil's chain_id to 31337 (what `config/chains.json`'s
    // "anvil" entry declares) so compiled txs — which carry `chain_id: 31337`
    // — aren't rejected by the node's signer-chain-id check.
    let anvil = Anvil::new().fork(fork_url()).chain_id(31337).try_spawn()?;

    let provider = ProviderBuilder::new().connect_http(anvil.endpoint_url());

    let signer = anvil.addresses()[0];
    let weth_addr = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");

    let weth = IWETH::new(weth_addr, &provider);
    let initial_balance = weth.balanceOf(signer).call().await?;
    println!("Initial WETH balance: {initial_balance}");

    // Compile the wrap intent
    let intent_json = format!(
        r#"{{
            "network": "anvil",
            "from": "{}",
            "steps": [
                {{ "wrap": {{ "asset": "ETH", "amount": "1.5" }} }}
            ]
        }}"#,
        signer
    );

    let result = compile_intent(&intent_json).expect("compile should succeed");
    let txs = extract_txs(&result.output);
    assert_eq!(txs.len(), 1, "Wrap should produce exactly 1 tx");

    let receipt = provider
        .send_transaction(to_alloy_tx(txs[0]))
        .await?
        .get_receipt()
        .await?;
    assert!(receipt.status(), "Wrap tx should succeed");

    let final_balance = weth.balanceOf(signer).call().await?;
    let expected_increase = U256::from(1_500_000_000_000_000_000u64);
    let actual_increase = final_balance - initial_balance;
    assert_eq!(actual_increase, expected_increase);

    println!("✅ Wrap ETH→WETH succeeded! WETH balance: {initial_balance} → {final_balance}");
    Ok(())
}

/// Test: Unwrap WETH → ETH on a local Anvil instance.
///
/// First wraps ETH via deposit() to get WETH (which properly updates all
/// contract state), then unwraps via the compiled intent.
///
/// NOTE: This test is ignored because WETH.withdraw() reverts on Anvil's forked
/// mode due to a gas stipend issue with `transfer()`. The compiler output is
/// correct — see plans/issues/weth-withdraw-anvil-revert.md for details.
#[tokio::test]
#[ignore = "WETH withdraw() reverts on Anvil fork — known environment bug, not a compiler bug"]
async fn test_unwrap_weth_on_anvil() -> eyre::Result<()> {
    // Pin the forked Anvil's chain_id to 31337 (what `config/chains.json`'s
    // "anvil" entry declares) so compiled txs — which carry `chain_id: 31337`
    // — aren't rejected by the node's signer-chain-id check.
    let anvil = Anvil::new().fork(fork_url()).chain_id(31337).try_spawn()?;

    let provider = ProviderBuilder::new().connect_http(anvil.endpoint_url());

    let signer = anvil.addresses()[0];
    let weth_addr = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
    let weth = IWETH::new(weth_addr, &provider);

    // Wrap 5 ETH first via compiled intent (properly updates all WETH state)
    let wrap_json = format!(
        r#"{{
            "network": "anvil",
            "from": "{}",
            "steps": [
                {{ "wrap": {{ "asset": "ETH", "amount": "5.0" }} }}
            ]
        }}"#,
        signer
    );
    let wrap_result = compile_intent(&wrap_json).unwrap();
    let wrap_txs = extract_txs(&wrap_result.output);
    provider
        .send_transaction(to_alloy_tx(wrap_txs[0]))
        .await?
        .get_receipt()
        .await?;

    let balance_before = weth.balanceOf(signer).call().await?;
    println!("WETH balance after wrap: {balance_before}");
    assert_eq!(balance_before, U256::from(5_000_000_000_000_000_000u64));

    // Now compile and submit unwrap intent for 2 WETH
    let unwrap_json = format!(
        r#"{{
            "network": "anvil",
            "from": "{}",
            "steps": [
                {{ "unwrap": {{ "asset": "WETH", "amount": "2.0" }} }}
            ]
        }}"#,
        signer
    );

    let result = compile_intent(&unwrap_json).unwrap();
    let txs = extract_txs(&result.output);
    assert_eq!(txs.len(), 1);

    let receipt = provider
        .send_transaction(to_alloy_tx(txs[0]))
        .await?
        .get_receipt()
        .await?;
    assert!(receipt.status(), "Unwrap tx should succeed");

    let balance_after = weth.balanceOf(signer).call().await?;
    let expected_decrease = U256::from(2_000_000_000_000_000_000u64);
    let actual_decrease = balance_before - balance_after;
    assert_eq!(actual_decrease, expected_decrease);

    println!("✅ Unwrap WETH→ETH succeeded! WETH balance: {balance_before} → {balance_after}");
    Ok(())
}

/// Test: Aave V3 deposit USDC — verify compiler produces batched router tx.
///
/// Since a router address is configured, the compiler batches the approve + supply
/// into a single router.execute() call. This test verifies the output structure.
/// Full on-chain execution of the batched tx is tested in the Foundry test suite.
#[tokio::test]
async fn test_aave_deposit_usdc_produces_batched_tx() -> eyre::Result<()> {
    // Pin the forked Anvil's chain_id to 31337 (what `config/chains.json`'s
    // "anvil" entry declares) so compiled txs — which carry `chain_id: 31337`
    // — aren't rejected by the node's signer-chain-id check.
    let anvil = Anvil::new().fork(fork_url()).chain_id(31337).try_spawn()?;

    let signer = anvil.addresses()[0];
    // Deployed IntentRouter address — sourced from config/protocols/anvil.json
    let router_addr = address!("9fF4608bAEb3a055CcBBa85c2Aabaf6EF5c50120");

    // Compile the deposit intent
    let intent_json = format!(
        r#"{{
            "network": "anvil",
            "from": "{}",
            "steps": [
                {{ "deposit": {{ "asset": "USDC", "amount": "100", "into": "aave" }} }}
            ]
        }}"#,
        signer
    );

    let result = compile_intent(&intent_json).expect("compile should succeed");

    // With a router configured, the 2 calls (approve + supply) are batched
    // into a single router.execute() tx
    let txs = extract_txs(&result.output);
    assert_eq!(
        txs.len(),
        1,
        "Batched deposit should produce 1 tx (router.execute)"
    );

    assert_eq!(
        txs[0].to, router_addr,
        "Batched tx should target the IntentRouter"
    );
    assert_eq!(txs[0].value, U256::ZERO, "No ETH value for USDC deposit");
    assert!(
        txs[0].data.len() > 4,
        "Should have calldata for router.execute()"
    );
    assert!(
        txs[0].description.contains("Batched"),
        "Description should mention batching"
    );

    println!(
        "✅ Aave deposit compiled to batched router tx ({} bytes calldata)",
        txs[0].data.len()
    );
    Ok(())
}
