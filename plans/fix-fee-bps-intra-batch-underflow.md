# Plan: fix fee_bps intra-batch underflow in `validate_amount_flow`

## Context

A user-submitted intent of the form `wrap 50 ETH → deposit 50 WETH → borrow 10k USDT` is rejected at compile time with:

```
Invalid intent chain: Step 2 requires 50000000000000000000 of token
0xC02a…756Cc2 but previous steps only guarantee 49950000000000000000
```

The 0.05 WETH (= 0.1%) shortfall is exactly `intent_router.fee_bps = 10` from `config/protocols/anvil.json:82`. The intent is semantically valid — on-chain, the router holds the full 50 WETH between the `wrap` and `deposit` calls; the fee is skimmed only at sweep time when leftover tokens flow back to the signer. The compiler is applying the fee one step too early in the cross-step amount flow check, so every multi-step intent that uses an **exact** amount for step 2+ gets incorrectly rejected on any network with `fee_bps > 0`. `"amount": "all"` dodges this only because normalize pre-resolves it to the same reduced floor the validator sees.

This blocks the "user types a round number" path that the UI and the LLM both lean on, which makes it a high-impact fix for a one-line change.

## Root cause

`step_produces` (`crates/intent-script/src/ir/canonical.rs:484–486`) unconditionally discounts by `fee_bps`:

```rust
let reduced = amount * U256::from(10_000 - fee_bps) / U256::from(10_000);
Some((token, reduced))
```

That's correct for callers that need the *post-sweep* floor (downstream `"all"` consumers, user-facing preview outputs). It is wrong for `validate_amount_flow` (`crates/intent-script/src/compiler/validate.rs:255–278`), which is checking router-internal hand-offs between steps — where no fee is skimmed on-chain.

Note on precedent: the same file already sets `fee_bps = 0` for the inner pipeline of a Balancer flashloan at line 175 (`let _ = fee_bps;`), with the rationale *"produced tokens inside a flashloan are returned to the Vault by `receiveFlashLoan`, not swept through the router fee path, so no fee reduction applies."* The same reasoning applies to intra-batch hand-offs: the produced tokens are consumed by the next step inside the router, never touching the sweep path.

## Approach

Single narrow fix: make `validate_amount_flow` pass `fee_bps = 0` into `step_produces`. Leave every other caller alone.

Concretely in `crates/intent-script/src/compiler/validate.rs:255–278`:

```rust
fn validate_amount_flow(steps: &[ResolvedStep], fee_bps: u16) -> Result<()> {
    // Intra-batch hand-offs don't pass through sweep, so no fee reduction
    // applies here. Mirrors the flashloan inner-pipeline rule at line 175.
    let _ = fee_bps;
    let mut produced: HashMap<Address, U256> = HashMap::new();
    for (i, step) in steps.iter().enumerate() {
        if let Some((token, required)) = step_consumes(step) {
            if let Some(available) = produced.get(&token) {
                if required > *available {
                    return Err(CompileError::InvalidChain(format!(
                        "Step {} requires {} of token {} but previous steps only guarantee {}",
                        i + 1, required, token, available
                    )));
                }
                produced.insert(token, *available - required);
            }
        }
        if let Some((token, guaranteed)) = step_produces(step, 0) {
            *produced.entry(token).or_insert(U256::ZERO) += guaranteed;
        }
    }
    Ok(())
}
```

Keep the `fee_bps` parameter in the signature so callers don't need to change — just ignore it with `let _ = fee_bps;` (matches the existing flashloan branch idiom). `step_consumes` needs no change — it doesn't take a `fee_bps`.

### Not in scope

- `normalize.rs:1257` still threads `fee_bps` into `step_produces` when resolving `"all"`. That's conservative defense-in-depth (over-reports the floor by 0.1% when the `"all"` consumer is actually router-internal, which could cause a 0.1% under-spend later) but doesn't cause user-visible rejection — leave for a follow-up.
- `build_preview` (`compiler/preview.rs:27`) continues to apply `fee_bps` to the user-facing "You receive (minimum)" output line. That IS correct — those tokens sweep back to the user and the fee does apply.
- Separate polish: `CompileError::InvalidChain` could format raw wei into human units so the "Ask LLM to fix this" retry gets a legible error. Cheap, but unrelated to the bug.

## Critical files

Modify:
- `crates/intent-script/src/compiler/validate.rs` — swap `step_produces(step, fee_bps)` at line 273 for `step_produces(step, 0)`, add the one-line rationale comment.

Read-only references:
- `crates/intent-script/src/ir/canonical.rs:484–486` — `step_produces` fee application.
- `crates/intent-script/src/compiler/validate.rs:175` — existing `let _ = fee_bps;` precedent inside flashloan validation.
- `crates/intent-script/src/compiler/normalize.rs:1257` — `"all"` resolution (intentionally keeps `fee_bps`).
- `config/protocols/anvil.json:82` — where `fee_bps: 10` lives, used by the user's failing case.

## Verification

1. **Regression test.** Add to `crates/intent-script/tests/integration.rs`:

```rust
#[test]
fn test_wrap_then_deposit_exact_amount_accepts_with_router_fee() {
    // Mirrors the user-reported failure: network=anvil (fee_bps=10) with an
    // exact-amount intra-batch hand-off must compile. Previously rejected
    // because the validator applied the 0.1% sweep fee to an intermediate
    // produce that never actually sweeps.
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap":    { "asset": "ETH",  "amount": "50" } },
            { "deposit": { "asset": "WETH", "amount": "50", "into": "aave" } }
        ]
    }"#;
    do_compile(input).expect(
        "exact-amount intra-batch hand-off must compile with router fee_bps set",
    );
}
```

Add a three-step variant too so the `borrow` tail is exercised:

```rust
#[test]
fn test_wrap_deposit_borrow_chain_accepts_exact_amounts() {
    let input = r#"{
        "network": "anvil",
        "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
        "steps": [
            { "wrap":    { "asset": "ETH",  "amount": "50" } },
            { "deposit": { "asset": "WETH", "amount": "50", "into": "aave" } },
            { "borrow":  { "asset": "USDT", "amount": "10000", "from": "aave" } }
        ]
    }"#;
    do_compile(input).expect("wrap→deposit→borrow with exact amounts must compile");
}
```

2. **Re-run the full suite.** `cargo test -p intent-script` — the 200+ existing tests must stay green (no test today relies on the validator discounting by fee_bps on a network where it's set, so this should not regress anything).

3. **Rebuild WASM + smoke test in the UI.** `pnpm build:wasm` in `intentOS-ui`, then retry the user's exact prompt in the chat:

   > "Wrap 50 ETH, then supply to Aave, then borrow 10k USDT"

   Expect: compile succeeds, preview renders, sim passes.

4. **"All" still works.** Manually re-verify the canonical composed example from the cookbook ("swap 5000 USDC → WETH, then deposit all into Aave") still compiles and produces the correct preview. `"all"` resolution is not on the changed path, but it's the adjacent logic most likely to surface a regression if anything drifted.

Step 3 is the real acceptance gate — the empirical symptom the user reported must disappear.
