// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { Test, console } from "forge-std/Test.sol";
import { IntentRouter } from "../src/IntentRouter.sol";
import { WETH9 } from "../src/mocks/WETH9.sol";

contract IntentRouterTest is Test {
    IntentRouter public router;
    WETH9 public weth;

    address public user = makeAddr("user");

    function setUp() public {
        router = new IntentRouter();
        weth = new WETH9();

        // Whitelist WETH as an allowed target (Task 8)
        router.setAllowedTarget(address(weth), true);

        // Fund user with 100 ETH
        vm.deal(user, 100 ether);
    }

    /// @notice Test: Wrap ETH → WETH through the router, verify sweep back to user.
    function test_wrapETH_throughRouter() public {
        // Build the call: WETH.deposit() with 1 ETH
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("deposit()"),
            value: 1 ether
        });

        // Sweep WETH back to user
        address[] memory tokensToSweep = new address[](1);
        tokensToSweep[0] = address(weth);

        // Execute as user
        vm.prank(user);
        router.executeDirect{ value: 1 ether }(calls, tokensToSweep);

        // Assertions
        assertEq(weth.balanceOf(user), 1 ether, "User should have 1 WETH");
        assertEq(weth.balanceOf(address(router)), 0, "Router should have 0 WETH");
        assertEq(address(router).balance, 0, "Router should have 0 ETH");

        console.log("User WETH balance:", weth.balanceOf(user));
    }

    /// @notice Test: Unwrap WETH → ETH through the router.
    ///         First give user WETH, then user approves router, router calls withdraw.
    function test_unwrapWETH_throughRouter() public {
        // Setup: user wraps 5 ETH directly to get WETH
        vm.prank(user);
        weth.deposit{ value: 5 ether }();
        assertEq(weth.balanceOf(user), 5 ether);

        // User approves router to spend WETH
        vm.prank(user);
        weth.approve(address(router), 2 ether);

        // Build calls:
        // 1. transferFrom user's WETH to router
        // 2. withdraw WETH to ETH (router receives ETH)
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](2);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature(
                "transferFrom(address,address,uint256)", user, address(router), 2 ether
            ),
            value: 0
        });
        calls[1] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("withdraw(uint256)", 2 ether),
            value: 0
        });

        // No ERC-20 tokens to sweep (we're unwrapping to ETH, which auto-refunds)
        address[] memory tokensToSweep = new address[](0);

        uint256 userEthBefore = user.balance;

        vm.prank(user);
        router.executeDirect(calls, tokensToSweep);

        // Assertions
        assertEq(weth.balanceOf(user), 3 ether, "User should have 3 WETH remaining");
        assertEq(weth.balanceOf(address(router)), 0, "Router should have 0 WETH");
        assertEq(address(router).balance, 0, "Router should have 0 ETH");
        assertEq(user.balance, userEthBefore + 2 ether, "User should have received 2 ETH back");

        console.log("User WETH balance:", weth.balanceOf(user));
        console.log("User ETH balance:", user.balance);
    }

    /// @notice Test: Wrap + Unwrap in a single batch (atomic round-trip).
    function test_wrapAndUnwrap_throughRouter() public {
        uint256 userEthBefore = user.balance;

        // Build calls:
        // 1. deposit() — wrap 1 ETH to WETH (minted to router)
        // 2. withdraw(1 ether) — unwrap WETH back to ETH (sent to router)
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](2);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("deposit()"),
            value: 1 ether
        });
        calls[1] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("withdraw(uint256)", 1 ether),
            value: 0
        });

        // No tokens to sweep — ETH refund handles the return
        address[] memory tokensToSweep = new address[](0);

        vm.prank(user);
        router.executeDirect{ value: 1 ether }(calls, tokensToSweep);

        // Assertions: round-trip should return all ETH to user
        assertEq(weth.balanceOf(user), 0, "User should have 0 WETH");
        assertEq(weth.balanceOf(address(router)), 0, "Router should have 0 WETH");
        assertEq(address(router).balance, 0, "Router should have 0 ETH");
        assertEq(user.balance, userEthBefore, "User ETH should be unchanged");

        console.log("Round-trip complete. User ETH:", user.balance);
    }

    /// @notice Test: Revert bubbles up from a failed sub-call.
    function test_revert_bubblesUp() public {
        // Try to withdraw WETH that the router doesn't have
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("withdraw(uint256)", 1 ether),
            value: 0
        });

        address[] memory tokensToSweep = new address[](0);

        vm.prank(user);
        vm.expectRevert("WETH: insufficient balance");
        router.executeDirect(calls, tokensToSweep);
    }

    /// @notice Test: Excess ETH is refunded to the caller.
    function test_excessETH_refunded() public {
        uint256 userEthBefore = user.balance;

        // Send 5 ETH but only wrap 1 ETH
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("deposit()"),
            value: 1 ether
        });

        address[] memory tokensToSweep = new address[](1);
        tokensToSweep[0] = address(weth);

        vm.prank(user);
        router.executeDirect{ value: 5 ether }(calls, tokensToSweep);

        // User should have 1 WETH and 4 ETH refunded
        assertEq(weth.balanceOf(user), 1 ether, "User should have 1 WETH");
        assertEq(user.balance, userEthBefore - 1 ether, "User should only spend 1 ETH");
        assertEq(address(router).balance, 0, "Router should have 0 ETH");
    }

    // ─── EIP-712 executeSigned tests ────────────────────────────────────

    /// @notice Helper: Build EIP-712 digest for an IntentBatch
    function _buildDigest(IntentRouter.IntentBatch memory batch) internal view returns (bytes32) {
        bytes32 CALL_TYPEHASH_ = keccak256("Call(address target,bytes callData,uint256 value)");
        bytes32 INTENT_BATCH_TYPEHASH_ = keccak256(
            "IntentBatch(address signer,Call[] calls,address[] tokensToSweep,uint256 nonce,uint256 deadline)Call(address target,bytes callData,uint256 value)"
        );

        // Hash calls
        bytes32[] memory callHashes = new bytes32[](batch.calls.length);
        for (uint256 i = 0; i < batch.calls.length; i++) {
            callHashes[i] = keccak256(abi.encode(
                CALL_TYPEHASH_,
                batch.calls[i].target,
                keccak256(batch.calls[i].callData),
                batch.calls[i].value
            ));
        }
        bytes32 callsHash = keccak256(abi.encodePacked(callHashes));

        // Hash struct
        bytes32 structHash = keccak256(abi.encode(
            INTENT_BATCH_TYPEHASH_,
            batch.signer,
            callsHash,
            keccak256(abi.encodePacked(batch.tokensToSweep)),
            batch.nonce,
            batch.deadline
        ));

        return keccak256(abi.encodePacked("\x19\x01", router.DOMAIN_SEPARATOR(), structHash));
    }

    /// @notice Test: executeSigned with valid signature
    function test_executeSigned_validSignature() public {
        // Create a signer with a known private key
        uint256 signerPk = 0xA11CE;
        address signer = vm.addr(signerPk);
        vm.deal(signer, 100 ether);

        // Build the batch
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("deposit()"),
            value: 1 ether
        });

        address[] memory tokensToSweep = new address[](1);
        tokensToSweep[0] = address(weth);

        IntentRouter.IntentBatch memory batch = IntentRouter.IntentBatch({
            signer: signer,
            calls: calls,
            tokensToSweep: tokensToSweep,
            nonce: 0,
            deadline: type(uint256).max
        });

        // Sign the digest
        bytes32 digest = _buildDigest(batch);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(signerPk, digest);
        bytes memory signature = abi.encodePacked(r, s, v);

        // Execute via a relayer (not the signer)
        address relayer = makeAddr("relayer");
        vm.deal(relayer, 10 ether);

        vm.prank(relayer);
        router.executeSigned{ value: 1 ether }(batch, signature);

        // Tokens should go to the signer, not the relayer
        assertEq(weth.balanceOf(signer), 1 ether, "Signer should have 1 WETH");
        assertEq(weth.balanceOf(relayer), 0, "Relayer should have 0 WETH");
        assertEq(weth.balanceOf(address(router)), 0, "Router should have 0 WETH");
    }

    /// @notice Test: executeSigned with invalid signature
    function test_executeSigned_invalidSignature() public {
        uint256 signerPk = 0xA11CE;
        address signer = vm.addr(signerPk);
        uint256 wrongPk = 0xB0B;

        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("deposit()"),
            value: 1 ether
        });

        address[] memory tokensToSweep = new address[](0);

        IntentRouter.IntentBatch memory batch = IntentRouter.IntentBatch({
            signer: signer,
            calls: calls,
            tokensToSweep: tokensToSweep,
            nonce: 0,
            deadline: type(uint256).max
        });

        // Sign with wrong key
        bytes32 digest = _buildDigest(batch);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(wrongPk, digest);
        bytes memory signature = abi.encodePacked(r, s, v);

        vm.deal(address(this), 10 ether);
        vm.expectRevert("Invalid signature");
        router.executeSigned{ value: 1 ether }(batch, signature);
    }

    /// @notice Test: executeSigned with expired deadline
    function test_executeSigned_expiredDeadline() public {
        uint256 signerPk = 0xA11CE;
        address signer = vm.addr(signerPk);

        // Warp to a realistic timestamp so deadline=1000 is in the past
        vm.warp(2000);

        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("deposit()"),
            value: 1 ether
        });

        address[] memory tokensToSweep = new address[](0);

        IntentRouter.IntentBatch memory batch = IntentRouter.IntentBatch({
            signer: signer,
            calls: calls,
            tokensToSweep: tokensToSweep,
            nonce: 0,
            deadline: 1000 // expired (block.timestamp is 2000)
        });

        bytes32 digest = _buildDigest(batch);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(signerPk, digest);
        bytes memory signature = abi.encodePacked(r, s, v);

        vm.deal(address(this), 10 ether);
        vm.expectRevert("Expired or missing deadline");
        router.executeSigned{ value: 1 ether }(batch, signature);
    }

    /// @notice Test: executeSigned replay protection (nonce increments)
    function test_executeSigned_replayProtection() public {
        uint256 signerPk = 0xA11CE;
        address signer = vm.addr(signerPk);
        vm.deal(signer, 100 ether);

        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("deposit()"),
            value: 1 ether
        });

        address[] memory tokensToSweep = new address[](1);
        tokensToSweep[0] = address(weth);

        IntentRouter.IntentBatch memory batch = IntentRouter.IntentBatch({
            signer: signer,
            calls: calls,
            tokensToSweep: tokensToSweep,
            nonce: 0,
            deadline: type(uint256).max
        });

        bytes32 digest = _buildDigest(batch);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(signerPk, digest);
        bytes memory signature = abi.encodePacked(r, s, v);

        // First execution should succeed
        address relayer = makeAddr("relayer");
        vm.deal(relayer, 10 ether);
        vm.prank(relayer);
        router.executeSigned{ value: 1 ether }(batch, signature);

        // Nonce should have incremented
        assertEq(router.nonces(signer), 1, "Nonce should be 1 after first execution");

        // Replay with same signature should fail (nonce mismatch)
        vm.deal(relayer, 10 ether);
        vm.prank(relayer);
        vm.expectRevert("Invalid nonce");
        router.executeSigned{ value: 1 ether }(batch, signature);
    }

    /// @notice Test: executeSigned with wrong nonce
    function test_executeSigned_wrongNonce() public {
        uint256 signerPk = 0xA11CE;
        address signer = vm.addr(signerPk);

        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("deposit()"),
            value: 1 ether
        });

        address[] memory tokensToSweep = new address[](0);

        // Use nonce 5 when current nonce is 0
        IntentRouter.IntentBatch memory batch = IntentRouter.IntentBatch({
            signer: signer,
            calls: calls,
            tokensToSweep: tokensToSweep,
            nonce: 5,
            deadline: type(uint256).max
        });

        bytes32 digest = _buildDigest(batch);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(signerPk, digest);
        bytes memory signature = abi.encodePacked(r, s, v);

        vm.deal(address(this), 10 ether);
        vm.expectRevert("Invalid nonce");
        router.executeSigned{ value: 1 ether }(batch, signature);
    }

    /// @notice Test: executeSigned sweeps tokens to signer, not msg.sender
    function test_executeSigned_sweepToSigner() public {
        uint256 signerPk = 0xA11CE;
        address signer = vm.addr(signerPk);

        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("deposit()"),
            value: 1 ether
        });

        address[] memory tokensToSweep = new address[](1);
        tokensToSweep[0] = address(weth);

        IntentRouter.IntentBatch memory batch = IntentRouter.IntentBatch({
            signer: signer,
            calls: calls,
            tokensToSweep: tokensToSweep,
            nonce: 0,
            deadline: type(uint256).max
        });

        bytes32 digest = _buildDigest(batch);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(signerPk, digest);
        bytes memory signature = abi.encodePacked(r, s, v);

        // Relayer submits
        address relayer = makeAddr("relayer");
        vm.deal(relayer, 10 ether);
        vm.prank(relayer);
        router.executeSigned{ value: 1 ether }(batch, signature);

        // WETH should go to signer, not relayer
        assertEq(weth.balanceOf(signer), 1 ether, "Signer should have 1 WETH");
        assertEq(weth.balanceOf(relayer), 0, "Relayer should have 0 WETH");
    }

    // ═════════════════════════════════════════════════════════════════
    // Fuzz Tests
    // ═════════════════════════════════════════════════════════════════

    /// @notice Fuzz: executeDirect with empty calls array succeeds (no-op).
    ///         The router doesn't revert on empty calls — it just does nothing.
    function testFuzz_executeDirect_emptyCallsSucceeds(uint256 seed) public {
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](0);
        address[] memory sweep = new address[](0);

        uint256 balanceBefore = user.balance;

        // Empty calls should succeed (no-op)
        vm.prank(user);
        router.executeDirect(calls, sweep);

        // User balance should be unchanged (no ETH sent)
        assertEq(user.balance, balanceBefore, "Balance should be unchanged after empty executeDirect");
    }

    /// @notice Fuzz: executeSigned with invalid signature should revert.
    function testFuzz_executeSigned_invalidSignature(uint8 v, bytes32 r, bytes32 s) public {
        // Build a minimal valid batch
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("deposit()"),
            value: 1 ether
        });

        address[] memory sweep = new address[](1);
        sweep[0] = address(weth);

        IntentRouter.IntentBatch memory batch = IntentRouter.IntentBatch({
            signer: user,
            calls: calls,
            tokensToSweep: sweep,
            nonce: 0,
            deadline: type(uint256).max
        });

        // Random signature should fail ECDSA recovery
        bytes memory signature = abi.encodePacked(r, s, v);

        vm.deal(address(this), 10 ether);
        vm.expectRevert();
        router.executeSigned{ value: 1 ether }(batch, signature);
    }

    /// @notice Fuzz: sweep with the WETH token address always works.
    ///         After wrapping ETH, the sweep sends WETH back to the user.
    function testFuzz_sweep_wethAmount(uint96 amount) public {
        // Bound to reasonable range (0.001 ETH to 50 ETH)
        vm.assume(amount > 0.001 ether);
        vm.assume(amount < 50 ether);

        // Build a wrap call + sweep WETH
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("deposit()"),
            value: amount
        });

        address[] memory sweep = new address[](1);
        sweep[0] = address(weth);

        uint256 wethBefore = weth.balanceOf(user);

        vm.prank(user);
        router.executeDirect{ value: amount }(calls, sweep);

        uint256 wethAfter = weth.balanceOf(user);
        assertEq(wethAfter - wethBefore, amount, "User should receive all wrapped WETH via sweep");
    }

    // ─── Allowlist tests (Task 8) ───────────────────────────────────────

    /// @notice Test: Non-allowed target reverts
    function test_nonAllowedTarget_reverts() public {
        // Create a new address that is NOT in the allowlist
        address notAllowed = makeAddr("notAllowed");

        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: notAllowed,
            callData: "",
            value: 0
        });

        address[] memory tokensToSweep = new address[](0);

        vm.prank(user);
        vm.expectRevert("Target not allowed");
        router.executeDirect(calls, tokensToSweep);
    }

    /// @notice Test: Only owner can set allowed targets
    function test_setAllowedTarget_onlyOwner() public {
        address notOwner = makeAddr("notOwner");
        address target = makeAddr("target");

        vm.prank(notOwner);
        vm.expectRevert("Not owner");
        router.setAllowedTarget(target, true);
    }

    /// @notice Test: executeSigned with zero deadline is rejected
    function test_executeSigned_zeroDeadline_rejected() public {
        uint256 signerPk = 0xA11CE;
        address signer = vm.addr(signerPk);

        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("deposit()"),
            value: 1 ether
        });

        address[] memory tokensToSweep = new address[](0);

        IntentRouter.IntentBatch memory batch = IntentRouter.IntentBatch({
            signer: signer,
            calls: calls,
            tokensToSweep: tokensToSweep,
            nonce: 0,
            deadline: 0 // zero deadline should be rejected
        });

        bytes32 digest = _buildDigest(batch);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(signerPk, digest);
        bytes memory signature = abi.encodePacked(r, s, v);

        vm.deal(address(this), 10 ether);
        vm.expectRevert("Expired or missing deadline");
        router.executeSigned{ value: 1 ether }(batch, signature);
    }
}
