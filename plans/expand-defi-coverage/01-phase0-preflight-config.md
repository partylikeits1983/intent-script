# Sub-Task 01 — Phase 0: Pre-flight + Config Scaffolding

**Status: ✅ COMPLETE (2026-04-22).** This file is kept for traceability. Do not re-run.

## Context

Before any compiler or contract changes can land, two things must be true:

1. Baseline tests pass cleanly so later diffs have a green reference.
2. `config/chains.json`, `config/assets/ethereum.json`, and `config/protocols/ethereum.json` exist — every later sub-task's configuration changes target these files.

## What was done

1. Ran `make test` from `intent-script/` — all 29 Foundry tests and 72 cargo tests passed.
2. Created `config/assets/ethereum.json` by copying `config/assets/anvil.json` verbatim (anvil forks mainnet, so the addresses are already mainnet addresses).
3. Created `config/protocols/ethereum.json` by copying `config/protocols/anvil.json` verbatim.
4. Added an `"ethereum"` entry to `config/chains.json`:
   ```json
   "ethereum": {
     "chain_id": 1,
     "native_asset": "ETH",
     "wrapped_native": "WETH"
   }
   ```
5. Smoke-tested by compiling a `"network": "ethereum"` wrap-ETH intent via the CLI — output reported `chain_id: 1` and produced valid calldata.
6. Re-ran `cargo test -p intent-script` — still 72 passed.

## Verification (already run)

```bash
cd /Users/riemann/Desktop/intentOS/intent-script
ls config/assets/ethereum.json config/protocols/ethereum.json    # both exist
jq '.ethereum' config/chains.json                                 # non-null
cargo run -p intent-script --features clap -- \
    <(echo '{"network":"ethereum","from":"0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045","steps":[{"wrap":{"asset":"ETH","amount":"1"}}]}') \
    --pretty
# → reports chain_id: 1
```

## Hand-off to sub-task 02

- Baseline is green. Any red test in sub-task 02+ is a regression owned by that sub-task.
- No `contracts/` or compiler source changes have been made. Only config files and this plans directory.
- The `intent_router` block in `config/protocols/ethereum.json` is a placeholder address copied from anvil.json. **Sub-task 02 will overwrite it** with the real deployment address once the updated router is deployed (or leave as-is if deployment is out of scope for the sub-task).
