//! Recipient-pinning invariant tests (B1).
//!
//! Today's normalizer sets every internal recipient / on_behalf / receiver /
//! owner field to the signer — the LLM has no input into these fields via
//! JSON. The recipient-pinning validator enforces that invariant at the IR
//! level as defense-in-depth: a future schema change or a regression in
//! normalize can't silently leak funds to an attacker-controlled address,
//! because `validate()` rejects before enrich / lower / build ever run.
//!
//! These tests bypass the JSON→IR path (which can't produce a non-signer
//! recipient today) and construct minimal `ResolvedIntent`s by hand. The
//! positive control confirms the happy path; each negative case mutates a
//! single recipient field to a bogus address and asserts `validate()` rejects.

mod common;

use alloy_primitives::{Address, U256, address};
use intent_script::compiler::validate;
use intent_script::error::CompileError;
use intent_script::ir::{MorphoMarketParams, ResolvedIntent, ResolvedStep};
use intent_script::registry::RegistryContext;

const SIGNER: Address = address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
const ATTACKER: Address = address!("BAD000BAd000BAD000baD000Bad000BAD000bAd0");
const USDC: Address = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
const WETH: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
const AAVE_POOL: Address = address!("87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2");
const UNI_ROUTER: Address = address!("E592427A0AEce92De3Edee1F18E0157C05861564");
const NPM: Address = address!("C36442b4a4522E871399CD717aBDD847Ab11FE88");

fn registry() -> RegistryContext {
    let (chains, assets, protocols) = common::load_anvil_config();
    RegistryContext::load(&chains, &assets, &protocols, "anvil")
        .expect("anvil registry should load")
}

fn intent(steps: Vec<ResolvedStep>) -> ResolvedIntent {
    ResolvedIntent {
        chain_id: 31337,
        signer: SIGNER,
        steps,
        tokens_to_sweep: Vec::new(),
        nonce: 0,
        deadline: 0,
        user_balances: None,
        required_pulls: Vec::new(),
        required_delegations: Vec::new(),
        fee_bps: 0,
        requires_router: false,
    }
}

fn morpho_market() -> MorphoMarketParams {
    MorphoMarketParams {
        loan_token: USDC,
        collateral_token: WETH,
        oracle: Address::ZERO,
        irm: Address::ZERO,
        lltv: U256::ZERO,
        id: [0u8; 32],
    }
}

/// Positive control: the normalizer's happy-path assignment of
/// `on_behalf_of: signer` on an Aave supply must pass pinning.
#[test]
fn aave_supply_with_signer_on_behalf_of_passes() {
    let i = intent(vec![ResolvedStep::AaveV3Supply {
        pool: AAVE_POOL,
        asset: USDC,
        amount: U256::from(1_000_000u64),
        on_behalf_of: SIGNER,
    }]);
    validate::validate(&i, &registry()).expect("signer-pinned supply should pass");
}

#[test]
fn aave_supply_with_attacker_on_behalf_of_is_rejected() {
    let i = intent(vec![ResolvedStep::AaveV3Supply {
        pool: AAVE_POOL,
        asset: USDC,
        amount: U256::from(1_000_000u64),
        on_behalf_of: ATTACKER,
    }]);
    let err = validate::validate(&i, &registry()).expect_err("must reject non-signer on_behalf_of");
    match err {
        CompileError::Validation(msg) => {
            assert!(
                msg.contains("on_behalf_of"),
                "err should name the field: {msg}"
            );
            assert!(msg.contains("signer"), "err should name signer: {msg}");
        }
        other => panic!("expected Validation error, got {other:?}"),
    }
}

#[test]
fn aave_borrow_with_attacker_on_behalf_of_is_rejected() {
    // Borrow's on_behalf_of is the *debt holder* — credit-delegation is a
    // known footgun. A hallucinated non-signer would borrow on the victim's
    // collateral against someone else's name.
    let i = intent(vec![
        ResolvedStep::AaveV3Supply {
            pool: AAVE_POOL,
            asset: USDC,
            amount: U256::from(10_000_000u64),
            on_behalf_of: SIGNER,
        },
        ResolvedStep::AaveV3Borrow {
            pool: AAVE_POOL,
            asset: WETH,
            amount: U256::from(1_000_000_000_000u64),
            rate_mode: 2,
            on_behalf_of: ATTACKER,
        },
    ]);
    let err = validate::validate(&i, &registry()).expect_err("borrow on_behalf_of must be pinned");
    assert!(
        matches!(err, CompileError::Validation(ref m) if m.contains("on_behalf_of")),
        "expected on_behalf_of rejection, got {err:?}"
    );
}

#[test]
fn aave_withdraw_with_attacker_to_is_rejected() {
    let i = intent(vec![ResolvedStep::AaveV3Withdraw {
        pool: AAVE_POOL,
        asset: USDC,
        amount: U256::from(1_000_000u64),
        to: ATTACKER,
    }]);
    let err = validate::validate(&i, &registry()).expect_err("withdraw recipient must be pinned");
    assert!(
        matches!(err, CompileError::Validation(ref m) if m.contains("'to'")),
        "expected 'to' rejection, got {err:?}"
    );
}

#[test]
fn uniswap_swap_with_attacker_recipient_is_rejected() {
    // The preview would net this out (inputs/outputs cancel for intermediate
    // tokens) but the swap actually sends WETH to the attacker. B1 catches it.
    let i = intent(vec![ResolvedStep::UniswapV3Swap {
        router: UNI_ROUTER,
        token_in: USDC,
        token_out: WETH,
        amount_in: U256::from(1_000_000u64),
        fee: 3000,
        recipient: ATTACKER,
        deadline: U256::from(1_712_345_000u64),
        amount_out_minimum: U256::from(1u64),
        native_input: false,
    }]);
    let err = validate::validate(&i, &registry()).expect_err("swap recipient must be pinned");
    assert!(
        matches!(err, CompileError::Validation(ref m) if m.contains("recipient")),
        "expected recipient rejection, got {err:?}"
    );
}

#[test]
fn uniswap_lp_mint_with_attacker_recipient_is_rejected() {
    let i = intent(vec![ResolvedStep::UniswapV3LpMint {
        npm: NPM,
        token0: USDC,
        token1: WETH,
        fee: 3000,
        tick_lower: -60,
        tick_upper: 60,
        amount0: U256::from(1_000_000u64),
        amount1: U256::from(1u64),
        amount0_min: U256::from(1u64),
        amount1_min: U256::from(1u64),
        recipient: ATTACKER,
        deadline: U256::from(1_712_345_000u64),
    }]);
    let err = validate::validate(&i, &registry()).expect_err("LP mint NFT must go to signer");
    assert!(
        matches!(err, CompileError::Validation(ref m) if m.contains("recipient")),
        "expected recipient rejection, got {err:?}"
    );
}

#[test]
fn morpho_borrow_with_attacker_receiver_is_rejected() {
    let i = intent(vec![ResolvedStep::MorphoBorrow {
        pool: address!("BBBBBbbBBb9cC5e90e3b3Af64bdAF62C37EEFFCb"),
        market_params: morpho_market(),
        amount: U256::from(1_000_000u64),
        on_behalf: SIGNER,
        receiver: ATTACKER,
    }]);
    let err = validate::validate(&i, &registry()).expect_err("morpho receiver must be pinned");
    assert!(
        matches!(err, CompileError::Validation(ref m) if m.contains("receiver")),
        "expected receiver rejection, got {err:?}"
    );
}

#[test]
fn erc20_transfer_from_with_attacker_source_is_rejected() {
    // Auto-inserted by the enricher; the `from` must always be the signer
    // (the user is the one being pulled from). A mismatched `from` means
    // the batch is trying to spend someone else's tokens.
    let i = intent(vec![ResolvedStep::Erc20TransferFrom {
        token: USDC,
        from: ATTACKER,
        to: Address::ZERO,
        amount: U256::from(1_000_000u64),
    }]);
    let err = validate::validate(&i, &registry()).expect_err("transferFrom source must be signer");
    assert!(
        matches!(err, CompileError::Validation(ref m) if m.contains("from")),
        "expected from-field rejection, got {err:?}"
    );
}

#[test]
fn send_steps_are_exempt_from_pinning() {
    // Explicit user-directed transfers to third parties are the only way
    // out of the router custody boundary. They're validated separately
    // (send-to-zero is rejected by validate_send) but are NOT pinned to
    // the signer because the whole point is to send elsewhere.
    let i = intent(vec![ResolvedStep::SendErc20 {
        token: USDC,
        to: ATTACKER, // explicit user-chosen address — allowed here, even
        // if the destination *looks* adversarial — because the
        // user authored the send step themselves.
        amount: U256::from(1_000_000u64),
    }]);
    validate::validate(&i, &registry()).expect("explicit send must not be pinning-rejected");
}
