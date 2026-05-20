# [WS-3C] Cross-stack advisor e2e (Rust → anvil fork)

**Repo:** `partylikeits1983/intent-script`
**Labels:** `area/testing`, `size/M`
**Depends on:** WS-3B

## Context

`intent-script`'s `tests/` directory exercises the compiler in isolation —
golden DSL → compile, recipient pinning, slippage caps, etc. None of those
tests drive the full natural-language → DSL → compile → simulate pipeline
that the `advisor` binary actually runs in production. We need a cross-stack
e2e harness that takes a plain-English prompt and asserts the final
on-anvil state, so that regressions in the LLM prompt, the DSL emitter, the
compiler, or the simulator all surface here.

The Playwright-driven full-stack flow originally scoped for `intentOS-ui`
(reactive chat + advisor card sign flows) is **out of scope here** — that
remains a UI-repo concern and gets its own issue if/when needed. This issue
covers only the Rust-driven advisor path, which is what the `advisor`
binary already does end-to-end:

```
cargo run -p intent-script --features advisor --bin advisor -- \
  "wrap 1 ETH to WETH" \
  --context crates/intent-script/examples/advisor-context.json \
  --network anvil --simulate --rpc http://127.0.0.1:8545 --pretty
```

## Scope

1. Test harness (`crates/intent-script/tests/common/e2e.rs`):
   - `AnvilGuard`: spawns `scripts/start-anvil.sh` on a randomized port,
     waits for `cast chain-id` to return 31337, kills + waits the child
     in `Drop`. Reuses the existing seeded-anvil script so USDC/USDT
     balances are pre-funded.
   - `run_advisor(prompt, context_path, rpc) -> AdvisorOutput`: runs the
     compiled `advisor` binary via `env!("CARGO_BIN_EXE_advisor")` with
     `--network anvil --simulate --rpc <url> --json`, parses stdout into
     a `CompileOutputJson` + simulation deltas.
   - Skip-not-fail when `OPENAI_API_KEY` is unset or `anvil`/`cast` are
     missing on PATH.

2. Tests (`crates/intent-script/tests/advisor_e2e.rs`, `#[ignore]`-gated):
   - `advisor_e2e_wraps_eth_to_weth`: prompt `"wrap 1 ETH to WETH"` against
     `examples/advisor-context.json`; asserts a single `deposit()` call to
     WETH9 and a +1e18 WETH delta on the advisor wallet.
   - `advisor_e2e_deposit_usdc_into_aave`: prompt
     `"deposit 5000 USDC into aave"`; asserts a USDC `approve()` then an
     Aave V3 `supply()`, and a positive `aUSDC` delta on the advisor
     wallet (recipient-pinning invariant — must be the signer).

3. Cargo wiring:
   - New `[[test]] name = "advisor_e2e" required-features = ["advisor"]`
     in `crates/intent-script/Cargo.toml`, so default `cargo test` stays
     no_std-clean and only `cargo test --features advisor -- --ignored`
     pulls these in.

4. Makefile target:
   - `make test-e2e-advisor` → runs the suite with `--ignored`.

5. CI integration (later, in WS-3A-IS): the CI workflow can opt in to this
   job with a foundry-bootstrap step + `OPENAI_API_KEY` from secrets.

## Files

- `intent-script/crates/intent-script/Cargo.toml` (new `[[test]]` entry)
- `intent-script/crates/intent-script/tests/advisor_e2e.rs` (new)
- `intent-script/crates/intent-script/tests/common/mod.rs` (add `pub mod e2e;`)
- `intent-script/crates/intent-script/tests/common/e2e.rs` (new)
- `intent-script/makefile` (new `test-e2e-advisor` target)

## Acceptance criteria

- [ ] `make test-e2e-advisor` locally starts anvil, runs both cases, passes.
- [ ] Each test asserts at least one on-chain state delta (token balance).
- [ ] Advisor flow asserts the recommended deposit's recipient is the
      signer (recipient pinning invariant).
- [ ] Default `cargo test -p intent-script` (no `--features advisor`) still
      compiles and passes — new tests are not pulled in.
- [ ] Skip-not-fail when `OPENAI_API_KEY` / `anvil` / `cast` are unavailable.
- [ ] `intent-script/makefile` documents the new target inline.
