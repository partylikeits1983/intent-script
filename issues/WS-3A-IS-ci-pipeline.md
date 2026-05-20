# [WS-3A-IS] CI pipeline for intent-script

**Repo:** `partylikeits1983/intent-script`
**Labels:** `area/ci`, `size/S`
**Depends on:** none

## Context

The compiler + contracts have strong local tests (`makefile`, 17 integration test files, Foundry suite) but no CI. Add it.

## Scope

1. Add `.github/workflows/ci.yml` with jobs:
   - `fmt` — `cargo fmt --all -- --check`
   - `clippy` — `cargo clippy --all-targets -- -D warnings`
   - `test` — `cargo test -p intent-script`
   - `test-evm` — `cargo test -p evm-testing` (needs anvil available via `foundry-rs/foundry-toolchain` action; fork URL from a repo secret `ETH_RPC_URL`)
   - `forge-test` — `forge test` in `contracts/`
   - `wasm-build` — `wasm-pack build --target web` on `crates/intent-script-wasm` for the browser UI
   - `server-compile-compat` — compile the Rust crate as a library dependency for `intentOS-server` (WS-1C), without requiring a Node WASM target
2. Caching: cargo registry, target dir, foundry.
3. `make ci` convenience target that runs the same sequence locally.
4. Status badge in README (or root-level once we have one).

## Files

- `intent-script/.github/workflows/ci.yml` (new)
- `intent-script/makefile` — add `ci` target
- `intent-script/README.md` — badge (if/when README is added in its own issue)

## Acceptance criteria

- [ ] Every PR triggers the workflow.
- [ ] All jobs pass on `feat/expand-defi-lido-queue-and-uni-v3-lp` (current branch) after this PR.
- [ ] `ETH_RPC_URL` secret documented in the PR body and in the workflow file.
- [ ] `make ci` runs locally end-to-end on a clean checkout.
