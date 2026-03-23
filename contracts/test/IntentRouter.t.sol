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
        router.execute{ value: 1 ether }(calls, tokensToSweep);

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
        router.execute(calls, tokensToSweep);

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
        router.execute{ value: 1 ether }(calls, tokensToSweep);

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
        router.execute(calls, tokensToSweep);
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
        router.execute{ value: 5 ether }(calls, tokensToSweep);

        // User should have 1 WETH and 4 ETH refunded
        assertEq(weth.balanceOf(user), 1 ether, "User should have 1 WETH");
        assertEq(user.balance, userEthBefore - 1 ether, "User should only spend 1 ETH");
        assertEq(address(router).balance, 0, "Router should have 0 ETH");
    }
}
