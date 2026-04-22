// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { Test } from "forge-std/Test.sol";
import { IntentRouter } from "../src/IntentRouter.sol";

/// @notice Malicious ERC-20 whose `transfer` re-enters `executeDirect` on the router.
///         Used to prove the nonReentrant modifier blocks reentry via a compromised
///         token on the sweep path.
contract MaliciousToken {
    IntentRouter public immutable router;
    address public user;
    bool public attackArmed;

    constructor(IntentRouter _router) {
        router = _router;
    }

    function arm(address _user) external { user = _user; attackArmed = true; }

    function balanceOf(address) external view returns (uint256) {
        // Non-zero balance so _sweep takes the transfer branch.
        return attackArmed ? 1 : 0;
    }

    function transfer(address, uint256) external returns (bool) {
        if (attackArmed) {
            // Re-enter the router. The nonReentrant modifier on executeDirect should
            // revert this nested call, which bubbles up through the sweep and fails
            // the outer executeDirect.
            attackArmed = false; // prevent infinite recursion on revert-catching callers
            IntentRouter.Call[] memory calls = new IntentRouter.Call[](0);
            address[] memory sweep = new address[](0);
            router.executeDirect(calls, sweep);
        }
        return true;
    }
}

contract IntentRouterReentrancyTest is Test {
    IntentRouter public router;
    MaliciousToken public badToken;
    address public user = makeAddr("user");

    function setUp() public {
        router = new IntentRouter();
        badToken = new MaliciousToken(router);
        vm.deal(user, 10 ether);
    }

    /// @notice A token whose `transfer` re-enters `executeDirect` must be stopped by
    ///         the ReentrancyGuard. The outer call reverts with the guard's message.
    function test_ExecuteDirect_Reentrancy_Reverts() public {
        badToken.arm(user);

        IntentRouter.Call[] memory calls = new IntentRouter.Call[](0);
        address[] memory sweep = new address[](1);
        sweep[0] = address(badToken);

        vm.prank(user);
        vm.expectRevert("ReentrancyGuard: reentrant call");
        router.executeDirect(calls, sweep);
    }
}
