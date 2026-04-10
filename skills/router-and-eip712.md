# IntentRouter Contract & EIP-712 Signing

> **Load this file** when you need to understand the on-chain router contract, EIP-712 typed data signing, execution modes, or the allowlist mechanism.

## IntentRouter Contract

**File:** `contracts/src/IntentRouter.sol`

The router is a minimal Solidity contract that batches multiple EVM calls into a single atomic transaction and sweeps tokens back to the user.

### Entry Points

| Function | Who Calls | Authorization | Use Case |
|----------|-----------|---------------|----------|
| `executeDirect(calls, tokensToSweep)` | User directly | `msg.sender` | Self-execution, no signature needed |
| `executeSigned(batch, signature)` | Relayer/solver | EIP-712 signature | Gasless execution, solver pays gas |

### Contract State

```solidity
bytes32 public immutable DOMAIN_SEPARATOR;  // EIP-712 domain, set in constructor
mapping(address => uint256) public nonces;  // per-signer replay protection
address public owner;                        // allowlist admin
mapping(address => bool) public allowedTargets; // target contract allowlist
```

### Execution Flow

```
User/Relayer → executeDirect/executeSigned
    → _executeCalls(calls)           // loop: call each target with calldata + value
        → require(allowedTargets[target])  // allowlist check
        → target.call{value}(calldata)     // low-level call
    → _sweep(tokensToSweep, recipient)  // transfer ERC-20 balances to user
    → _refundETH(recipient)             // send remaining ETH to user
```

### Allowlist (Security)

The router only calls contracts on the allowlist. This prevents arbitrary code execution.

```solidity
function setAllowedTarget(address target, bool allowed) external onlyOwner;
function setAllowedTargets(address[] calldata targets, bool allowed) external onlyOwner;
```

In `_executeCalls()`:
```solidity
require(allowedTargets[calls[i].target], "Target not allowed");
```

**Important for tests:** Every Foundry test `setUp()` must whitelist all contracts the test uses via `router.setAllowedTarget(address, true)`.

### Sweep Mechanism

After all calls execute, the router transfers any remaining ERC-20 token balances back to the user:

```solidity
function _sweep(address[] calldata tokens, address recipient) internal {
    for (uint256 i = 0; i < tokens.length; i++) {
        uint256 balance = IERC20(tokens[i]).balanceOf(address(this));
        if (balance > 0) {
            IERC20(tokens[i]).transfer(recipient, balance);
        }
    }
}
```

Any remaining ETH is also refunded:
```solidity
function _refundETH(address recipient) internal {
    uint256 ethBalance = address(this).balance;
    if (ethBalance > 0) {
        (bool sent,) = recipient.call{value: ethBalance}("");
        require(sent, "ETH refund failed");
    }
}
```

---

## EIP-712 Typed Data

### Domain Separator

```solidity
EIP712Domain {
    name: "IntentRouter",
    version: "1",
    chainId: block.chainid,
    verifyingContract: address(this)
}
```

Computed in the constructor and stored as `DOMAIN_SEPARATOR`.

### Type Hashes

```solidity
CALL_TYPEHASH = keccak256("Call(address target,bytes callData,uint256 value)");

INTENT_BATCH_TYPEHASH = keccak256(
    "IntentBatch(address signer,Call[] calls,address[] tokensToSweep,uint256 nonce,uint256 deadline)"
    "Call(address target,bytes callData,uint256 value)"
);
```

### Structs

```solidity
struct Call {
    address target;
    bytes callData;
    uint256 value;
}

struct IntentBatch {
    address signer;
    Call[] calls;
    address[] tokensToSweep;
    uint256 nonce;
    uint256 deadline;
}
```

### Hashing (Solidity side)

```solidity
// Hash a single call
keccak256(abi.encode(CALL_TYPEHASH, call.target, keccak256(call.callData), call.value))

// Hash the calls array
keccak256(abi.encodePacked(callHashes))  // array of individual call hashes

// Hash the intent batch
keccak256(abi.encode(
    INTENT_BATCH_TYPEHASH, batch.signer,
    hashCalls(batch.calls),
    keccak256(abi.encodePacked(batch.tokensToSweep)),
    batch.nonce, batch.deadline
))

// Final EIP-712 digest
keccak256(abi.encodePacked("\x19\x01", DOMAIN_SEPARATOR, hashIntentBatch(batch)))
```

### Hashing (Rust side)

The Rust implementation in `crates/intent-script/src/eip712.rs` mirrors the Solidity hashing exactly:

- `hash_call(target, calldata, value)` → matches `_hashCalls` per-element
- `hash_calls(calls)` → matches `_hashCalls` array hash
- `hash_intent_batch(signer, calls, tokens_to_sweep, nonce, deadline)` → matches `_hashIntentBatch`
- `hash_typed_data(domain_separator, struct_hash)` → matches `_hashTypedData`
- `compute_domain_separator(name, version, chain_id, verifying_contract)` → matches constructor

**Critical:** The Rust and Solidity hashing MUST produce identical results. The EIP-712 unit tests in `eip712.rs` verify this by comparing against known hashes.

---

## Signed Execution Flow

### `executeSigned` Verification

```solidity
function executeSigned(IntentBatch calldata batch, bytes calldata signature) external payable {
    // 1. Deadline check (must be non-zero and not expired)
    require(batch.deadline > 0 && block.timestamp <= batch.deadline, "Expired or missing deadline");
    
    // 2. Nonce check (must match current nonce for signer)
    require(batch.nonce == nonces[batch.signer], "Invalid nonce");
    nonces[batch.signer]++;
    
    // 3. Signature verification
    bytes32 digest = _hashTypedData(batch);
    address recovered = _recover(digest, signature);
    require(recovered == batch.signer, "Invalid signature");
    
    // 4. Execute
    _executeCalls(batch.calls);
    _sweep(batch.tokensToSweep, batch.signer);  // sweep to SIGNER, not msg.sender
    _refundETH(batch.signer);
}
```

**Key security properties:**
- Deadline must be non-zero and in the future
- Nonce auto-increments → prevents replay
- Tokens sweep to `batch.signer`, not `msg.sender` → solver cannot steal tokens
- Signature recovery uses inline ECDSA (no OpenZeppelin dependency)

### Two Execution Flows

**Flow 1: User Self-Executes**
```
User → compile intent → get directTx → sign & submit executeDirect() → tokens swept to msg.sender
```

**Flow 2: Solver Executes**
```
User → compile intent → get EIP-712 typed data → sign EIP-712 → hand signature to solver
Solver → submit executeSigned(batch, signature) → tokens swept to batch.signer (user)
```

---

## Compiler Output for Batched Intents

When the compiler produces a batched intent (2+ calls with router), the output is `CompileOutput::Eip712Intent`:

```rust
pub struct Eip712IntentOutput {
    pub domain: Eip712Domain,           // EIP-712 domain params
    pub intent_batch: IntentBatchData,  // signer, calls, tokensToSweep, nonce, deadline
    pub typed_data_hash: [u8; 32],      // pre-computed EIP-712 hash
    pub description: String,            // human-readable description
    pub direct_tx: UnsignedTx,          // executeDirect() calldata for self-execution
}
```

The JSON output is directly compatible with `eth_signTypedData_v4`:

```json
{
  "type": "eip712_intent",
  "eip712": {
    "domain": { "name": "IntentRouter", "version": "1", "chainId": 1, "verifyingContract": "0x..." },
    "primaryType": "IntentBatch",
    "types": {
      "EIP712Domain": [...],
      "Call": [{ "name": "target", "type": "address" }, { "name": "callData", "type": "bytes" }, { "name": "value", "type": "uint256" }],
      "IntentBatch": [{ "name": "signer", "type": "address" }, { "name": "calls", "type": "Call[]" }, ...]
    },
    "message": { "signer": "0x...", "calls": [...], "tokensToSweep": [...], "nonce": "0", "deadline": "1712345678" }
  },
  "directTx": { "to": "0x...", "data": "0x...", "value": "0", ... }
}
```

---

## Prerequisites for Batched Execution

Before submitting a batched transaction, the user must:

1. **Approve the router** for each input ERC-20 token:
   ```
   USDC.approve(router, amount)
   ```

2. **Approve credit delegation** for borrows (Aave V3):
   ```
   variableDebtDAI.approveDelegation(router, amount)
   ```

These prerequisites are NOT included in the batch — they must be done as separate transactions before the batch is submitted. The compiler does not produce these prerequisite transactions.

---

## Foundry Test Infrastructure

### Test Files

| File | What | Uses |
|------|------|------|
| `contracts/test/IntentRouter.t.sol` | Unit tests with mocks | Mock ERC-20, mock swap router |
| `contracts/test/IntentRouterCalldata.t.sol` | Calldata verification | Reads compiler-generated fixture files |
| `contracts/test/IntentForkTests.t.sol` | Local mock integration | Mock protocols, full flows |
| `contracts/test/IntentForkE2E.t.sol` | Fork E2E against mainnet | Real Uniswap, Aave, Lido on fork |

### Fixture System

The Rust tests generate fixture files that Foundry tests consume:

```
contracts/test/fixtures/
├── {scenario}.txt          # executeDirect() calldata (hex)
├── {scenario}_to.txt       # target address
├── {scenario}_value.txt    # ETH value in wei
├── {scenario}_batch.json   # IntentBatch JSON for executeSigned
└── {scenario}_eip712.json  # Full EIP-712 typed data
```

Generated by:
- `crates/intent-script/tests/generate_calldata.rs` — calldata fixtures
- `crates/intent-script/tests/generate_eip712_fixtures.rs` — EIP-712 fixtures

Regenerate with: `make generate-fixtures`
