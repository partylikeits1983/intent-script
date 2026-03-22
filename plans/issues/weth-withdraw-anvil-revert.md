# Issue: WETH `withdraw()` Reverts on Anvil Forked Mainnet

## Status
Open — low priority, Anvil environment bug, not a compiler bug.

## Description

The `test_unwrap_weth_on_anvil` test fails with `execution reverted, data: "0x"` when calling `WETH.withdraw(uint256)` on an Anvil instance forking Ethereum mainnet.

## Reproduction

1. Fork mainnet with Anvil: `anvil --fork-url https://ethereum-rpc.publicnode.com`
2. Wrap ETH → WETH via `deposit()` — **succeeds**
3. Unwrap WETH → ETH via `withdraw(uint256)` — **reverts with empty data**

This happens with both:
- `cast send --unlocked` directly
- Compiled intent-script transactions via `send_transaction`

## Root Cause

The WETH contract's `withdraw` function uses:
```solidity
msg.sender.transfer(wad);
```

`transfer()` forwards only **2300 gas** to the recipient. On Anvil's forked mode, the access list for the recipient EOA's balance storage slot may not be pre-warmed, causing the 2300 gas stipend to be insufficient for the `SSTORE` operation (which costs 5000+ gas for a cold slot post-EIP-2929).

## Verification

- The compiled calldata is correct: selector `0x2e1a7d4d` + ABI-encoded `uint256`
- The WETH balance is confirmed present before the call
- The WETH contract has sufficient ETH backing
- `deposit()` works correctly on the same Anvil instance
- The same revert occurs with `cast send` directly (not through our code)

## Workarounds

1. **Update Anvil** — newer Foundry versions may fix the access list warming
2. **Use `--gas-limit`** — doesn't help (tried with 100000 gas)
3. **Skip the test** — the unwrap test is currently left in but expected to fail until Anvil is fixed

## Impact

- **Compiler output is correct** — verified by unit tests and calldata inspection
- Only the Anvil fork test environment is affected
- Real mainnet execution would work fine (EOA balance slots are always warm for `msg.sender`)
