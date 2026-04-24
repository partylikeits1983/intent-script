// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { IERC20 } from "./interfaces/IERC20.sol";
import { IERC721Receiver } from "./interfaces/IERC721Receiver.sol";
import { ReentrancyGuard } from "./utils/ReentrancyGuard.sol";

/// @title IntentRouter
/// @notice Executes batched calls and sweeps tokens back to the caller/signer.
/// @dev Supports two execution modes:
///      - `executeDirect`: User submits tx directly (uses msg.sender)
///      - `executeSigned`: Relayer submits on behalf of signer (EIP-712 signature)
contract IntentRouter is ReentrancyGuard {
    // ─── EIP-712 ────────────────────────────────────────────
    bytes32 public constant DOMAIN_TYPEHASH = keccak256(
        "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
    );
    bytes32 public constant CALL_TYPEHASH = keccak256(
        "Call(address target,bytes callData,uint256 value)"
    );
    bytes32 public constant INTENT_BATCH_TYPEHASH = keccak256(
        "IntentBatch(address signer,Call[] calls,address[] tokensToSweep,uint256 nonce,uint256 deadline)Call(address target,bytes callData,uint256 value)"
    );

    bytes32 public immutable DOMAIN_SEPARATOR;

    mapping(address => uint256) public nonces;

    // ─── Allowlist (Task 8) ─────────────────────────────────
    address public owner;
    mapping(address => bool) public allowedTargets;

    // ─── Balancer V2 flashloan ──────────────────────────────
    /// Balancer V2 Vault mainnet address — used for 0% flashloans.
    address public constant BALANCER_VAULT = 0xBA12222222228d8Ba445958a75a0704d566BF2C8;
    /// `flashLoan(address,address[],uint256[],bytes)` selector.
    bytes4 public constant FLASHLOAN_SELECTOR = 0x5c38449e;
    /// Transient-storage slot (EIP-1153) used as a boolean sentinel while a
    /// flashloan call is in flight. Set just before `vault.flashLoan` and
    /// cleared in `receiveFlashLoan` before the inner pipeline runs.
    bytes32 private constant FLASHLOAN_GUARD_SLOT = keccak256("intent.flashloan.guard");

    // ─── Fees ───────────────────────────────────────────────
    uint16 public constant MAX_FEE_BPS = 100;   // 1.00% hard cap
    uint256 public constant FEE_TIMELOCK = 1 days;

    uint16  public feeBps;              // active rate
    address public feeRecipient;        // active recipient
    uint16  public pendingFeeBps;       // queued rate
    address public pendingFeeRecipient; // queued recipient
    uint64  public pendingFeeApplyAt;   // earliest timestamp at which queued fee may be applied; 0 = none queued

    // ─── Pause (B11) ────────────────────────────────────────
    /// Emergency kill-switch. Owner can pause immediately to halt the
    /// blast radius when a compromised target / exploit is detected.
    /// Unpausing is time-locked by FEE_TIMELOCK so a compromised owner
    /// key can't toggle the router back on at will.
    bool public paused;
    uint64 public pendingUnpauseAt; // 0 = none pending

    event FeeQueued(uint16 bps, address recipient, uint64 applyAt);
    event FeeApplied(uint16 bps, address recipient);
    event FeeCollected(address indexed token, address indexed recipient, uint256 amount);
    event Paused();
    event UnpauseQueued(uint64 applyAt);
    event Unpaused();

    modifier onlyOwner() {
        require(msg.sender == owner, "Not owner");
        _;
    }

    modifier whenNotPaused() {
        require(!paused, "Router paused");
        _;
    }

    /// @notice A single call to execute.
    struct Call {
        address target;
        bytes callData;
        uint256 value;
    }

    /// @notice A signed batch of calls with replay protection.
    struct IntentBatch {
        address signer;
        Call[] calls;
        address[] tokensToSweep;
        uint256 nonce;
        uint256 deadline;
    }

    constructor() {
        owner = msg.sender;
        DOMAIN_SEPARATOR = keccak256(abi.encode(
            DOMAIN_TYPEHASH,
            keccak256("IntentRouter"),
            keccak256("1"),
            block.chainid,
            address(this)
        ));
    }

    // ─── Allowlist management ───────────────────────────────

    /// @notice Set whether a target address is allowed.
    function setAllowedTarget(address target, bool allowed) external onlyOwner {
        allowedTargets[target] = allowed;
    }

    /// @notice Batch-set allowed targets.
    function setAllowedTargets(address[] calldata targets, bool allowed) external onlyOwner {
        for (uint256 i = 0; i < targets.length; i++) {
            allowedTargets[targets[i]] = allowed;
        }
    }

    // ─── Fee governance ─────────────────────────────────────

    /// @notice Queue a new fee rate + recipient. The new fee can be activated by
    ///         calling `applyFee()` after `FEE_TIMELOCK` has elapsed.
    /// @dev    Queueing again before apply overwrites the previous queue (and resets the timelock).
    function queueFee(uint16 newBps, address newRecipient) external onlyOwner {
        require(newBps <= MAX_FEE_BPS, "fee > max");
        pendingFeeBps = newBps;
        pendingFeeRecipient = newRecipient;
        pendingFeeApplyAt = uint64(block.timestamp + FEE_TIMELOCK);
        emit FeeQueued(newBps, newRecipient, pendingFeeApplyAt);
    }

    /// @notice Apply a previously queued fee once the timelock has expired.
    ///         Permissionless by design: any party can finalize the queued value.
    function applyFee() external {
        require(pendingFeeApplyAt != 0, "no pending fee");
        require(block.timestamp >= pendingFeeApplyAt, "timelock");
        feeBps = pendingFeeBps;
        feeRecipient = pendingFeeRecipient;
        pendingFeeApplyAt = 0;
        emit FeeApplied(feeBps, feeRecipient);
    }

    // ─── Pause ───────────────────────────────────────────────

    /// @notice Immediate pause (owner only). No timelock — speed matters
    ///         when halting a live exploit.
    function pause() external onlyOwner {
        paused = true;
        pendingUnpauseAt = 0; // cancel any in-flight unpause
        emit Paused();
    }

    /// @notice Queue an unpause. Must wait FEE_TIMELOCK seconds before
    ///         applyUnpause() can take effect. Timelocking the *unpause*
    ///         side prevents a compromised owner key from flipping the
    ///         switch back on at will.
    function queueUnpause() external onlyOwner {
        require(paused, "not paused");
        pendingUnpauseAt = uint64(block.timestamp + FEE_TIMELOCK);
        emit UnpauseQueued(pendingUnpauseAt);
    }

    /// @notice Apply a queued unpause once the timelock has elapsed.
    ///         Permissionless so the owner can't delay the unpause beyond
    ///         the timelock by withholding their key.
    function applyUnpause() external {
        require(paused, "not paused");
        require(pendingUnpauseAt != 0, "no pending unpause");
        require(block.timestamp >= pendingUnpauseAt, "timelock");
        paused = false;
        pendingUnpauseAt = 0;
        emit Unpaused();
    }

    // ─── ERC721 receiver ────────────────────────────────────

    /// @notice Accept ERC-721 safe transfers into the router. No custody logic — NFTs that
    ///         land here should be swept out by a follow-up call in the same or next batch.
    function onERC721Received(address, address, uint256, bytes calldata)
        external pure returns (bytes4) {
        return IERC721Receiver.onERC721Received.selector;
    }

    // ─── Self-execute (user submits tx directly) ────────────

    /// @notice Execute a batch of calls and sweep tokens back to the caller.
    /// @param calls Array of calls to execute in order.
    /// @param tokensToSweep Array of ERC-20 token addresses to sweep back to msg.sender.
    function executeDirect(
        Call[] calldata calls,
        address[] calldata tokensToSweep
    ) external payable nonReentrant whenNotPaused {
        _executeCalls(calls);
        _sweep(tokensToSweep, msg.sender);
        _refundETH(msg.sender);
    }

    // ─── Solver-execute (relayer submits on behalf of signer) ─

    /// @notice Execute a signed batch of calls on behalf of the signer.
    /// @param batch The signed intent batch.
    /// @param signature The EIP-712 signature (65 bytes: r, s, v).
    function executeSigned(
        IntentBatch calldata batch,
        bytes calldata signature
    ) external payable nonReentrant whenNotPaused {
        // Verify deadline (Task 2: require non-zero deadline)
        require(batch.deadline > 0 && block.timestamp <= batch.deadline, "Expired or missing deadline");

        // Verify nonce
        require(batch.nonce == nonces[batch.signer], "Invalid nonce");
        nonces[batch.signer]++;

        // Verify EIP-712 signature
        bytes32 digest = _hashTypedData(batch);
        address recovered = _recover(digest, signature);
        require(recovered == batch.signer, "Invalid signature");

        // Execute
        _executeCalls(batch.calls);
        _sweep(batch.tokensToSweep, batch.signer);
        _refundETH(batch.signer);
    }

    // ─── Internal helpers ───────────────────────────────────

    function _executeCalls(Call[] calldata calls) internal {
        for (uint256 i = 0; i < calls.length; i++) {
            require(allowedTargets[calls[i].target], "Target not allowed");

            // Just before dispatching a Balancer flashLoan, arm the transient
            // sentinel. `receiveFlashLoan` requires it to be set and clears it
            // before running inner calls. This defends against a compromised
            // allowlisted target re-entering `receiveFlashLoan` via delegatecall.
            if (
                calls[i].target == BALANCER_VAULT
                && calls[i].callData.length >= 4
                && bytes4(calls[i].callData[:4]) == FLASHLOAN_SELECTOR
            ) {
                bytes32 slot = FLASHLOAN_GUARD_SLOT;
                assembly { tstore(slot, 1) }
            }

            (bool success, bytes memory result) = calls[i].target.call{ value: calls[i].value }(
                calls[i].callData
            );
            if (!success) {
                // Bubble up the revert reason
                assembly {
                    revert(add(result, 32), mload(result))
                }
            }
        }
    }

    /// Execute a single call held in memory (used by `receiveFlashLoan`).
    /// Applies the same allowlist check as `_executeCalls`.
    function _execOne(Call memory call) internal {
        require(allowedTargets[call.target], "Target not allowed");
        (bool success, bytes memory result) = call.target.call{ value: call.value }(call.callData);
        if (!success) {
            assembly { revert(add(result, 32), mload(result)) }
        }
    }

    /// @notice Balancer V2 flashloan callback. Balancer transfers `amounts[i]`
    ///         of each `tokens[i]` to this contract before calling this
    ///         function, then asserts `balanceOf(self) >= pre + feeAmounts[i]`
    ///         after return. We decode `userData` into an inner `Call[]`,
    ///         execute it (subject to the allowlist), and transfer the owed
    ///         amount back to the Vault. The sentinel must have been armed by
    ///         `_executeCalls` immediately before the flashLoan call.
    function receiveFlashLoan(
        address[] calldata tokens,
        uint256[] calldata amounts,
        uint256[] calldata feeAmounts,
        bytes calldata userData
    ) external {
        require(msg.sender == BALANCER_VAULT, "not vault");

        bytes32 slot = FLASHLOAN_GUARD_SLOT;
        bytes32 guard;
        assembly { guard := tload(slot) }
        require(guard != bytes32(0), "no flashloan in progress");
        // Clear before running inner calls so a nested flashLoan via a
        // compromised allowlisted target can't satisfy the sentinel check.
        assembly { tstore(slot, 0) }

        Call[] memory innerCalls = abi.decode(userData, (Call[]));
        for (uint256 i = 0; i < innerCalls.length; i++) {
            _execOne(innerCalls[i]);
        }

        // Repay: Vault checks `balanceOf(address(this)) >= pre + feeAmounts[i]`.
        // Transfer the owed amount back explicitly rather than leaving it to
        // the Vault's balance comparison — cheaper on non-rebasing tokens and
        // keeps semantics obvious.
        for (uint256 i = 0; i < tokens.length; i++) {
            uint256 owed = amounts[i] + feeAmounts[i];
            require(
                IERC20(tokens[i]).transfer(BALANCER_VAULT, owed),
                "flashloan repay fail"
            );
        }
    }

    function _sweep(address[] calldata tokens, address recipient) internal {
        uint256 bps = feeBps;
        address feeTo = feeRecipient;
        bool feesOn = bps > 0 && feeTo != address(0);
        for (uint256 i = 0; i < tokens.length; i++) {
            uint256 balance = IERC20(tokens[i]).balanceOf(address(this));
            if (balance == 0) continue;
            uint256 fee = feesOn ? (balance * bps) / 10_000 : 0;
            if (fee > 0) {
                require(IERC20(tokens[i]).transfer(feeTo, fee), "fee xfer fail");
                emit FeeCollected(tokens[i], feeTo, fee);
            }
            require(IERC20(tokens[i]).transfer(recipient, balance - fee), "Token sweep failed");
        }
    }

    function _refundETH(address recipient) internal {
        uint256 ethBalance = address(this).balance;
        if (ethBalance == 0) return;
        uint256 bps = feeBps;
        address feeTo = feeRecipient;
        uint256 fee = (bps > 0 && feeTo != address(0)) ? (ethBalance * bps) / 10_000 : 0;
        if (fee > 0) {
            (bool feeOk,) = feeTo.call{ value: fee }("");
            require(feeOk, "fee eth fail");
            emit FeeCollected(address(0), feeTo, fee);
        }
        (bool sent,) = recipient.call{ value: ethBalance - fee }("");
        require(sent, "ETH refund failed");
    }

    function _hashTypedData(IntentBatch calldata batch) internal view returns (bytes32) {
        return keccak256(abi.encodePacked(
            "\x19\x01",
            DOMAIN_SEPARATOR,
            _hashIntentBatch(batch)
        ));
    }

    function _hashIntentBatch(IntentBatch calldata batch) internal pure returns (bytes32) {
        return keccak256(abi.encode(
            INTENT_BATCH_TYPEHASH,
            batch.signer,
            _hashCalls(batch.calls),
            keccak256(abi.encodePacked(batch.tokensToSweep)),
            batch.nonce,
            batch.deadline
        ));
    }

    function _hashCalls(Call[] calldata calls) internal pure returns (bytes32) {
        bytes32[] memory hashes = new bytes32[](calls.length);
        for (uint256 i = 0; i < calls.length; i++) {
            hashes[i] = keccak256(abi.encode(
                CALL_TYPEHASH,
                calls[i].target,
                keccak256(calls[i].callData),
                calls[i].value
            ));
        }
        return keccak256(abi.encodePacked(hashes));
    }

    function _recover(bytes32 digest, bytes calldata sig) internal pure returns (address) {
        require(sig.length == 65, "Invalid signature length");
        bytes32 r;
        bytes32 s;
        uint8 v;
        assembly {
            r := calldataload(sig.offset)
            s := calldataload(add(sig.offset, 32))
            v := byte(0, calldataload(add(sig.offset, 64)))
        }
        // EIP-2 canonical signature: reject the high-s half (malleable form).
        // The nonce check already prevents same-signer replay, so this is
        // defense-in-depth — but future changes that key on signature bytes
        // (dedup, receipt indexing, off-chain verifiers) break silently
        // without the canonical check. The bound is n/2 where n is the
        // secp256k1 curve order.
        require(
            uint256(s) <= 0x7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF5D576E7357A4501DDFE92F46681B20A0,
            "Signature s value not canonical"
        );
        require(v == 27 || v == 28, "Invalid signature v value");
        address recovered = ecrecover(digest, v, r, s);
        require(recovered != address(0), "ecrecover returned zero");
        return recovered;
    }

    /// @notice Accept ETH transfers (e.g., from WETH.withdraw()).
    receive() external payable { }
}
