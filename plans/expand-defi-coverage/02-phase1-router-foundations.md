# Sub-Task 02 — Phase 1: Router Contract Foundations

## Context

All later sub-tasks need three things in `IntentRouter.sol`:
1. A reentrancy guard on entry points (defense for all future protocols).
2. An `onERC721Received` hook (Uniswap V3 LP in sub-task 07 receives NFTs if user chooses `recipient=router`).
3. A governance-timelocked fee mechanism applied at sweep/refund time (sub-task 03 threads this into the compiler).

**Compiler code does NOT change in this sub-task.** Only `contracts/` + Foundry tests.

## Prerequisites

- Sub-task 01 complete. `config/protocols/ethereum.json` exists.

## Files to read first

- `contracts/src/IntentRouter.sol` — full file, to understand constructor, entry points, sweep, refund, allowlist.
- `contracts/test/IntentRouter.t.sol` — existing test patterns (setUp, mocks).
- `contracts/foundry.toml` — confirm `solc = "0.8.28"`.
- `contracts/script/` — any deploy scripts; you'll need to update them if allowlisted targets are hardcoded.

## Implementation

### 1.1 Inline `ReentrancyGuard` (do not pull in OpenZeppelin)

Create `contracts/src/utils/ReentrancyGuard.sol`:

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

abstract contract ReentrancyGuard {
    uint256 private constant _NOT_ENTERED = 1;
    uint256 private constant _ENTERED = 2;
    uint256 private _status = _NOT_ENTERED;

    modifier nonReentrant() {
        require(_status != _ENTERED, "ReentrancyGuard: reentrant call");
        _status = _ENTERED;
        _;
        _status = _NOT_ENTERED;
    }
}
```

### 1.2 `IERC721Receiver` interface

Create `contracts/src/interfaces/IERC721Receiver.sol`:

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

interface IERC721Receiver {
    function onERC721Received(address, address, uint256, bytes calldata) external returns (bytes4);
}
```

Add to `IntentRouter` (as a pure selector return — we do not do any custody logic in the handler; NFTs land and sit until swept by a follow-up intent):

```solidity
function onERC721Received(address, address, uint256, bytes calldata)
    external pure returns (bytes4) {
    return IERC721Receiver.onERC721Received.selector;  // 0x150b7a02
}
```

### 1.3 Fee mechanism with 24-hour timelock

Add to `IntentRouter.sol` (state + queue/apply functions). Preserve all existing state slots — append fee state at the end to avoid upgrade-surprises even though this contract isn't upgradable.

```solidity
// ─── Fees ────────────────────────────────────────────────
uint16 public constant MAX_FEE_BPS = 100;   // 1.00% hard cap
uint256 public constant FEE_TIMELOCK = 1 days;

uint16  public feeBps;
address public feeRecipient;
uint16  public pendingFeeBps;
address public pendingFeeRecipient;
uint64  public pendingFeeApplyAt;

event FeeQueued(uint16 bps, address recipient, uint64 applyAt);
event FeeApplied(uint16 bps, address recipient);
event FeeCollected(address indexed token, address indexed recipient, uint256 amount);

function queueFee(uint16 newBps, address newRecipient) external onlyOwner {
    require(newBps <= MAX_FEE_BPS, "fee > max");
    pendingFeeBps = newBps;
    pendingFeeRecipient = newRecipient;
    pendingFeeApplyAt = uint64(block.timestamp + FEE_TIMELOCK);
    emit FeeQueued(newBps, newRecipient, pendingFeeApplyAt);
}

function applyFee() external {
    require(pendingFeeApplyAt != 0, "no pending fee");
    require(block.timestamp >= pendingFeeApplyAt, "timelock");
    feeBps = pendingFeeBps;
    feeRecipient = pendingFeeRecipient;
    pendingFeeApplyAt = 0;
    emit FeeApplied(feeBps, feeRecipient);
}
```

### 1.4 Fee-aware `_sweep` and `_refundETH`

Replace `_sweep` body:

```solidity
function _sweep(address[] calldata tokens, address recipient) internal {
    uint256 bps = feeBps;
    address feeTo = feeRecipient;
    bool feesOn = bps > 0 && feeTo != address(0);
    for (uint256 i = 0; i < tokens.length; i++) {
        uint256 bal = IERC20(tokens[i]).balanceOf(address(this));
        if (bal == 0) continue;
        uint256 fee = feesOn ? (bal * bps) / 10_000 : 0;
        if (fee > 0) {
            require(IERC20(tokens[i]).transfer(feeTo, fee), "fee xfer fail");
            emit FeeCollected(tokens[i], feeTo, fee);
        }
        require(IERC20(tokens[i]).transfer(recipient, bal - fee), "Token sweep failed");
    }
}

function _refundETH(address recipient) internal {
    uint256 bal = address(this).balance;
    if (bal == 0) return;
    uint256 bps = feeBps;
    address feeTo = feeRecipient;
    uint256 fee = (bps > 0 && feeTo != address(0)) ? (bal * bps) / 10_000 : 0;
    if (fee > 0) {
        (bool s,) = feeTo.call{value: fee}("");
        require(s, "fee eth fail");
        emit FeeCollected(address(0), feeTo, fee);
    }
    (bool sent,) = recipient.call{value: bal - fee}("");
    require(sent, "ETH refund failed");
}
```

### 1.5 Apply `nonReentrant` to entry points

- `executeDirect`: add `nonReentrant` modifier.
- `executeSigned`: add `nonReentrant` modifier.
- Inherit from `ReentrancyGuard`.

Flashloan callback (added in sub-task 06) will intentionally be invoked *inside* a non-reentrant context — design guard there with transient storage, not the contract-level modifier.

### 1.6 Tests

Create `contracts/test/IntentRouterFees.t.sol`:
- `test_QueueAndApplyFee_HappyPath`
- `test_ApplyFee_RevertsBeforeTimelock`
- `test_QueueFee_RevertsAboveMax`
- `test_Sweep_DeductsFee_Tokens`
- `test_Sweep_ZeroFee_NoDeduction`
- `test_Refund_DeductsFee_Eth`
- `test_FeeRecipientZero_NoFeeTaken`

Create `contracts/test/IntentRouterReentrancy.t.sol`:
- `test_ExecuteDirect_Reentrancy_Reverts` — malicious token's `transfer` attempts to re-enter `executeDirect`; expect revert.

Update `contracts/test/IntentRouter.t.sol::setUp`: call `router.queueFee(0, address(0))`, `vm.warp(block.timestamp + 1 days)`, `router.applyFee()` so existing tests see zero fee and their math unchanged.

### 1.7 Deploy script (if applicable)

If `contracts/script/` has a deploy script that hardcodes the allowlist, verify it still builds. Don't add new targets yet — sub-tasks 04-08 will do that as their protocols are added.

## Definition of done

- [ ] `cd contracts && forge test -vv` all pass.
- [ ] New `ReentrancyGuard.sol` and `IERC721Receiver.sol` exist.
- [ ] `IntentRouter` inherits `ReentrancyGuard`, implements `onERC721Received`, has queue/apply fee flow.
- [ ] `_sweep` / `_refundETH` deduct fee when configured.
- [ ] All existing Foundry tests still pass (after the `setUp` update to zero out fee).
- [ ] `git diff --stat` touches only `contracts/` (plus this plan file).

## Verification

```bash
cd /Users/riemann/Desktop/intentOS/intent-script
make test
cd contracts && forge test -vv
```

Both must be green.

## Hand-off to sub-task 03

- Router now has `feeBps()` public getter. Sub-task 03 will read this at config-load time (via a config field, not by calling the chain — compiler is offline) and thread it into `step_produces` so `"all"` chains don't overestimate.
- The `intent_router` entry in `config/protocols/ethereum.json` will gain a `"fee_bps": 10` field — add it as a placeholder now or leave to sub-task 03.
- `onERC721Received` return value `0x150b7a02` is the ERC721 standard selector — verify against `IERC721Receiver.onERC721Received.selector` in tests.
