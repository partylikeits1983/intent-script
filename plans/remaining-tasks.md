# Remaining Tasks

This document describes the remaining work from the [implementation plan](./implementation-plan.md). **All phases are now complete.** The codebase compiles, all 35 Rust tests pass (`cargo test -p intent-script`), and all 23 Foundry tests pass (`cd contracts && forge test`).

## What Was Completed

### Phase 1: New Protocol Adapters ✅
- **Task 1.1**: wstETH wrapping support — `WstETHWrap` IR variant, normalize/enrich/adapter/config changes
- **Task 1.2**: Optional fee tier for Uniswap V3 swaps — `fee` field on `SwapStep`
- **Task 1.3**: 1inch swap adapter — `via`/`calldata` fields, `OneInchSwap` IR variant, `oneinch.rs` adapter
- **Task 1.4**: ERC-20 permit infrastructure — `Erc20Permit` IR variant, `lower_permit()` in `erc20.rs`

### Phase 2: EIP-712 Router Upgrade ✅
- **Task 2.1**: New `IntentRouter.sol` with `executeDirect` + `executeSigned` (EIP-712)
- **Task 2.2**: All Foundry tests updated for `executeDirect`, 6 new `executeSigned` tests added
- **Task 2.3**: `eip712.rs` module with domain separator, struct hashing, typed data hash
- **Task 2.4**: New output types: `Eip712IntentOutput`, `Eip712Domain`, `IntentBatchData`, `CallData`
- **Task 2.5**: Build stage produces `Eip712Intent` for batched plans (with `directTx` for self-execution)
- **Task 2.6**: All integration tests updated for `Eip712Intent` output type
- **Task 2.7**: Fixtures regenerated, full end-to-end pipeline verified

### Phase 3: Polish & Examples ✅
- **Task 3.1**: All example JSON files created in `crates/intent-script/examples/`
- **Task 3.2**: CoW Swap stub — **removed from scope** (off-chain intents handled by frontend/solver, not the compiler)

### Additional Fixes
- **evm-testing crate**: Fixed `extract_txs()` in `helpers.rs` to handle `CompileOutput::Eip712Intent` variant

## Example Files

All example files in `crates/intent-script/examples/` compile successfully:

| File | Description |
|------|-------------|
| `wrap_eth.json` | Wrap ETH → WETH |
| `aave_deposit.json` | Deposit USDC into Aave |
| `aave_borrow.json` | Deposit USDC + Borrow DAI |
| `aave_withdraw.json` | Withdraw USDC from Aave |
| `swap_uniswap.json` | Uniswap V3 swap (USDC → WETH) |
| `swap_1inch.json` | 1inch swap with pre-fetched calldata |
| `stake_lido.json` | Stake ETH → stETH via Lido |
| `stake_lido_wsteth.json` | Full flow: ETH → stETH → wstETH |
| `complex_defi.json` | Multi-step: swap → deposit → borrow |

## Verification Commands

```bash
# Rust tests (all 35 pass)
cargo test -p intent-script

# Foundry tests (all 23 pass)
cd contracts && forge test

# Test an example file
cargo run -p intent-script -- crates/intent-script/examples/wrap_eth.json -c ./config -p

# Full workspace build (evm-testing included)
cargo test --workspace  # Note: test_unwrap_weth_on_anvil is a known Anvil environment issue
```

## Known Issues

- `test_unwrap_weth_on_anvil` in `evm-testing` fails due to an Anvil fork environment bug (WETH `withdraw()` gas stipend issue). See [`plans/issues/weth-withdraw-anvil-revert.md`](./issues/weth-withdraw-anvil-revert.md). The compiler output is correct; only the Anvil test environment is affected.
