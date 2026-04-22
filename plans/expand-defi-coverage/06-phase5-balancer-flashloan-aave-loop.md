# Sub-Task 06 — Phase 5: Balancer Flashloan + Aave Looping

## Context

Enable leveraged loops (e.g. 3× ETH on Aave) by adding Balancer V2 flashloans as a nested step type. This is the biggest refactor in the overall plan because the enricher must now run **recursively**: outer pipeline → flashloan step → inner pipeline with its own transferFrom/approve auto-insertion → outer repayment.

Balancer is the chosen provider because V2 Vault flashloans are **0% fee**, making loops free apart from gas.

## Prerequisites

- Sub-task 02 complete (router reentrancy guard in place).
- Sub-task 03 complete (`step_produces` is fee-aware so we can validate repayability).

## ⚠ Read these corrections first (from `00-corrections.md`)

- **§4**: no solc pragma bump needed. `foundry.toml` already pins `0.8.28`.
- **§5**: use only the boolean-sentinel guard, not the cookie variant that the parent plan half-sketched.
- **§6**: the IR variant for a flashloan must carry `inner_steps: Vec<ResolvedStep>`, NOT `inner_calls: Vec<ConcreteCall>`. Lowering happens after enrichment.

## Files to read first

- `contracts/src/IntentRouter.sol` — updated router with reentrancy guard.
- `crates/intent-script/src/compiler/enrich.rs` — understand the single-pass enrich flow; you'll add recursion.
- `crates/intent-script/src/compiler/lower.rs` — the lowering pass.
- Balancer V2 Vault source (docs or GitHub) — for `flashLoan` signature and `receiveFlashLoan` repayment semantics (Vault transfers tokens to recipient, then calls back, then checks `balanceOf(address(this)) >= amountsOwed`).

## Implementation

### 6.1 Router callback (`contracts/src/IntentRouter.sol`)

```solidity
address public constant BALANCER_VAULT = 0xBA12222222228d8Ba445958a75a0704d566BF2C8;

// EIP-1153 transient-storage slot
bytes32 private constant FLASHLOAN_GUARD_SLOT = keccak256("intent.flashloan.guard");

function receiveFlashLoan(
    address[] calldata tokens,
    uint256[] calldata amounts,
    uint256[] calldata feeAmounts,
    bytes calldata userData
) external {
    require(msg.sender == BALANCER_VAULT, "not vault");

    bytes32 guard;
    bytes32 slot = FLASHLOAN_GUARD_SLOT;
    assembly { guard := tload(slot) }
    require(guard != bytes32(0), "no flashloan in progress");
    assembly { tstore(slot, 0) }  // clear before executing inner calls

    Call[] memory innerCalls = abi.decode(userData, (Call[]));

    for (uint256 i = 0; i < innerCalls.length; i++) {
        require(allowedTargets[innerCalls[i].target], "Target not allowed");
        (bool ok, bytes memory ret) = innerCalls[i].target.call{value: innerCalls[i].value}(innerCalls[i].callData);
        if (!ok) assembly { revert(add(ret, 32), mload(ret)) }
    }

    // Repay: Balancer Vault checks `balanceOf(address(this)) >= pre + feeAmounts[i]`.
    // Transfer the owed amount back explicitly.
    for (uint256 i = 0; i < tokens.length; i++) {
        uint256 owed = amounts[i] + feeAmounts[i];
        require(IERC20(tokens[i]).transfer(BALANCER_VAULT, owed), "flashloan repay fail");
    }
}
```

**Setting the guard:** inside `_executeCalls`, before each `.call`, detect calls whose target is `BALANCER_VAULT` and selector matches `flashLoan(address,address[],uint256[],bytes)`. Immediately before, `tstore(FLASHLOAN_GUARD_SLOT, bytes32(uint256(1)))`. After the call returns, the guard is already cleared by `receiveFlashLoan`.

```solidity
bytes4 constant FLASHLOAN_SELECTOR = 0x5c38449e;  // flashLoan(address,address[],uint256[],bytes)

// Inside _executeCalls, just before the call:
if (calls[i].target == BALANCER_VAULT
    && calls[i].callData.length >= 4
    && bytes4(calls[i].callData[:4]) == FLASHLOAN_SELECTOR) {
    bytes32 slot = FLASHLOAN_GUARD_SLOT;
    assembly { tstore(slot, 1) }
}
```

Verify the selector by computing `cast sig "flashLoan(address,address[],uint256[],bytes)"` before pasting.

### 6.2 Config

`config/protocols/ethereum.json`:
```json
"balancer": {
  "type": "flashloan_provider", "version": "v2",
  "contracts": { "vault": "0xBA12222222228d8Ba445958a75a0704d566BF2C8" }
}
```

### 6.3 DSL

```json
{ "flashloan": {
    "via": "balancer",
    "assets": [{ "asset": "WETH", "amount": "2.0" }],
    "then": [
      { "deposit": { "asset": "WETH", "amount": "3.0", "into": "aave" } },
      { "borrow":  { "asset": "USDC", "amount": "5500", "from": "aave" } },
      { "swap":    { "from": "USDC", "amount": "all", "to": "WETH", "min_amount_out": "2.0" } }
    ]
  }
}
```

### 6.4 Schema

```rust
pub enum Step { …, Flashloan(FlashloanStep) }

pub struct FlashloanStep {
    pub via: String,             // "balancer"
    pub assets: Vec<FlashloanAsset>,
    pub then: Vec<Step>,         // bounded to ≤5 elements
}

pub struct FlashloanAsset { pub asset: String, pub amount: String }
```

### 6.5 IR (note: `inner_steps`, NOT `inner_calls`)

```rust
pub enum ResolvedStep { …,
    BalancerFlashloan {
        vault: Address,
        tokens: Vec<Address>,
        amounts: Vec<U256>,
        inner_steps: Vec<ResolvedStep>,  // ← see corrections §6
    },
}
```

### 6.6 Normalize

- Resolve `via="balancer"` → vault address from registry.
- Resolve each `asset`/`amount` in `assets`.
- Recursively normalize `then:` — produces `Vec<ResolvedStep>`. Inner normalization runs with the same `ResolvedIntent` context (same signer, same chain).

### 6.7 Validate

- Reject nested flashloans (an inner step being itself a `Flashloan`).
- Build `produced_by_inner[token]` by summing `step_produces(s, fee_bps)` over `inner_steps`.
- For each flashloaned `(token, amount)`: require `produced_by_inner[token] >= amount`. Otherwise `CompileError::Validation("Flashloan not repayable: ...")`.
- Enforce `then.len() ≤ 5`.
- Enforce depth = 1.

### 6.8 Enrich (recursive)

Create a helper `enrich_inner(steps, fresh_context)`:

1. Initial context: `tokens_in_router` pre-populated with the flashloan tokens (Balancer sends tokens to router before calling back).
2. Run the existing enrich logic on `inner_steps` with this seeded context.
3. Auto-append nothing at the end — repayment happens in Solidity (`receiveFlashLoan` transfers back). Validation already proved the inner pipeline leaves enough token balance.

At the outer level:
- The `BalancerFlashloan` step itself is a single concrete call to `vault.flashLoan`.
- No sweep for the flashloaned tokens (they end the inner sequence owed to Vault).
- Any *excess* tokens left after repayment (e.g. swap dust) should still be added to outer `sweep_tokens`.

### 6.9 Lower

```rust
ResolvedStep::BalancerFlashloan { vault, tokens, amounts, inner_steps } => {
    // First lower inner_steps to Vec<ConcreteCall>:
    let inner_calls: Vec<ConcreteCall> = inner_steps.iter()
        .map(|s| lower_step(s, registry))
        .collect::<Result<Vec<_>>>()?
        .into_iter().flatten().collect();
    // Encode userData = abi.encode(Call[])
    let user_data = encode_call_array(&inner_calls);
    // Build outer flashLoan call
    balancer::lower_flashloan(vault, tokens, amounts, &user_data, registry.router_address())
}
```

### 6.10 Adapter `adapters/balancer.rs` (NEW)

```rust
alloy_sol_types::sol! {
    function flashLoan(
        address recipient,
        address[] tokens,
        uint256[] amounts,
        bytes userData
    ) external;
}

pub fn lower_flashloan(
    vault: &Address,
    tokens: &[Address],
    amounts: &[U256],
    user_data: &Bytes,
    router: Address,
) -> Result<Vec<ConcreteCall>> {
    let calldata = flashLoanCall {
        recipient: router,
        tokens: tokens.to_vec(),
        amounts: amounts.to_vec(),
        userData: user_data.clone(),
    }.abi_encode();
    Ok(vec![ConcreteCall {
        to: *vault,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,
        …
    }])
}
```

`recipient` is the IntentRouter address. Thread it via the `registry` (which has the router address) or add it to the `ResolvedStep::BalancerFlashloan` variant — prefer threading through registry.

### 6.11 Tests

Rust:
- `tests/flashloan_tests.rs` (NEW):
  - `test_flashloan_rejects_unrepayable`
  - `test_flashloan_rejects_nested`
  - `test_flashloan_inner_enrich_preserves_tokens_in_router`
  - `test_flashloan_repayment_transfer_encoded_correctly`
- `tests/integration.rs`: `test_balancer_flashloan_simple` (assert outer call targets vault, inner userData decodes to expected Call[]).
- `tests/generate_calldata.rs`: fixture `loop_aave_3x_weth.txt`.

Foundry:
- `contracts/test/IntentRouterFlashloan.t.sol` (NEW): fork mainnet, 3× leverage loop on Aave, assert final HF > 1.5 and user's net position matches.
- Add BALANCER_VAULT + Aave pool to allowlist in this test.

## Definition of done

- [ ] `make test && make test-foundry` green.
- [ ] `ETH_RPC_URL=… make test-fork-e2e` passes including the new flashloan fork test.
- [ ] `BalancerFlashloan` IR carries `Vec<ResolvedStep>`, and enrich runs recursively.
- [ ] Validation rejects nested flashloans and unrepayable inner pipelines.
- [ ] Sentinel guard in router: set just before flashLoan, checked + cleared in callback.

## Verification

```bash
cd /Users/riemann/Desktop/intentOS/intent-script
make test && make test-foundry
ETH_RPC_URL=… make test-fork-e2e
```

## Hand-off to sub-task 07

- LP sub-task (07) is independent of flashloans — it only depends on sub-task 02 (router had ERC721 receiver added there).
- If sub-tasks 06 and 07 run serially, sub-task 07 doesn't need to re-read this file.
