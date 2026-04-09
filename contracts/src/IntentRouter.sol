// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { IERC20 } from "./interfaces/IERC20.sol";

/// @title IntentRouter
/// @notice Executes batched calls and sweeps tokens back to the caller/signer.
/// @dev Supports two execution modes:
///      - `executeDirect`: User submits tx directly (uses msg.sender)
///      - `executeSigned`: Relayer submits on behalf of signer (EIP-712 signature)
contract IntentRouter {
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

    modifier onlyOwner() {
        require(msg.sender == owner, "Not owner");
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

    // ─── Self-execute (user submits tx directly) ────────────

    /// @notice Execute a batch of calls and sweep tokens back to the caller.
    /// @param calls Array of calls to execute in order.
    /// @param tokensToSweep Array of ERC-20 token addresses to sweep back to msg.sender.
    function executeDirect(
        Call[] calldata calls,
        address[] calldata tokensToSweep
    ) external payable {
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
    ) external payable {
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

    function _sweep(address[] calldata tokens, address recipient) internal {
        for (uint256 i = 0; i < tokens.length; i++) {
            uint256 balance = IERC20(tokens[i]).balanceOf(address(this));
            if (balance > 0) {
                bool success = IERC20(tokens[i]).transfer(recipient, balance);
                require(success, "Token sweep failed");
            }
        }
    }

    function _refundETH(address recipient) internal {
        uint256 ethBalance = address(this).balance;
        if (ethBalance > 0) {
            (bool sent,) = recipient.call{ value: ethBalance }("");
            require(sent, "ETH refund failed");
        }
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
        return ecrecover(digest, v, r, s);
    }

    /// @notice Accept ETH transfers (e.g., from WETH.withdraw()).
    receive() external payable { }
}
