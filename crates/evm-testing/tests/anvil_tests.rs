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

mod hex {
    pub fn encode(data: &[u8]) -> String {
        data.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Test: Wrap ETH → WETH on a local Anvil instance forking mainnet.
#[tokio::test]
async fn test_wrap_eth_on_anvil() -> eyre::Result<()> {
    let anvil = Anvil::new().fork(fork_url()).try_spawn()?;

    let provider = ProviderBuilder::new().connect_http(anvil.endpoint_url());

    let signer = anvil.addresses()[0];
    let weth_addr = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");

    let weth = IWETH::new(weth_addr, &provider);
    let initial_balance = weth.balanceOf(signer).call().await?;
    println!("Initial WETH balance: {initial_balance}");

    // Compile the wrap intent
    let intent_json = format!(
        r#"{{
            "network": "ethereum",
            "from": "{}",
            "steps": [
                {{ "wrap": {{ "asset": "ETH", "amount": "1.5" }} }}
            ]
        }}"#,
        signer
    );

    let output = compile_intent(&intent_json).expect("compile should succeed");
    let txs = extract_txs(&output);
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
#[tokio::test]
async fn test_unwrap_weth_on_anvil() -> eyre::Result<()> {
    let anvil = Anvil::new().fork(fork_url()).try_spawn()?;

    let provider = ProviderBuilder::new().connect_http(anvil.endpoint_url());

    let signer = anvil.addresses()[0];
    let weth_addr = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
    let weth = IWETH::new(weth_addr, &provider);

    // Wrap 5 ETH first via compiled intent (properly updates all WETH state)
    let wrap_json = format!(
        r#"{{
            "network": "ethereum",
            "from": "{}",
            "steps": [
                {{ "wrap": {{ "asset": "ETH", "amount": "5.0" }} }}
            ]
        }}"#,
        signer
    );
    let wrap_output = compile_intent(&wrap_json).unwrap();
    let wrap_txs = extract_txs(&wrap_output);
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
            "network": "ethereum",
            "from": "{}",
            "steps": [
                {{ "unwrap": {{ "asset": "WETH", "amount": "2.0" }} }}
            ]
        }}"#,
        signer
    );

    let output = compile_intent(&unwrap_json).unwrap();
    let txs = extract_txs(&output);
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

/// Test: Aave V3 deposit USDC on forked mainnet.
///
/// Uses cheatcodes to set USDC balance for the signer.
#[tokio::test]
async fn test_aave_deposit_usdc_on_anvil() -> eyre::Result<()> {
    let anvil = Anvil::new().fork(fork_url()).try_spawn()?;

    let provider = ProviderBuilder::new().connect_http(anvil.endpoint_url());

    let signer = anvil.addresses()[0];
    let usdc_addr = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
    let aave_pool = address!("87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2");

    // Set USDC balance for signer using cheatcode
    // USDC (proxy) balanceOf mapping is at storage slot 9
    let usdc_amount = U256::from(10_000_000_000u64); // 10,000 USDC (6 decimals)
    set_erc20_balance(&provider, usdc_addr, signer, usdc_amount, U256::from(9)).await?;

    let usdc = IERC20::new(usdc_addr, &provider);
    let initial_balance = usdc.balanceOf(signer).call().await?;
    println!("Signer USDC balance: {initial_balance}");
    assert!(
        initial_balance >= U256::from(100_000_000u64),
        "Should have at least 100 USDC"
    );

    // Compile the deposit intent
    let deposit_amount_usdc = U256::from(100_000_000u64); // 100 USDC
    let intent_json = format!(
        r#"{{
            "network": "ethereum",
            "from": "{}",
            "steps": [
                {{ "deposit": {{ "asset": "USDC", "amount": "100", "into": "aave" }} }}
            ]
        }}"#,
        signer
    );

    let output = compile_intent(&intent_json).expect("compile should succeed");
    let txs = extract_txs(&output);
    assert_eq!(
        txs.len(),
        2,
        "Deposit should produce 2 txs (approve + supply)"
    );

    assert_eq!(
        txs[0].to, usdc_addr,
        "First tx should target USDC (approve)"
    );
    assert_eq!(
        txs[1].to, aave_pool,
        "Second tx should target Aave Pool (supply)"
    );

    // Submit approve tx
    let approve_receipt = provider
        .send_transaction(to_alloy_tx(txs[0]))
        .await?
        .get_receipt()
        .await?;
    assert!(approve_receipt.status(), "Approve tx should succeed");
    println!("✅ Approve tx succeeded");

    // Verify allowance
    let allowance = usdc.allowance(signer, aave_pool).call().await?;
    println!("USDC allowance for Aave: {allowance}");
    assert!(allowance >= deposit_amount_usdc);

    // Submit supply tx
    let supply_receipt = provider
        .send_transaction(to_alloy_tx(txs[1]))
        .await?
        .get_receipt()
        .await?;
    assert!(supply_receipt.status(), "Supply tx should succeed");
    println!("✅ Supply tx succeeded");

    // Verify USDC balance decreased
    let final_balance = usdc.balanceOf(signer).call().await?;
    let spent = initial_balance - final_balance;
    assert_eq!(spent, deposit_amount_usdc);

    println!(
        "✅ Aave deposit succeeded! USDC balance: {initial_balance} → {final_balance} (spent {spent})"
    );
    Ok(())
}
