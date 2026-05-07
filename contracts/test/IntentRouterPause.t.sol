// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { Test } from "forge-std/Test.sol";
import { IntentRouter } from "../src/IntentRouter.sol";

/// @notice B11: pause / queued-unpause flow.
///
/// - pause() is immediate (emergency kill-switch)
/// - unpause requires queueUnpause() + FEE_TIMELOCK + applyUnpause()
/// - paused router rejects executeDirect and executeSigned
contract IntentRouterPauseTest is Test {
    IntentRouter router;
    address owner;
    address stranger;

    function setUp() public {
        owner = address(this);
        router = new IntentRouter(0xBA12222222228d8Ba445958a75a0704d566BF2C8, 0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2);
        stranger = vm.addr(0xBEEF);
    }

    function test_pause_onlyOwner() public {
        vm.prank(stranger);
        vm.expectRevert(bytes("Not owner"));
        router.pause();
    }

    function test_pause_setsFlag() public {
        assertFalse(router.paused());
        router.pause();
        assertTrue(router.paused());
    }

    function test_executeDirect_reverts_whenPaused() public {
        router.pause();
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](0);
        address[] memory sweep = new address[](0);
        vm.expectRevert(bytes("Router paused"));
        router.executeDirect(calls, sweep);
    }

    function test_queueUnpause_requiresPaused() public {
        vm.expectRevert(bytes("not paused"));
        router.queueUnpause();
    }

    function test_applyUnpause_beforeTimelock_reverts() public {
        router.pause();
        router.queueUnpause();
        // Not yet at the timelock horizon.
        vm.expectRevert(bytes("timelock"));
        router.applyUnpause();
    }

    function test_applyUnpause_afterTimelock_clears() public {
        router.pause();
        router.queueUnpause();
        vm.warp(block.timestamp + router.FEE_TIMELOCK() + 1);
        router.applyUnpause();
        assertFalse(router.paused());
    }

    function test_pause_cancelsPendingUnpause() public {
        router.pause();
        router.queueUnpause();
        assertGt(router.pendingUnpauseAt(), 0);
        // A second pause() (e.g. the owner spots a new exploit during the
        // timelock window) must reset any queued unpause so the timelock
        // restarts from zero the next time the owner tries to unwind.
        router.pause();
        assertEq(router.pendingUnpauseAt(), 0);
    }

    function test_applyUnpause_isPermissionless() public {
        router.pause();
        router.queueUnpause();
        vm.warp(block.timestamp + router.FEE_TIMELOCK() + 1);
        vm.prank(stranger);
        router.applyUnpause();
        assertFalse(router.paused());
    }
}
