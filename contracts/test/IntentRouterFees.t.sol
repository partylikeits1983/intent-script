// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { Test } from "forge-std/Test.sol";
import { IntentRouter } from "../src/IntentRouter.sol";
import { MockERC20 } from "../src/mocks/MockERC20.sol";
import { WETH9 } from "../src/mocks/WETH9.sol";

contract IntentRouterFeesTest is Test {
    IntentRouter public router;
    MockERC20 public token;
    WETH9 public weth;

    address public user = makeAddr("user");
    address public feeTo = makeAddr("feeTo");
    address public outsider = makeAddr("outsider");

    function setUp() public {
        router = new IntentRouter(0xBA12222222228d8Ba445958a75a0704d566BF2C8, 0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2);
        token = new MockERC20("Mock", "MCK", 18);
        weth = new WETH9();

        router.setAllowedTarget(address(weth), true);

        vm.deal(user, 100 ether);
    }

    // ─── Fee governance (queue / apply) ─────────────────────────────

    function test_QueueAndApplyFee_HappyPath() public {
        vm.expectEmit(false, false, false, true, address(router));
        emit IntentRouter.FeeQueued(10, feeTo, uint64(block.timestamp + 1 days));
        router.queueFee(10, feeTo);

        assertEq(router.pendingFeeBps(), 10, "pending bps queued");
        assertEq(router.pendingFeeRecipient(), feeTo, "pending recipient queued");
        assertEq(router.pendingFeeApplyAt(), uint64(block.timestamp + 1 days), "applyAt set");
        assertEq(router.feeBps(), 0, "active bps unchanged pre-apply");
        assertEq(router.feeRecipient(), address(0), "active recipient unchanged pre-apply");

        vm.warp(block.timestamp + 1 days);

        vm.expectEmit(false, false, false, true, address(router));
        emit IntentRouter.FeeApplied(10, feeTo);
        router.applyFee();

        assertEq(router.feeBps(), 10, "active bps applied");
        assertEq(router.feeRecipient(), feeTo, "active recipient applied");
        assertEq(router.pendingFeeApplyAt(), 0, "pending cleared");
    }

    function test_ApplyFee_RevertsBeforeTimelock() public {
        router.queueFee(10, feeTo);

        // 1 second before the timelock elapses → must revert
        vm.warp(block.timestamp + 1 days - 1);
        vm.expectRevert("timelock");
        router.applyFee();
    }

    function test_ApplyFee_RevertsWhenNoPending() public {
        vm.expectRevert("no pending fee");
        router.applyFee();
    }

    function test_QueueFee_RevertsAboveMax() public {
        uint16 overMax = router.MAX_FEE_BPS() + 1;
        vm.expectRevert("fee > max");
        router.queueFee(overMax, feeTo);
    }

    function test_QueueFee_OnlyOwner() public {
        vm.prank(outsider);
        vm.expectRevert("Not owner");
        router.queueFee(10, feeTo);
    }

    // ─── Fee deduction at sweep / refund ────────────────────────────

    /// @dev Helper: activate a given fee instantly.
    function _activateFee(uint16 bps, address recipient) internal {
        router.queueFee(bps, recipient);
        vm.warp(block.timestamp + 1 days);
        router.applyFee();
    }

    /// @notice Sweep deducts fee on ERC-20 tokens when fee is active.
    function test_Sweep_DeductsFee_Tokens() public {
        _activateFee(10, feeTo); // 10 bps = 0.1%

        // Pre-fund the router directly with 1000 tokens (simulate a completed swap/deposit).
        token.mint(address(router), 1000 ether);

        address[] memory sweep = new address[](1);
        sweep[0] = address(token);
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](0);

        vm.prank(user);
        router.executeDirect(calls, sweep);

        uint256 expectedFee = (1000 ether * 10) / 10_000; // 1 ether
        assertEq(token.balanceOf(feeTo), expectedFee, "fee recipient received fee");
        assertEq(token.balanceOf(user), 1000 ether - expectedFee, "user received balance minus fee");
        assertEq(token.balanceOf(address(router)), 0, "router swept clean");
    }

    /// @notice Zero-fee sweep makes no fee transfer.
    function test_Sweep_ZeroFee_NoDeduction() public {
        // No _activateFee call → feeBps stays 0.
        token.mint(address(router), 1000 ether);

        address[] memory sweep = new address[](1);
        sweep[0] = address(token);
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](0);

        vm.prank(user);
        router.executeDirect(calls, sweep);

        assertEq(token.balanceOf(feeTo), 0, "no fee taken");
        assertEq(token.balanceOf(user), 1000 ether, "user got full balance");
    }

    /// @notice ETH refund deducts fee when active.
    function test_Refund_DeductsFee_Eth() public {
        _activateFee(100, feeTo); // 100 bps = 1%

        // Wrap 5 ETH but we'll send 10 ETH so that 5 ETH is refunded as ETH.
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("deposit()"),
            value: 5 ether
        });
        address[] memory sweep = new address[](1);
        sweep[0] = address(weth);

        uint256 feeToEthBefore = feeTo.balance;
        uint256 userEthBefore = user.balance;
        uint256 userWethBefore = weth.balanceOf(user);

        vm.prank(user);
        router.executeDirect{ value: 10 ether }(calls, sweep);

        // 5 ETH wrapped → 5 WETH minus 1% fee = 4.95 WETH to user.
        uint256 expectedWethFee = (5 ether * 100) / 10_000; // 0.05 WETH
        assertEq(weth.balanceOf(feeTo), expectedWethFee, "WETH fee captured");
        assertEq(
            weth.balanceOf(user) - userWethBefore, 5 ether - expectedWethFee, "user WETH after fee"
        );

        // 5 ETH refunded minus 1% = 4.95 ETH to user, 0.05 ETH to feeTo.
        uint256 expectedEthFee = (5 ether * 100) / 10_000; // 0.05 ETH
        assertEq(feeTo.balance - feeToEthBefore, expectedEthFee, "ETH fee captured");
        // User paid 10 ETH, got back (5 - 0.05) = 4.95 ETH → net spent 5.05 ETH.
        assertEq(
            userEthBefore - user.balance,
            10 ether - (5 ether - expectedEthFee),
            "user ETH net after fee refund"
        );
    }

    /// @notice If feeRecipient is the zero address, no fee is taken even when feeBps > 0.
    function test_FeeRecipientZero_NoFeeTaken() public {
        _activateFee(10, address(0));

        token.mint(address(router), 1000 ether);

        address[] memory sweep = new address[](1);
        sweep[0] = address(token);
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](0);

        vm.prank(user);
        router.executeDirect(calls, sweep);

        assertEq(token.balanceOf(user), 1000 ether, "user got full balance, no fee");
    }

    /// @notice Queue overwrites previous queue and resets the timelock.
    function test_QueueFee_OverwritesPending() public {
        router.queueFee(10, feeTo);
        uint64 firstApplyAt = router.pendingFeeApplyAt();

        // Wait half the timelock, queue a different rate → apply-at should reset.
        vm.warp(block.timestamp + 12 hours);
        router.queueFee(50, outsider);

        assertEq(router.pendingFeeBps(), 50, "new bps queued");
        assertEq(router.pendingFeeRecipient(), outsider, "new recipient queued");
        assertGt(router.pendingFeeApplyAt(), firstApplyAt, "apply-at moved forward");
    }
}
