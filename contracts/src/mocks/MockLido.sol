// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title MockLido
/// @notice Mock Lido stETH contract that implements submit() with the same ABI.
///         Mints stETH 1:1 for ETH deposited.
///         Implements a minimal ERC-20 interface for stETH balance tracking.
contract MockLido {
    string public constant name = "Liquid staked Ether 2.0";
    string public constant symbol = "stETH";
    uint8 public constant decimals = 18;

    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);
    event Submitted(address indexed sender, uint256 amount, address referral);

    /// @notice Stake ETH and receive stETH.
    ///         Same ABI as Lido: submit(address _referral) external payable returns (uint256)
    function submit(address _referral) external payable returns (uint256) {
        require(msg.value > 0, "Must send ETH");

        uint256 shares = msg.value; // 1:1 for mock
        totalSupply += shares;
        balanceOf[msg.sender] += shares;

        emit Submitted(msg.sender, msg.value, _referral);
        emit Transfer(address(0), msg.sender, shares);

        return shares;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        emit Approval(msg.sender, spender, amount);
        return true;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        require(balanceOf[msg.sender] >= amount, "stETH: insufficient balance");
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        emit Transfer(msg.sender, to, amount);
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        require(balanceOf[from] >= amount, "stETH: insufficient balance");
        require(allowance[from][msg.sender] >= amount, "stETH: insufficient allowance");
        allowance[from][msg.sender] -= amount;
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        emit Transfer(from, to, amount);
        return true;
    }

    /// @notice Accept ETH transfers.
    receive() external payable {}
}
