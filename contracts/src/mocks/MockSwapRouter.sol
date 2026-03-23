// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { IERC20 } from "../interfaces/IERC20.sol";
import { IWETH } from "../interfaces/IWETH.sol";

/// @title MockSwapRouter
/// @notice Mock Uniswap V3 SwapRouter that implements exactInputSingle.
///         Handles WETH output correctly by wrapping ETH via WETH.deposit().
///         For non-WETH ERC-20 outputs, uses mint() on MockERC20.
///         Implements the same ABI as the real Uniswap V3 SwapRouter.
contract MockSwapRouter {
    address public immutable WETH9;

    /// @notice Fixed exchange rate numerator (output = input * rate / 1e18)
    /// For testing: 1 USDC (6 dec) = 0.0005 WETH (18 dec) → rate = 5e14 * 1e18 / 1e6 = 5e26
    /// Simplified: we just use 1:1 for same-decimal tokens, and a configurable rate.
    uint256 public rate = 1e18; // 1:1 by default

    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 deadline;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }

    constructor(address _weth9) {
        WETH9 = _weth9;
    }

    /// @notice Set the exchange rate (amountOut = amountIn * rate / 1e18)
    function setRate(uint256 _rate) external {
        rate = _rate;
    }

    /// @notice Mock swap: transfers tokenIn from msg.sender, produces tokenOut to recipient.
    function exactInputSingle(ExactInputSingleParams calldata params) external payable returns (uint256 amountOut) {
        require(block.timestamp <= params.deadline || params.deadline == type(uint256).max, "Deadline expired");

        // Transfer tokenIn from caller
        IERC20(params.tokenIn).transferFrom(msg.sender, address(this), params.amountIn);

        // Calculate output amount using rate
        amountOut = params.amountIn * rate / 1e18;
        require(amountOut >= params.amountOutMinimum, "Insufficient output");

        if (params.tokenOut == WETH9) {
            // Output is WETH: wrap ETH via deposit()
            // The mock router must have been funded with ETH for this
            IWETH(WETH9).deposit{ value: amountOut }();
            // Transfer WETH to recipient
            IERC20(WETH9).transfer(params.recipient, amountOut);
        } else {
            // Output is a MockERC20: use mint()
            (bool success,) = params.tokenOut.call(
                abi.encodeWithSignature("mint(address,uint256)", params.recipient, amountOut)
            );
            require(success, "Mock: failed to mint output token");
        }

        return amountOut;
    }

    /// @notice Accept ETH (needed for WETH wrapping)
    receive() external payable {}
}
