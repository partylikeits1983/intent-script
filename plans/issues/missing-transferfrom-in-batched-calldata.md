# Issue: Compiler Does Not Generate `transferFrom` to Pull Tokens Into Router

## Status
**Fixed** — see [plans/fix-missing-transferfrom.md](../fix-missing-transferfrom.md).

## Severity
**High** — the compiler-generated calldata for batched router execution is incomplete and will revert on mainnet.

## Description

When the compiler batches multiple calls through the `IntentRouter` via `executeDirect()`, it generates `approve` + protocol calls (e.g., `approve(Uniswap, amount)` + `exactInputSingle(...)`) but does **not** generate a `transferFrom(user, router, amount)` call to first pull the user's ERC-20 tokens into the router.

The router executes calls on behalf of the user. When the router calls `approve(Uniswap, amount)`, it approves Uniswap to spend tokens held by **the router** (since `msg.sender` in the approve is the router). But the router doesn't have the tokens — the user does.

## Expected Behavior

For any batched intent that uses ERC-20 tokens (swap, deposit, etc.), the compiler should generate:

1. `transferFrom(user, router, amount)` — pull tokens from user to router
2. `approve(protocol, amount)` — router approves the protocol
3. Protocol call (e.g., `exactInputSingle(...)`) — router calls the protocol

## Actual Behavior

The compiler generates only:

1. `approve(protocol, amount)` — router approves the protocol  
2. Protocol call — router calls the protocol

The `transferFrom` step is missing, so the protocol tries to pull tokens from the router which has zero balance, causing `ERC20: transfer amount exceeds balance` reverts.

## Reproduction

```bash
# Generate fixtures and run fork tests
make generate-fixtures
cd contracts && forge test --mc IntentForkE2E --fork-url https://ethereum-rpc.publicnode.com -vvv
```

5 of 7 fork tests fail with `revert: STF` (Uniswap's "Safe Transfer Failed") or similar ERC-20 balance errors. The 2 passing tests (`test_fork_wrapETH` and `test_fork_stakeETH_lido`) work because they use native ETH (sent as `msg.value`), not ERC-20 tokens.

## Root Cause

In [`crates/intent-script/src/compiler/enrich.rs`](../../crates/intent-script/src/compiler/enrich.rs), the enrich stage inserts `Erc20Approve` steps before protocol calls but never inserts a `transferFrom` to move tokens from the user to the router.

For example, the `UniswapV3Swap` case (line 41-63):
```rust
ResolvedStep::UniswapV3Swap { router: swap_router, token_in, amount_in, .. } => {
    // Inserts approve — but no transferFrom!
    enriched_steps.push(ResolvedStep::Erc20Approve {
        token: *token_in,
        spender: *swap_router,
        amount: *amount_in,
    });
    enriched_steps.push(step.clone());
}
```

The same issue affects `AaveV3Supply` (line 25-39) and any other step that requires ERC-20 tokens.

## Evidence

The local mock tests in [`contracts/test/IntentForkTests.t.sol`](../../contracts/test/IntentForkTests.t.sol) manually add the `transferFrom` step (see lines 81-87 for the swap test), which is why they pass. The compiler should generate this automatically.

Example from the working mock test:
```solidity
// Step 0: transferFrom USDC from user to router (MISSING from compiler output)
calls[0] = IntentRouter.Call({
    target: address(usdc),
    callData: abi.encodeWithSignature(
        "transferFrom(address,address,uint256)",
        user, address(router), usdcAmount
    ),
    value: 0
});
// Step 1: approve swap router (compiler generates this)
// Step 2: swap (compiler generates this)
```

## Fix

In `enrich.rs`, when a router is configured and the step requires ERC-20 tokens, insert a `transferFrom(signer, router, amount)` call before the approve. This needs a new `ResolvedStep` variant (e.g., `Erc20TransferFrom`) or reuse of an existing mechanism.

The fix should:
1. Add a `ResolvedStep::Erc20TransferFrom { token, from, to, amount }` variant
2. In `enrich.rs`, when `router.is_some()`, insert `Erc20TransferFrom` before `Erc20Approve` for steps that consume ERC-20 tokens
3. Add a corresponding `lower_transfer_from` in the adapters
4. Ensure the user's external `approve(router, amount)` is documented as a prerequisite

## Affected Tests

Once this is fixed, the fork E2E tests in [`contracts/test/IntentForkE2E.t.sol`](../../contracts/test/IntentForkE2E.t.sol) should pass without modification:
- `test_fork_swapUSDC_WETH` 
- `test_fork_aaveDepositUSDC`
- `test_fork_depositBorrow`
- `test_fork_complexDefi_executeDirect`
- `test_fork_complexDefi_executeSigned`

The following tests already pass (they use native ETH, not ERC-20):
- `test_fork_wrapETH` ✅
- `test_fork_stakeETH_lido` ✅
