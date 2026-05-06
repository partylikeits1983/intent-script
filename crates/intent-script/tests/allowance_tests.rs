mod common;

use common::{compile_anvil_with_allowances, max_allowance_decimal};
use intent_script::CompileOutput;

const USDC_DEPOSIT_INPUT: &str = r#"{
    "network": "anvil",
    "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
    "steps": [
        { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } }
    ]
}"#;

#[test]
fn prerequisite_approval_emitted_when_allowance_is_below_required_pull() {
    let allowances = r#"{ "tokens": { "USDC": "4999999999" } }"#;
    let result =
        compile_anvil_with_allowances(USDC_DEPOSIT_INPUT, Some(allowances)).expect("compile ok");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(intent.prerequisite_approvals.len(), 1);
            assert_eq!(
                format!("{}", intent.prerequisite_approvals[0].to),
                "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
            );
        }
        other => panic!("expected Eip712Intent, got {other:?}"),
    }
}

#[test]
fn exact_allowance_match_emits_no_prerequisite_approval() {
    let allowances = r#"{ "tokens": { "USDC": "5000000000" } }"#;
    let result =
        compile_anvil_with_allowances(USDC_DEPOSIT_INPUT, Some(allowances)).expect("compile ok");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(intent.prerequisite_approvals.is_empty());
        }
        other => panic!("expected Eip712Intent, got {other:?}"),
    }
}

#[test]
fn larger_allowance_emits_no_prerequisite_approval() {
    let allowances = format!(
        r#"{{ "tokens": {{ "USDC": "{}" }} }}"#,
        max_allowance_decimal()
    );
    let result =
        compile_anvil_with_allowances(USDC_DEPOSIT_INPUT, Some(&allowances)).expect("compile ok");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(intent.prerequisite_approvals.is_empty());
        }
        other => panic!("expected Eip712Intent, got {other:?}"),
    }
}

#[test]
fn missing_token_entry_is_treated_as_zero_allowance() {
    let allowances = r#"{ "tokens": { "DAI": "1" } }"#;
    let result =
        compile_anvil_with_allowances(USDC_DEPOSIT_INPUT, Some(allowances)).expect("compile ok");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert_eq!(intent.prerequisite_approvals.len(), 1);
        }
        other => panic!("expected Eip712Intent, got {other:?}"),
    }
}

#[test]
fn unknown_alias_in_allowances_emits_warning_and_does_not_break_compile() {
    let allowances = r#"{ "tokens": { "FAKE": "123", "USDC": "5000000000" } }"#;
    let result =
        compile_anvil_with_allowances(USDC_DEPOSIT_INPUT, Some(allowances)).expect("compile ok");

    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("Allowance entry for unknown asset 'FAKE' ignored")),
        "expected unknown-asset allowance warning, got {:?}",
        result.warnings
    );
}

#[test]
fn malformed_allowance_value_fails_compilation() {
    let allowances = r#"{ "tokens": { "USDC": "not-a-number" } }"#;
    let err = compile_anvil_with_allowances(USDC_DEPOSIT_INPUT, Some(allowances))
        .unwrap_err()
        .to_string();

    assert!(err.contains("Invalid amount"));
}

#[test]
fn native_asset_allowance_entry_is_ignored_with_warning() {
    let allowances = r#"{ "tokens": { "ETH": "1", "USDC": "5000000000" } }"#;
    let result =
        compile_anvil_with_allowances(USDC_DEPOSIT_INPUT, Some(allowances)).expect("compile ok");

    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("Allowance entry for unknown asset 'ETH' ignored")),
        "expected native-asset allowance warning, got {:?}",
        result.warnings
    );
}

// ─── Aave V3 credit delegation prerequisites ─────────────────────────────────
//
// When the IntentRouter is `msg.sender` for an Aave V3 borrow but the user is
// `onBehalfOf`, Aave reverts with custom error `0x1cb19ef3`
// (InsufficientBorrowAllowance) unless the user has previously called
// `vDebtToken.approveDelegation(router, amount)`. The compiler emits that
// `approveDelegation` call as a prerequisite UnsignedTx alongside ERC-20
// approves, filtered against the caller-supplied delegation snapshot.

const USDC_DEPOSIT_USDT_BORROW: &str = r#"{
    "network": "anvil",
    "from": "0x14dC79964da2C08b23698B3D3cc7Ca32193d9955",
    "current_timestamp": 1778086140,
    "steps": [
        { "deposit": { "asset": "USDC", "amount": "10000", "into": "aave" } },
        { "borrow":  { "asset": "USDT", "amount": "2000",  "from": "aave" } }
    ]
}"#;

/// `approveDelegation(address,uint256)` selector — keccak256 of the signature
/// taken from the first 4 bytes. Hard-coded here so tests can recognize the
/// prerequisite without depending on alloy's encoder.
const APPROVE_DELEGATION_SELECTOR: [u8; 4] = [0xc0, 0x4a, 0x8a, 0x10];

/// Aave V3 mainnet variable-debt-token for USDT (mirrors the configured
/// `variable_debt_tokens` map). Used to verify the prerequisite tx is
/// targeted at the right contract.
const VDEBT_USDT: &str = "0x6df1c1E379bC5a00a7b4C6e67a203333772f45A8";

#[test]
fn aave_borrow_emits_credit_delegation_prerequisite() {
    // No allowances, no delegations: caller will need to approve both the
    // ERC-20 transferFrom *and* the credit delegation before the batch.
    let allowances = r#"{ "tokens": {}, "delegations": {} }"#;
    let result = compile_anvil_with_allowances(USDC_DEPOSIT_USDT_BORROW, Some(allowances))
        .expect("compile ok");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            // Should have two prerequisites: ERC-20 USDC approve + USDT
            // credit delegation.
            assert_eq!(
                intent.prerequisite_approvals.len(),
                2,
                "expected ERC-20 approve + credit delegation, got {:?}",
                intent.prerequisite_approvals
            );

            let delegation = intent
                .prerequisite_approvals
                .iter()
                .find(|tx| tx.data.starts_with(&APPROVE_DELEGATION_SELECTOR))
                .expect("expected one approveDelegation prerequisite");

            // Target = vDebt USDT mainnet contract.
            assert_eq!(
                format!("{}", delegation.to).to_lowercase(),
                VDEBT_USDT.to_lowercase(),
                "delegation prereq should target the USDT variable debt token"
            );

            // Calldata payload (after the 4-byte selector): 32-byte
            // delegatee address + 32-byte amount. Last 32 bytes encode 2000e6.
            let amount_bytes = &delegation.data[delegation.data.len() - 32..];
            let mut padded = [0u8; 32];
            padded[24..].copy_from_slice(&2_000_000_000u64.to_be_bytes()); // 2000e6
            assert_eq!(amount_bytes, &padded[..], "delegation amount mismatch");
        }
        other => panic!("expected Eip712Intent (router-batched), got {other:?}"),
    }
}

#[test]
fn aave_borrow_skips_delegation_prereq_when_already_delegated() {
    // Caller reports a delegation that exceeds the borrow amount: prereq
    // should be omitted, exactly mirroring how ERC-20 allowance saturation
    // suppresses the approve prereq.
    let allowances = r#"{
        "tokens": { "USDC": "999999999999999999999999" },
        "delegations": { "USDT": "999999999999999999999999" }
    }"#;
    let result = compile_anvil_with_allowances(USDC_DEPOSIT_USDT_BORROW, Some(allowances))
        .expect("compile ok");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(
                intent
                    .prerequisite_approvals
                    .iter()
                    .all(|tx| !tx.data.starts_with(&APPROVE_DELEGATION_SELECTOR)),
                "no approveDelegation prereq expected when delegation is already saturated; got {:?}",
                intent.prerequisite_approvals
            );
            assert!(
                intent.prerequisite_approvals.is_empty(),
                "all prereqs should be cleared by saturated allowances + delegations"
            );
        }
        other => panic!("expected Eip712Intent, got {other:?}"),
    }
}

#[test]
fn aave_borrow_legacy_compile_omits_delegation_prereq_for_backcompat() {
    // No allowances JSON ⇒ no prereqs of any kind, byte-identical to
    // pre-feature compile() output. Same back-compat contract as ERC-20
    // approve emission.
    let result = compile_anvil_with_allowances(USDC_DEPOSIT_USDT_BORROW, None).expect("compile ok");
    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            assert!(
                intent.prerequisite_approvals.is_empty(),
                "legacy compile() without allowances must not emit prereqs"
            );
        }
        other => panic!("expected Eip712Intent, got {other:?}"),
    }
}

#[test]
fn aave_borrow_with_partial_delegation_emits_prereq_for_full_amount() {
    // Caller has *some* delegation but less than the borrow amount: a
    // prereq is still required, and it asks for the *full* required amount
    // (matches the existing approve-emission strategy in build.rs — re-grant
    // the full need rather than the marginal delta, since on-chain
    // `approveDelegation` overwrites the previous allowance, not adds).
    let allowances =
        r#"{ "tokens": { "USDC": "10000000000" }, "delegations": { "USDT": "1000000000" } }"#;
    let result = compile_anvil_with_allowances(USDC_DEPOSIT_USDT_BORROW, Some(allowances))
        .expect("compile ok");

    match &result.output {
        CompileOutput::Eip712Intent(intent) => {
            let delegation = intent
                .prerequisite_approvals
                .iter()
                .find(|tx| tx.data.starts_with(&APPROVE_DELEGATION_SELECTOR))
                .expect("expected approveDelegation prereq");
            // Encoded amount in last 32 bytes should equal the borrow amount
            // (2000e6), not the delta (1000e6).
            let mut expected = [0u8; 32];
            expected[24..].copy_from_slice(&2_000_000_000u64.to_be_bytes());
            let amount_bytes = &delegation.data[delegation.data.len() - 32..];
            assert_eq!(amount_bytes, &expected[..]);
        }
        other => panic!("expected Eip712Intent, got {other:?}"),
    }
}
