// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { Test } from "forge-std/Test.sol";
import { IntentRouter } from "../src/IntentRouter.sol";
import { WETH9 } from "../src/mocks/WETH9.sol";

/// @notice B7: per-target function-selector allowlist.
///
/// Rollout: deploy with enforcement off, populate the allowlist, flip
/// enforcement on. Once on, every call's leading 4 bytes must be in
/// `allowedSelectors[target]` — allowlisting `pool` for `supply` no
/// longer implicitly allows every other public function on that target.
contract IntentRouterSelectorAllowlistTest is Test {
    IntentRouter router;
    WETH9 weth;
    bytes4 constant WETH_DEPOSIT_SELECTOR = bytes4(keccak256("deposit()"));
    bytes4 constant WETH_WITHDRAW_SELECTOR = bytes4(keccak256("withdraw(uint256)"));

    function setUp() public {
        router = new IntentRouter(0xBA12222222228d8Ba445958a75a0704d566BF2C8, 0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2);
        weth = new WETH9();
        router.setAllowedTarget(address(weth), true);
    }

    function _wrapCalls() internal view returns (IntentRouter.Call[] memory) {
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSelector(WETH_DEPOSIT_SELECTOR),
            value: 1 ether
        });
        return calls;
    }

    function test_selector_allowlist_off_by_default() public {
        assertFalse(router.selectorAllowlistEnforced());
        address[] memory sweep = new address[](0);
        vm.deal(address(this), 1 ether);
        router.executeDirect{ value: 1 ether }(_wrapCalls(), sweep);
        // Success — no allowlist enforced.
    }

    function test_enforced_empty_list_rejects_everything() public {
        router.setSelectorAllowlistEnforced(true);
        address[] memory sweep = new address[](0);
        vm.deal(address(this), 1 ether);
        vm.expectRevert(bytes("Selector not allowed"));
        router.executeDirect{ value: 1 ether }(_wrapCalls(), sweep);
    }

    function test_allowed_selector_passes() public {
        router.setAllowedSelector(address(weth), WETH_DEPOSIT_SELECTOR, true);
        router.setSelectorAllowlistEnforced(true);
        address[] memory sweep = new address[](0);
        vm.deal(address(this), 1 ether);
        router.executeDirect{ value: 1 ether }(_wrapCalls(), sweep);
    }

    function test_disallowed_selector_on_allowed_target_rejects() public {
        // Allowlist deposit only — a calldata hallucination that swaps to
        // withdraw on the same (allowlisted) target must now reject.
        router.setAllowedSelector(address(weth), WETH_DEPOSIT_SELECTOR, true);
        router.setSelectorAllowlistEnforced(true);

        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSelector(WETH_WITHDRAW_SELECTOR, 0),
            value: 0
        });
        address[] memory sweep = new address[](0);
        vm.expectRevert(bytes("Selector not allowed"));
        router.executeDirect(calls, sweep);
    }

    function test_batch_setter() public {
        bytes4[] memory sels = new bytes4[](2);
        sels[0] = WETH_DEPOSIT_SELECTOR;
        sels[1] = WETH_WITHDRAW_SELECTOR;
        router.setAllowedSelectors(address(weth), sels, true);
        assertTrue(router.allowedSelectors(address(weth), WETH_DEPOSIT_SELECTOR));
        assertTrue(router.allowedSelectors(address(weth), WETH_WITHDRAW_SELECTOR));

        // Revoke via the batch path too.
        router.setAllowedSelectors(address(weth), sels, false);
        assertFalse(router.allowedSelectors(address(weth), WETH_DEPOSIT_SELECTOR));
    }

    function test_setAllowedSelector_onlyOwner() public {
        address stranger = vm.addr(0xC0FFEE);
        vm.prank(stranger);
        vm.expectRevert(bytes("Not owner"));
        router.setAllowedSelector(address(weth), WETH_DEPOSIT_SELECTOR, true);
    }

    function test_setSelectorAllowlistEnforced_onlyOwner() public {
        address stranger = vm.addr(0xC0FFEE);
        vm.prank(stranger);
        vm.expectRevert(bytes("Not owner"));
        router.setSelectorAllowlistEnforced(true);
    }

    function test_short_calldata_rejected_when_enforced() public {
        router.setSelectorAllowlistEnforced(true);
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: hex"aa", // < 4 bytes
            value: 0
        });
        address[] memory sweep = new address[](0);
        vm.expectRevert(bytes("Selector required"));
        router.executeDirect(calls, sweep);
    }
}
