// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { IERC20 } from "./interfaces/IERC20.sol";

/// @title IntentRouter
/// @notice Executes batched calls and sweeps tokens back to the caller.
/// @dev Similar to 1inch's AggregationRouter — accepts an array of calls,
///      executes each via low-level .call(), then sweeps specified ERC-20
///      tokens and remaining ETH back to msg.sender.
contract IntentRouter {
    /// @notice A single call to execute.
    struct Call {
        address target;
        bytes callData;
        uint256 value;
    }

    /// @notice Execute a batch of calls and sweep tokens back to the caller.
    /// @param calls Array of calls to execute in order.
    /// @param tokensToSweep Array of ERC-20 token addresses to sweep back to msg.sender.
    function execute(Call[] calldata calls, address[] calldata tokensToSweep) external payable {
        // Execute each call
        for (uint256 i = 0; i < calls.length; i++) {
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

        // Sweep ERC-20 tokens back to caller
        for (uint256 i = 0; i < tokensToSweep.length; i++) {
            uint256 balance = IERC20(tokensToSweep[i]).balanceOf(address(this));
            if (balance > 0) {
                bool success = IERC20(tokensToSweep[i]).transfer(msg.sender, balance);
                require(success, "Token sweep failed");
            }
        }

        // Refund remaining ETH to caller
        uint256 ethBalance = address(this).balance;
        if (ethBalance > 0) {
            (bool sent,) = msg.sender.call{ value: ethBalance }("");
            require(sent, "ETH refund failed");
        }
    }

    /// @notice Accept ETH transfers (e.g., from WETH.withdraw()).
    receive() external payable { }
}
