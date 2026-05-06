// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { Test, console } from "forge-std/Test.sol";
import { IntentRouter } from "../src/IntentRouter.sol";
import { WETH9 } from "../src/mocks/WETH9.sol";
import { MockERC20 } from "../src/mocks/MockERC20.sol";
import { MockSwapRouter } from "../src/mocks/MockSwapRouter.sol";
import { MockAavePool } from "../src/mocks/MockAavePool.sol";
import { MockLido } from "../src/mocks/MockLido.sol";

/// @title IntentLocalTests
/// @notice Tests that deploy mock protocol contracts locally and execute
///         DeFi operations through the IntentRouter.
///         No fork needed — everything runs locally with mock contracts.
///
///         These tests validate that the compiler-generated calldata structure
///         (approve + protocol call) works correctly end-to-end.
contract IntentLocalTests is Test {
    IntentRouter public router;
    WETH9 public weth;
    MockERC20 public usdc;
    MockERC20 public dai;
    MockSwapRouter public swapRouter;
    MockAavePool public aavePool;
    MockLido public lido;

    address public user = makeAddr("user");

    // 1 USDC (6 dec) → 0.0005 WETH (18 dec)
    // rate = 0.0005 * 1e18 = 5e14
    // But since amountOut = amountIn * rate / 1e18:
    //   1000 * 1e6 * 5e14 / 1e18 = 5e8 * 5e14 / 1e18 = 5e23 / 1e18 = 5e5 = 500000 wei
    // That's 0.0000000000005 WETH — too small.
    // Better: use rate = 5e26 so 1000 USDC → 0.5 WETH
    //   1000e6 * 5e26 / 1e18 = 1e9 * 5e26 / 1e18 = 5e35 / 1e18 = 5e17 = 0.5 WETH ✓
    uint256 constant USDC_WETH_RATE = 5e26; // 1 USDC = 0.0005 WETH

    function setUp() public {
        // Deploy infrastructure
        router = new IntentRouter();
        weth = new WETH9();

        // Deploy mock tokens
        usdc = new MockERC20("USD Coin", "USDC", 6);
        dai = new MockERC20("Dai Stablecoin", "DAI", 18);

        // Deploy mock protocols (swap router needs WETH address)
        swapRouter = new MockSwapRouter(address(weth));
        aavePool = new MockAavePool();
        lido = new MockLido();

        // Set exchange rate for USDC → WETH
        swapRouter.setRate(USDC_WETH_RATE);

        // Whitelist all target contracts (Task 8: allowlist)
        router.setAllowedTarget(address(weth), true);
        router.setAllowedTarget(address(usdc), true);
        router.setAllowedTarget(address(dai), true);
        router.setAllowedTarget(address(swapRouter), true);
        router.setAllowedTarget(address(aavePool), true);
        router.setAllowedTarget(address(lido), true);

        // Fund user
        vm.deal(user, 100 ether);
        usdc.mint(user, 10_000 * 1e6); // 10,000 USDC

        // Fund swap router with ETH so it can produce WETH
        vm.deal(address(swapRouter), 1000 ether);
    }

    // ─── Test 1: Swap USDC → WETH via Router ───────────────────────────

    /// @notice Swap USDC → WETH through the IntentRouter.
    ///         Mirrors what the compiler generates: approve + exactInputSingle.
    function test_swapUSDCtoWETH_throughRouter() public {
        uint256 usdcAmount = 1000 * 1e6; // 1000 USDC

        // User approves router to pull USDC
        vm.prank(user);
        usdc.approve(address(router), usdcAmount);

        // Build batched calls (same structure as compiler output):
        // 1. TransferFrom USDC from user to router
        // 2. Approve swap router to spend USDC (from router)
        // 3. Swap USDC → WETH via mock swap router
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](3);

        calls[0] = IntentRouter.Call({
            target: address(usdc),
            callData: abi.encodeWithSignature(
                "transferFrom(address,address,uint256)", user, address(router), usdcAmount
            ),
            value: 0
        });

        calls[1] = IntentRouter.Call({
            target: address(usdc),
            callData: abi.encodeWithSignature(
                "approve(address,uint256)", address(swapRouter), usdcAmount
            ),
            value: 0
        });

        calls[2] = IntentRouter.Call({
            target: address(swapRouter),
            callData: abi.encodeWithSelector(
                MockSwapRouter.exactInputSingle.selector,
                MockSwapRouter.ExactInputSingleParams({
                    tokenIn: address(usdc),
                    tokenOut: address(weth),
                    fee: 3000,
                    recipient: user,
                    deadline: type(uint256).max,
                    amountIn: usdcAmount,
                    amountOutMinimum: 1, // Non-zero for slippage protection
                    sqrtPriceLimitX96: 0
                })
            ),
            value: 0
        });

        address[] memory tokensToSweep = new address[](1);
        tokensToSweep[0] = address(weth);

        // Execute through router
        vm.prank(user);
        router.executeDirect(calls, tokensToSweep);

        // Verify: user should have received WETH
        uint256 wethBalance = weth.balanceOf(user);
        // 1000 USDC * 5e26 / 1e18 = 5e17 = 0.5 WETH
        uint256 expectedWeth = usdcAmount * USDC_WETH_RATE / 1e18;
        assertEq(wethBalance, expectedWeth, "User should have correct WETH amount");
        assertEq(usdc.balanceOf(user), 10_000 * 1e6 - usdcAmount, "User USDC should decrease");

        console.log("Swap USDC->WETH succeeded. WETH balance:", wethBalance);
    }

    // ─── Test 2: Deposit USDC + Borrow DAI via Router ───────────────────

    /// @notice Deposit USDC into Aave, then borrow DAI — all through the router.
    function test_depositAndBorrow_throughRouter() public {
        uint256 usdcAmount = 5000 * 1e6; // 5000 USDC
        uint256 daiBorrow = 2000 * 1e18; // 2000 DAI

        // User approves router to pull USDC
        vm.prank(user);
        usdc.approve(address(router), usdcAmount);

        // Build batched calls:
        // 1. TransferFrom USDC from user to router
        // 2. Approve Aave pool to spend USDC (from router)
        // 3. Supply USDC to Aave (on behalf of user)
        // 4. Borrow DAI from Aave (on behalf of user)
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](4);

        calls[0] = IntentRouter.Call({
            target: address(usdc),
            callData: abi.encodeWithSignature(
                "transferFrom(address,address,uint256)", user, address(router), usdcAmount
            ),
            value: 0
        });

        calls[1] = IntentRouter.Call({
            target: address(usdc),
            callData: abi.encodeWithSignature("approve(address,uint256)", address(aavePool), usdcAmount),
            value: 0
        });

        calls[2] = IntentRouter.Call({
            target: address(aavePool),
            callData: abi.encodeWithSignature(
                "supply(address,uint256,address,uint16)", address(usdc), usdcAmount, user, uint16(0)
            ),
            value: 0
        });

        calls[3] = IntentRouter.Call({
            target: address(aavePool),
            callData: abi.encodeWithSignature(
                "borrow(address,uint256,uint256,uint16,address)",
                address(dai),
                daiBorrow,
                uint256(2),
                uint16(0),
                user
            ),
            value: 0
        });

        address[] memory tokensToSweep = new address[](0);

        // Execute through router
        vm.prank(user);
        router.executeDirect(calls, tokensToSweep);

        // Verify
        assertEq(aavePool.deposits(user, address(usdc)), usdcAmount, "USDC should be deposited");
        assertEq(dai.balanceOf(user), daiBorrow, "User should have borrowed DAI");

        console.log("Deposit+Borrow succeeded.");
        console.log("  USDC deposited:", aavePool.deposits(user, address(usdc)));
        console.log("  DAI borrowed:", dai.balanceOf(user));
    }

    // ─── Test 3: Swap + Deposit + Borrow via Router ─────────────────────

    /// @notice Full chain: Swap USDC → WETH, deposit WETH in Aave, borrow DAI.
    function test_swapDepositBorrow_throughRouter() public {
        uint256 usdcAmount = 5000 * 1e6;
        uint256 daiBorrow = 1000 * 1e18;

        // Expected WETH from swap: 5000e6 * 5e26 / 1e18 = 2.5e18 = 2.5 WETH
        uint256 expectedWeth = usdcAmount * USDC_WETH_RATE / 1e18;

        // User approves router to pull USDC
        vm.prank(user);
        usdc.approve(address(router), usdcAmount);

        // Build batched calls:
        // 1. TransferFrom USDC from user to router
        // 2. Approve swap router to spend USDC
        // 3. Swap USDC → WETH (recipient = router, so WETH stays in router)
        // 4. Approve Aave pool to spend WETH
        // 5. Supply WETH to Aave (on behalf of user)
        // 6. Borrow DAI from Aave (on behalf of user)
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](6);

        calls[0] = IntentRouter.Call({
            target: address(usdc),
            callData: abi.encodeWithSignature(
                "transferFrom(address,address,uint256)", user, address(router), usdcAmount
            ),
            value: 0
        });

        calls[1] = IntentRouter.Call({
            target: address(usdc),
            callData: abi.encodeWithSignature(
                "approve(address,uint256)", address(swapRouter), usdcAmount
            ),
            value: 0
        });

        // Swap: recipient = router (so WETH stays in router for next step)
        calls[2] = IntentRouter.Call({
            target: address(swapRouter),
            callData: abi.encodeWithSelector(
                MockSwapRouter.exactInputSingle.selector,
                MockSwapRouter.ExactInputSingleParams({
                    tokenIn: address(usdc),
                    tokenOut: address(weth),
                    fee: 3000,
                    recipient: address(router),
                    deadline: type(uint256).max,
                    amountIn: usdcAmount,
                    amountOutMinimum: 1, // Non-zero for slippage protection
                    sqrtPriceLimitX96: 0
                })
            ),
            value: 0
        });

        // After swap, router has WETH. Approve Aave to spend it.
        calls[3] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature(
                "approve(address,uint256)", address(aavePool), expectedWeth
            ),
            value: 0
        });

        calls[4] = IntentRouter.Call({
            target: address(aavePool),
            callData: abi.encodeWithSignature(
                "supply(address,uint256,address,uint16)", address(weth), expectedWeth, user, uint16(0)
            ),
            value: 0
        });

        calls[5] = IntentRouter.Call({
            target: address(aavePool),
            callData: abi.encodeWithSignature(
                "borrow(address,uint256,uint256,uint16,address)",
                address(dai),
                daiBorrow,
                uint256(2),
                uint16(0),
                user
            ),
            value: 0
        });

        address[] memory tokensToSweep = new address[](0);

        vm.prank(user);
        router.executeDirect(calls, tokensToSweep);

        // Verify
        assertEq(aavePool.deposits(user, address(weth)), expectedWeth, "WETH should be deposited");
        assertEq(dai.balanceOf(user), daiBorrow, "User should have borrowed DAI");

        console.log("Swap+Deposit+Borrow succeeded.");
        console.log("  WETH deposited:", aavePool.deposits(user, address(weth)));
        console.log("  DAI borrowed:", dai.balanceOf(user));
    }

    // ─── Test 4: Stake ETH in Lido via Router ───────────────────────────

    /// @notice Stake ETH in Lido through the router, receive stETH.
    function test_stakeETH_throughRouter() public {
        uint256 stakeAmount = 10 ether;

        // Build batched call: lido.submit{value: 10 ether}(address(0))
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);

        calls[0] = IntentRouter.Call({
            target: address(lido),
            callData: abi.encodeWithSignature("submit(address)", address(0)),
            value: stakeAmount
        });

        // Sweep stETH back to user
        address[] memory tokensToSweep = new address[](1);
        tokensToSweep[0] = address(lido); // stETH is the Lido contract itself

        vm.prank(user);
        router.executeDirect{ value: stakeAmount }(calls, tokensToSweep);

        // Verify: user should have stETH
        uint256 stethBalance = lido.balanceOf(user);
        assertEq(stethBalance, stakeAmount, "User should have stETH equal to staked ETH");
        assertEq(address(router).balance, 0, "Router should have 0 ETH");
        assertEq(lido.balanceOf(address(router)), 0, "Router should have 0 stETH (swept)");

        console.log("Stake ETH in Lido succeeded. stETH balance:", stethBalance);
    }

    // ─── Test 5: Swap USDC → WETH, unwrap to ETH, stake in Lido ────────

    /// @notice Full chain: Swap USDC → WETH, unwrap WETH → ETH, stake ETH in Lido.
    function test_swapAndStake_throughRouter() public {
        uint256 usdcAmount = 5000 * 1e6;

        // Expected WETH from swap: 5000e6 * 5e26 / 1e18 = 2.5e18 = 2.5 WETH
        uint256 expectedWeth = usdcAmount * USDC_WETH_RATE / 1e18;

        // User approves router to pull USDC
        vm.prank(user);
        usdc.approve(address(router), usdcAmount);

        // Build batched calls:
        // 1. TransferFrom USDC from user to router
        // 2. Approve swap router to spend USDC
        // 3. Swap USDC → WETH (recipient = router)
        // 4. Unwrap WETH → ETH (router gets ETH back)
        // 5. Stake ETH in Lido (router sends ETH, gets stETH)
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](5);

        calls[0] = IntentRouter.Call({
            target: address(usdc),
            callData: abi.encodeWithSignature(
                "transferFrom(address,address,uint256)", user, address(router), usdcAmount
            ),
            value: 0
        });

        calls[1] = IntentRouter.Call({
            target: address(usdc),
            callData: abi.encodeWithSignature(
                "approve(address,uint256)", address(swapRouter), usdcAmount
            ),
            value: 0
        });

        calls[2] = IntentRouter.Call({
            target: address(swapRouter),
            callData: abi.encodeWithSelector(
                MockSwapRouter.exactInputSingle.selector,
                MockSwapRouter.ExactInputSingleParams({
                    tokenIn: address(usdc),
                    tokenOut: address(weth),
                    fee: 3000,
                    recipient: address(router),
                    deadline: type(uint256).max,
                    amountIn: usdcAmount,
                    amountOutMinimum: 1, // Non-zero for slippage protection
                    sqrtPriceLimitX96: 0
                })
            ),
            value: 0
        });

        // Unwrap WETH → ETH (router has WETH from swap)
        calls[3] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("withdraw(uint256)", expectedWeth),
            value: 0
        });

        // Stake ETH in Lido (router now has ETH from unwrap)
        calls[4] = IntentRouter.Call({
            target: address(lido),
            callData: abi.encodeWithSignature("submit(address)", address(0)),
            value: expectedWeth
        });

        // Sweep stETH back to user
        address[] memory tokensToSweep = new address[](1);
        tokensToSweep[0] = address(lido);

        // No extra ETH needed — the ETH comes from unwrapping WETH
        vm.prank(user);
        router.executeDirect{ value: 0 }(calls, tokensToSweep);

        // Verify
        uint256 stethBalance = lido.balanceOf(user);
        assertEq(stethBalance, expectedWeth, "User should have stETH equal to swapped WETH");
        assertEq(usdc.balanceOf(user), 10_000 * 1e6 - usdcAmount, "User USDC should decrease");
        assertEq(address(router).balance, 0, "Router should have 0 ETH");
        assertEq(lido.balanceOf(address(router)), 0, "Router stETH should be swept");

        console.log("Swap+Unwrap+Stake succeeded. stETH balance:", stethBalance);
    }
}
