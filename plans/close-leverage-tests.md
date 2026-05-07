# Close-leverage tests + example JSONs

## Context

`intent-script` already supports `close_position` as a first-class step (compiler logic in `crates/intent-script/src/compiler/leverage.rs:335-472`). It desugars into a Balancer V2 flashloan wrapping (1) Aave V3 repay → (2) Aave V3 withdraw → (3) Uniswap V3 swap collateral→borrow that must produce ≥ `current_debt` to repay the flashloan.

What was missing was symmetry with the open-side leverage tests: explicit close-long and close-short tests anchored to the same scenarios as `test_long_5x_eth_accepts` and `examples/short_eth_3x.json`, plus user-facing example JSONs documenting the close flow.

## Changes landed

### `crates/intent-script/tests/integration.rs`

- Removed the generic `test_close_position_compiles` (was a strict subset of the new close-long: same shape, weaker `let _ = result;` assertion, unanchored state numbers).
- Added `test_close_long_5x_eth_accepts` — closes a 5x WETH/USDC long. State (`current_collateral=5.0`, `current_debt=12864.0`) is derived from the open at `test_long_5x_eth_accepts` (1.0 WETH × 5 leverage; 4.0 WETH flashloan × 3200 price × 1.005 slippage). Asserts the Balancer vault `0xBA12222222228d8Ba445958a75a0704d566BF2C8` appears in the compiled `intent_batch.calls`, mirroring the open-side assertion.
- Added `test_close_short_3x_eth_accepts` — closes a 3x USDC/WETH short. State (`current_collateral=30000.0`, `current_debt=5.714286`) is derived from `examples/short_eth_3x.json` (10000 USDC × 3 leverage; 20000 USDC borrowed / 3500 price). Same Balancer-vault assertion.
- Kept `test_close_position_requires_state` — covers the zero-state validation branch.

### Example JSONs in `crates/intent-script/examples/`

- `close_long_eth_from_5x.json` — mirrors `long_eth_5x.json` style, documents close of a 5x WETH/USDC long.
- `close_short_eth_from_3x.json` — mirrors `short_eth_3x.json` style, documents close of a 3x USDC/WETH short.

Both use `network: "ethereum"` per the convention (anvil is reserved for tests). Examples are referenced explicitly from `tests/generate_calldata.rs`, not auto-discovered, so the new JSONs require no test registration.

## Verification

All passed locally:

- `cargo test -p intent-script --test integration test_close` → 3/3 (`test_close_position_requires_state`, `test_close_long_5x_eth_accepts`, `test_close_short_3x_eth_accepts`).
- `cargo test -p intent-script --test integration test_long` → 3/3 (no regression).
- `cargo test -p intent-script --test integration test_short` → 1/1 (no regression).

## Files touched

- `crates/intent-script/tests/integration.rs`
- `crates/intent-script/examples/close_long_eth_from_5x.json` (new)
- `crates/intent-script/examples/close_short_eth_from_3x.json` (new)
