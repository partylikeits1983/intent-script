//! Cross-stack end-to-end test for the `advisor` binary.
//!
//! Spawns a seeded local Anvil + IntentRouter via `scripts/start-anvil.sh`,
//! runs the advisor binary against it (`--simulate --rpc <local>`), and
//! asserts that the on-chain simulation produced the expected balance delta
//! on the signer.
//!
//! Gated behind `#[ignore]` so default `cargo test` doesn't pull in the LLM
//! call / anvil spawn. Run with:
//!
//! ```text
//! cargo test -p intent-script --features advisor --test advisor_e2e -- \
//!   --ignored --nocapture --test-threads=1
//! ```
//!
//! Or via the makefile: `make test-e2e-advisor`.

mod common;

use std::path::PathBuf;

use common::e2e::{AnvilGuard, run_advisor, skip_reason};

fn example_context() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/advisor-context.json")
}

/// Mirrors the user's reference command:
///
/// ```text
/// cargo run -p intent-script --features advisor --bin advisor -- \
///   "wrap 1 ETH to WETH" \
///   --context crates/intent-script/examples/advisor-context.json \
///   --network anvil --pretty
/// ```
///
/// Plus `--simulate --rpc <local-anvil>` so we get an on-chain assertion.
#[test]
#[ignore = "spawns anvil + calls OpenAI; run with --ignored"]
fn advisor_e2e_wraps_eth_to_weth() {
    if let Some(reason) = skip_reason() {
        eprintln!("SKIP advisor_e2e_wraps_eth_to_weth: {reason}");
        return;
    }

    let anvil = AnvilGuard::spawn().expect("anvil failed to start");
    let out = run_advisor("wrap 1 ETH to WETH", &example_context(), &anvil.rpc)
        .expect("advisor failed to run");

    eprintln!("=== advisor stderr ===\n{}", out.stderr);
    eprintln!("=== advisor stdout ===\n{}", out.stdout);

    if out.was_rate_limited() {
        eprintln!("SKIP advisor_e2e_wraps_eth_to_weth: OpenAI rate-limited");
        return;
    }
    assert_eq!(
        out.exit_code,
        Some(0),
        "advisor exited non-zero — see stdout/stderr above"
    );
    assert!(
        out.simulation_succeeded(),
        "simulation block did not report 'all transactions succeeded'"
    );

    let deltas = out.asset_deltas();
    let weth = deltas
        .iter()
        .find(|(sym, _)| sym == "WETH")
        .expect("no WETH delta in simulation report");
    // The change column is signed and human-formatted, e.g. "+1.0" or
    // "+1.000000000000000000". We just check it starts with '+' and the
    // numeric body is exactly 1 (allowing trailing zeros).
    let change = &weth.1;
    assert!(
        change.starts_with('+'),
        "WETH delta was not positive: {change:?}"
    );
    let numeric = change.trim_start_matches('+');
    let normalized: f64 = numeric
        .parse()
        .unwrap_or_else(|_| panic!("could not parse WETH delta {numeric:?} as f64"));
    assert!(
        (normalized - 1.0).abs() < 1e-9,
        "WETH delta should be ~+1, got {normalized}"
    );
}
