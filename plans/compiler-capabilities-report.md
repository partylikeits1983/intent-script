# Intent-Script Compiler — Capabilities Report

## Supported DeFi Protocols

| Protocol | Version | Actions | On-Chain Contract |
|----------|---------|---------|-------------------|
| **Uniswap** | V3 | `swap` (exactInputSingle) | SwapRouter `0xE592427A0AEce92De3Edee1F18E0157C05861564` |
| **Aave** | V3 | `deposit` (supply), `borrow`, `withdraw` | Pool `0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2` |
| **Lido** | — | `stake` (ETH→stETH), `wrap` (stETH→wstETH) | stETH `0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84`, wstETH `0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0` |
| **WETH9** | — | `wrap` (ETH→WETH), `unwrap` (WETH→ETH) | WETH `0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2` |
| **1inch** | Fusion v6 | `swap` (calldata passthrough) | Router `0x111111125421cA6dc452d289314280a0f8842A65` |

### Network Support

- **Ethereum mainnet** (chain ID 1) — fully configured with assets and protocol addresses
- Sepolia, Base, Arbitrum — chain configs exist but no asset/protocol configs yet

---

## Supported Transaction Flows via IntentRouter

The `IntentRouter` contract batches multiple EVM calls into a single atomic transaction. The compiler automatically inserts `transferFrom`, `approve`, and token sweep steps.

### Single-Step Flows (no router needed)

| Flow | JSON Step | Description |
|------|-----------|-------------|
| Wrap ETH | `{ "wrap": { "asset": "ETH", "amount": "1.0" } }` | WETH9.deposit() |
| Unwrap WETH | `{ "unwrap": { "asset": "WETH", "amount": "1.0" } }` | WETH9.withdraw() |
| Stake ETH in Lido | `{ "stake": { "asset": "ETH", "amount": "10.0", "into": "lido" } }` | Lido.submit() |

### Multi-Step Batched Flows (via IntentRouter)

| Flow | Steps | Auto-Inserted by Compiler |
|------|-------|---------------------------|
| **Swap** | swap | transferFrom + approve + exactInputSingle + sweep |
| **Deposit into Aave** | deposit | transferFrom + approve + supply |
| **Deposit + Borrow** | deposit → borrow | transferFrom + approve + supply + borrow + sweep(borrowed) |
| **Swap + Deposit** | swap → deposit | transferFrom + approve + swap(→router) + approve + supply |
| **Swap + Deposit + Borrow** | swap → deposit → borrow | transferFrom + approve + swap(→router) + approve + supply + borrow + sweep(WETH,DAI) |
| **Stake + Wrap** | stake → wrap(stETH) | stake(ETH→stETH) + approve(stETH→wstETH) + wrap + sweep(wstETH) |
| **Withdraw from Aave** | withdraw | withdraw (user must have existing position) |

### Key Compiler Behaviors

- **Token routing**: When batching, swap output goes to the router (not the user). Subsequent steps consume tokens already in the router without needing another `transferFrom`.
- **Token sweeping**: After all calls execute, the router sweeps remaining ERC-20 balances back to the user.
- **ETH refund**: Any leftover ETH in the router is refunded to the user.
- **Credit delegation**: Aave V3 borrows through the router require the user to `approveDelegation` to the router off-chain (not handled by the compiler).

---

## Most Complex Single-Transaction Intent Flow

The most complex usable intent flow that can be executed in a single atomic transaction:

### Swap → Deposit → Borrow (6 internal calls)

```json
{
  "network": "ethereum",
  "from": "0xUserAddress",
  "steps": [
    { "swap": { "from": "USDC", "amount": "5000", "to": "WETH", "min_amount_out": "0.5" } },
    { "deposit": { "asset": "WETH", "amount": "2.0", "into": "aave" } },
    { "borrow": { "asset": "DAI", "amount": "1000", "from": "aave" } }
  ]
}
```

**What the compiler produces (6 batched calls):**

1. `USDC.transferFrom(user, router, 5000e6)` — pull USDC from user into router
2. `USDC.approve(UniswapRouter, 5000e6)` — router approves Uniswap to spend USDC
3. `UniswapRouter.exactInputSingle(USDC→WETH, recipient=router, minOut=0.5 WETH)` — swap, output stays in router
4. `WETH.approve(AavePool, 2e18)` — router approves Aave to spend WETH (already in router from swap)
5. `AavePool.supply(WETH, 2e18, onBehalfOf=user)` — deposit WETH as collateral for the user
6. `AavePool.borrow(DAI, 1000e18, variableRate, onBehalfOf=user)` — borrow DAI against the collateral

**After execution, the router sweeps:**
- Excess WETH (from swap output minus 2.0 deposited) → back to user
- Borrowed DAI (Aave sends to msg.sender=router) → back to user

**Prerequisites (user must do before submitting):**
- `USDC.approve(router, 5000e6)` — allow router to pull USDC
- `variableDebtDAI.approveDelegation(router, 1000e18)` — allow router to borrow on user's behalf

### Theoretical Maximum Complexity

The router has no limit on the number of calls. Theoretically, a single transaction could chain:

```
swap(A→B) → swap(B→C) → deposit(C, aave) → borrow(D, aave) → swap(D→E) → deposit(E, aave) → borrow(F, aave) → ...
```

Each additional step adds ~2-3 internal calls (approve + protocol call + optional transferFrom). The practical limit is the block gas limit (~30M gas on Ethereum mainnet). A 6-call batch like the swap+deposit+borrow above uses ~700K gas, so roughly **~250 internal calls** could fit in a single block.

---

## Execution Modes

| Mode | Condition | Output |
|------|-----------|--------|
| **SingleTx** | 1 call (e.g., wrap ETH) | Single unsigned transaction |
| **Eip712Intent** | 2+ calls with router configured | Batched `executeDirect()` tx + EIP-712 typed data for `executeSigned()` |
| **TxSequence** | 2+ calls, no router | Multiple unsigned transactions (user signs each) |

### EIP-712 Signed Execution

For batched intents, the compiler produces EIP-712 typed data that a relayer/solver can submit via `executeSigned()`. This enables gasless execution where a third party pays gas on behalf of the user. Replay protection is provided via nonces and deadlines.

---

## 1inch Fusion Swaps (Off-Chain)

1inch swaps are **not** executed through the IntentRouter. The compiler outputs the swap as a separate signed intent that the frontend propagates to the 1inch Fusion API. The `calldata` field in the JSON is pre-fetched by the frontend from the 1inch API. Slippage for 1inch swaps is handled by the Fusion protocol, not by the compiler.
