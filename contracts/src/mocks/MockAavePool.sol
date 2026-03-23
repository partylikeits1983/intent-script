// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { IERC20 } from "../interfaces/IERC20.sol";

/// @title MockAavePool
/// @notice Mock Aave V3 Pool that implements supply() and borrow() with the same ABI.
///         Tracks deposits and allows borrowing up to deposited amount.
contract MockAavePool {
    // Track deposits per user per asset
    mapping(address => mapping(address => uint256)) public deposits;

    // Track borrows per user per asset
    mapping(address => mapping(address => uint256)) public borrows;

    /// @notice Supply (deposit) an asset into the pool.
    ///         Same ABI as Aave V3: supply(address asset, uint256 amount, address onBehalfOf, uint16 referralCode)
    function supply(address asset, uint256 amount, address onBehalfOf, uint16 /* referralCode */) external {
        // Transfer asset from caller to pool
        IERC20(asset).transferFrom(msg.sender, address(this), amount);
        deposits[onBehalfOf][asset] += amount;
    }

    /// @notice Borrow an asset from the pool.
    ///         Same ABI as Aave V3: borrow(address asset, uint256 amount, uint256 interestRateMode, uint16 referralCode, address onBehalfOf)
    ///         Mock: allows borrowing up to total deposits (simplified, no collateral factor).
    function borrow(
        address asset,
        uint256 amount,
        uint256 /* interestRateMode */,
        uint16 /* referralCode */,
        address onBehalfOf
    ) external {
        borrows[onBehalfOf][asset] += amount;

        // Mint borrowed tokens to the borrower
        (bool success,) = asset.call(
            abi.encodeWithSignature("mint(address,uint256)", onBehalfOf, amount)
        );
        require(success, "Mock: failed to mint borrowed tokens");
    }

    /// @notice Withdraw an asset from the pool.
    ///         Same ABI as Aave V3: withdraw(address asset, uint256 amount, address to)
    function withdraw(address asset, uint256 amount, address to) external returns (uint256) {
        require(deposits[msg.sender][asset] >= amount, "Mock: insufficient deposit");
        deposits[msg.sender][asset] -= amount;
        IERC20(asset).transfer(to, amount);
        return amount;
    }
}
