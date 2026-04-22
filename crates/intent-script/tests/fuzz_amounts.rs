//! Fuzz-style tests for amount parsing edge cases.
//!
//! Tests the `parse_amount` function through the public `compile()` API
//! to ensure robust handling of all possible amount string inputs.

use std::path::{Path, PathBuf};

use intent_script::{CompileResult, compile};

fn config_dir() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
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

fn do_compile(input: &str) -> Result<CompileResult, intent_script::error::CompileError> {
    let (c, a, p) = load_config();
    compile(input, &c, &a, &p)
}

/// Helper: compile a wrap intent with the given amount string.
/// Returns Ok if compilation succeeds, Err with the error message if it fails.
fn compile_with_amount(amount: &str) -> Result<(), String> {
    let input = format!(
        r#"{{
            "network": "anvil",
            "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
            "steps": [
                {{ "wrap": {{ "asset": "ETH", "amount": "{amount}" }} }}
            ]
        }}"#
    );
    do_compile(&input).map(|_| ()).map_err(|e| e.to_string())
}

/// Helper: compile a deposit intent with the given amount string (USDC, 6 decimals).
fn compile_usdc_deposit(amount: &str) -> Result<(), String> {
    let input = format!(
        r#"{{
            "network": "anvil",
            "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
            "steps": [
                {{ "deposit": {{ "asset": "USDC", "amount": "{amount}", "into": "aave" }} }}
            ]
        }}"#
    );
    do_compile(&input).map(|_| ()).map_err(|e| e.to_string())
}

// ─── Valid amounts ─────────────────────────────────────────────────────

#[test]
fn test_integer_amount() {
    assert!(compile_with_amount("1").is_ok());
}

#[test]
fn test_large_integer_amount() {
    assert!(compile_with_amount("999999999999").is_ok());
}

#[test]
fn test_decimal_amount() {
    assert!(compile_with_amount("1.5").is_ok());
}

#[test]
fn test_small_decimal_amount() {
    // 0.000001 ETH = 1000000000000 wei (18 decimals)
    assert!(compile_with_amount("0.000001").is_ok());
}

#[test]
fn test_very_small_usdc_amount() {
    // 0.000001 USDC = 1 (6 decimals, minimum unit)
    assert!(compile_usdc_deposit("0.000001").is_ok());
}

#[test]
fn test_trailing_zeros() {
    assert!(compile_with_amount("1.50000").is_ok());
}

#[test]
fn test_leading_zeros_in_integer() {
    // "001.5" — the integer part "001" should parse as 1
    assert!(compile_with_amount("001.5").is_ok());
}

#[test]
fn test_whole_number_with_decimal_point() {
    assert!(compile_with_amount("100.0").is_ok());
}

#[test]
fn test_max_precision_usdc() {
    // USDC has 6 decimals — "0.123456" should work
    assert!(compile_usdc_deposit("0.123456").is_ok());
}

// ─── Invalid amounts ───────────────────────────────────────────────────

#[test]
fn test_zero_amount_rejected() {
    let result = compile_with_amount("0");
    assert!(result.is_err(), "Zero amount should be rejected");
    let err = result.unwrap_err();
    assert!(
        err.contains("greater than zero") || err.contains("Invalid"),
        "Error should mention zero amount: {err}"
    );
}

#[test]
fn test_zero_decimal_amount_rejected() {
    let result = compile_with_amount("0.0");
    assert!(result.is_err(), "Zero decimal amount should be rejected");
}

#[test]
fn test_zero_with_many_zeros_rejected() {
    let result = compile_with_amount("0.000000");
    assert!(
        result.is_err(),
        "Zero with many decimal zeros should be rejected"
    );
}

#[test]
fn test_empty_string_rejected() {
    let result = compile_with_amount("");
    assert!(result.is_err(), "Empty string should be rejected");
}

#[test]
fn test_just_a_dot_rejected() {
    let result = compile_with_amount(".");
    assert!(result.is_err(), "Just a dot should be rejected");
}

#[test]
fn test_multiple_dots_rejected() {
    let result = compile_with_amount("1.2.3");
    assert!(result.is_err(), "Multiple dots should be rejected");
}

#[test]
fn test_negative_amount_rejected() {
    let result = compile_with_amount("-1.5");
    assert!(result.is_err(), "Negative amount should be rejected");
}

#[test]
fn test_negative_integer_rejected() {
    let result = compile_with_amount("-100");
    assert!(result.is_err(), "Negative integer should be rejected");
}

#[test]
fn test_scientific_notation_rejected() {
    let result = compile_with_amount("1e18");
    assert!(
        result.is_err(),
        "Scientific notation should be rejected (not supported)"
    );
}

#[test]
fn test_whitespace_rejected() {
    let result = compile_with_amount(" 1.5 ");
    assert!(result.is_err(), "Whitespace in amount should be rejected");
}

#[test]
fn test_non_numeric_rejected() {
    let result = compile_with_amount("abc");
    assert!(result.is_err(), "Non-numeric string should be rejected");
}

#[test]
fn test_comma_separator_rejected() {
    let result = compile_with_amount("1,000.50");
    assert!(
        result.is_err(),
        "Comma separators should be rejected (not supported)"
    );
}

#[test]
fn test_too_many_decimals_for_usdc() {
    // USDC has 6 decimals — "0.1234567" has 7 decimal places
    let result = compile_usdc_deposit("0.1234567");
    assert!(
        result.is_err(),
        "More decimal places than token supports should be rejected"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("decimal"),
        "Error should mention decimal places: {err}"
    );
}

#[test]
fn test_unicode_digits_rejected() {
    let result = compile_with_amount("１.５");
    assert!(
        result.is_err(),
        "Unicode digits should be rejected (not supported)"
    );
}

#[test]
fn test_plus_sign_accepted() {
    // Rust's u128::parse accepts "+1" as valid, so "+1.5" compiles successfully.
    // This is acceptable behavior — "+1.5" is unambiguously a positive number.
    assert!(compile_with_amount("+1.5").is_ok());
}

#[test]
fn test_hex_amount_rejected() {
    let result = compile_with_amount("0xff");
    assert!(result.is_err(), "Hex amount should be rejected");
}

// ─── Boundary cases ────────────────────────────────────────────────────

#[test]
fn test_very_large_amount() {
    // Very large but valid amount
    assert!(compile_with_amount("999999999999999999").is_ok());
}

#[test]
fn test_one_wei_equivalent() {
    // Smallest possible ETH amount: 0.000000000000000001 (18 decimals)
    assert!(compile_with_amount("0.000000000000000001").is_ok());
}

#[test]
fn test_one_usdc_unit() {
    // Smallest possible USDC amount: 0.000001 (6 decimals)
    assert!(compile_usdc_deposit("0.000001").is_ok());
}
