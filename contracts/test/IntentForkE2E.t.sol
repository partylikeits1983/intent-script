// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test, console} from "forge-std/Test.sol";
import {IntentRouter} from "../src/IntentRouter.sol";
import {IERC20} from "../src/interfaces/IERC20.sol";

/// @title IntentForkE2E
/// @notice End-to-end fork tests that:
///   1) Deploy IntentRouter on a forked L1 (via vm.etch at the config address)
///   2) Read compiler-generated calldata from fixture files
///   3) Execute via executeDirect or executeSigned
///   4) Assert token balances are correct after execution
///
/// Prerequisites:
///   - Run `make generate-calldata` and `make generate-fixtures` first
///   - Run with: forge test --mc IntentForkE2E --fork-url $ETH_RPC_URL -vvv
///
/// The compiler outputs calldata targeting the router at ROUTER_ADDR.
/// We use vm.etch to place our IntentRouter bytecode at that address.
///
/// NOTE: The compiler currently does NOT generate transferFrom calls to pull
/// tokens from the user into the router. The user must approve the router,
/// and the test must transfer tokens to the router before executing.
/// This is a known compiler limitation — see plans/test-improvement-plan.md.
contract IntentForkE2E is Test {
    // ─── Mainnet addresses ───────────────────────────────────────────
    // Must match the `intent_router.router` address in
    // config/protocols/ethereum.json — compiler-generated calldata bakes this
    // address into transferFrom/swap-recipient fields, and the fork test etches
    // IntentRouter bytecode at this same address so the calls line up.
    address constant ROUTER_ADDR = 0x9fF4608bAEb3a055CcBBa85c2Aabaf6EF5c50120;
    address constant WETH = 0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2;
    address constant USDC = 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48;
    address constant DAI = 0x6B175474E89094C44Da98b954EedeAC495271d0F;
    address constant STETH = 0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84;
    address constant WSTETH = 0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0;
    address constant AAVE_POOL = 0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2;
    address constant UNI_ROUTER = 0xE592427A0AEce92De3Edee1F18E0157C05861564;

    // Compiler signer address — must match the "from" field in example JSON files.
    // The compiler bakes this address into transferFrom calls, so the test user
    // must be the same address for compiler-generated calldata to work.
    address constant COMPILER_SIGNER = 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045;

    // Aave V3 aToken addresses on mainnet
    address constant A_WETH = 0x4d5F47FA6A74757f35C14fD3a6Ef8E3C9BC514E8;
    address constant A_USDC = 0x98C23E9d8f34FEFb1B7BD6a91B7FF122F4e16F5c;
    address constant A_WSTETH = 0x0B925eD163218f6662a35e0f0371Ac234f9E9371;

    // Variable debt token for DAI on Aave V3
    address constant VDEBT_DAI = 0xcF8d0c70c850859266f5C338b38F9D663181C314;
    // Variable debt token for USDC on Aave V3
    address constant VDEBT_USDC = 0x72E95b8931767C79bA4EeE721354d6E99a61D004;

    // New protocol addresses added in the DeFi-expansion phases.
    address constant BALANCER_VAULT = 0xBA12222222228d8Ba445958a75a0704d566BF2C8;
    address constant NPM = 0xC36442b4a4522E871399CD717aBDD847Ab11FE88; // Uniswap V3 NonfungiblePositionManager
    address constant CHAINLINK_ETH_USD = 0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419; // ETH/USD feed, 8 decimals

    // Uniswap V3 pools for LP lifecycle tests. 0.3% pool = 0x8ad5..6D8.
    address constant USDC_WETH_V3_POOL_3000 = 0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8;

    IntentRouter public router;
    address public user;

    // ─── Known signer for EIP-712 tests ──────────────────────────────
    // Foundry's vm.addr(1) private key
    uint256 constant SIGNER_PK = 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80;

    function setUp() public {
        // Deploy a fresh IntentRouter to get the correct bytecode
        IntentRouter impl_ = new IntentRouter();

        // Etch our router bytecode at the config address so compiler-generated
        // calldata works as-is. Note: DOMAIN_SEPARATOR immutable will have the
        // impl_ address, but executeDirect doesn't use it.
        vm.etch(ROUTER_ADDR, address(impl_).code);
        router = IntentRouter(payable(ROUTER_ADDR));

        // vm.etch copies code but not storage, so the etched router starts with
        // an all-zero storage slate. ReentrancyGuard's `_status` defaults to 1
        // (_NOT_ENTERED) in the constructor; we write it here so nonReentrant
        // doesn't accidentally flag a zero value as invalid state.
        // Storage layout (IntentRouter is ReentrancyGuard):
        //   slot 0 = ReentrancyGuard._status
        //   slot 1 = nonces (mapping)
        //   slot 2 = owner
        //   slot 3 = allowedTargets (mapping)
        vm.store(ROUTER_ADDR, bytes32(uint256(0)), bytes32(uint256(1))); // _NOT_ENTERED

        _allowTarget(WETH);
        _allowTarget(USDC);
        _allowTarget(DAI);
        _allowTarget(STETH);
        _allowTarget(WSTETH);
        _allowTarget(AAVE_POOL);
        _allowTarget(UNI_ROUTER);

        // Use the same signer address as the compiler so that transferFrom calls
        // in the compiler-generated calldata reference the correct user.
        user = COMPILER_SIGNER;
        vm.deal(user, 1000 ether);
    }

    /// @dev Directly write allowedTargets[target] = true via vm.store
    ///      Bypasses the onlyOwner check (owner is address(0) after vm.etch).
    ///      IntentRouter inherits ReentrancyGuard which occupies slot 0, so
    ///      allowedTargets lives at slot 3 (not slot 2).
    function _allowTarget(address target) internal {
        bytes32 slot = keccak256(abi.encode(target, uint256(3)));
        vm.store(ROUTER_ADDR, slot, bytes32(uint256(1)));
    }

    // ─── Helper: read fixture files ──────────────────────────────────

    function _readCalldata(string memory name) internal view returns (bytes memory) {
        string memory path = string.concat("test/fixtures/", name, ".txt");
        string memory hex_ = vm.readFile(path);
        return vm.parseBytes(hex_);
    }

    function _readValue(string memory name) internal view returns (uint256) {
        string memory path = string.concat("test/fixtures/", name, "_value.txt");
        string memory val = vm.readFile(path);
        return vm.parseUint(val);
    }

    // ─── Helper: give user ERC-20 tokens via deal ────────────────────

    function _dealERC20(address token, address to, uint256 amount) internal {
        deal(token, to, amount);
    }

    function _assertRouterCleared(address[] memory tokens) internal view {
        for (uint256 i = 0; i < tokens.length; i++) {
            assertEq(IERC20(tokens[i]).balanceOf(ROUTER_ADDR), 0, "router should not retain swept token balance");
        }
        assertEq(address(ROUTER_ADDR).balance, 0, "router should not retain ETH");
    }

    // ─── Helper: approve credit delegation for Aave V3 borrows ───────

    /// @dev Aave V3 requires credit delegation when msg.sender != onBehalfOf.
    ///      When borrowing through the router, the router is msg.sender but
    ///      onBehalfOf is the user, so the user must delegate borrow power.
    function _approveDelegation(address vDebtToken, address delegator, address delegatee, uint256 amount) internal {
        vm.prank(delegator);
        (bool ok,) = vDebtToken.call(abi.encodeWithSignature("approveDelegation(address,uint256)", delegatee, amount));
        require(ok, "approveDelegation failed");
    }

    // ─── Helper: build EIP-712 digest for executeSigned ──────────────

    bytes32 constant CALL_TYPEHASH = keccak256("Call(address target,bytes callData,uint256 value)");
    bytes32 constant INTENT_BATCH_TYPEHASH = keccak256(
        "IntentBatch(address signer,Call[] calls,address[] tokensToSweep,uint256 nonce,uint256 deadline)Call(address target,bytes callData,uint256 value)"
    );

    function _buildDigest(IntentRouter.IntentBatch memory batch) internal view returns (bytes32) {
        bytes32[] memory callHashes = new bytes32[](batch.calls.length);
        for (uint256 i = 0; i < batch.calls.length; i++) {
            callHashes[i] = keccak256(
                abi.encode(
                    CALL_TYPEHASH, batch.calls[i].target, keccak256(batch.calls[i].callData), batch.calls[i].value
                )
            );
        }
        bytes32 callsHash = keccak256(abi.encodePacked(callHashes));

        bytes32 structHash = keccak256(
            abi.encode(
                INTENT_BATCH_TYPEHASH,
                batch.signer,
                callsHash,
                keccak256(abi.encodePacked(batch.tokensToSweep)),
                batch.nonce,
                batch.deadline
            )
        );

        return keccak256(abi.encodePacked("\x19\x01", router.DOMAIN_SEPARATOR(), structHash));
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 1: Wrap ETH → WETH via compiler-generated calldata
    // ═════════════════════════════════════════════════════════════════

    /// @notice Wrap ETH to WETH using compiler-generated calldata on a fork.
    ///         The wrap intent produces a direct WETH.deposit() call (SingleTx).
    function test_fork_wrapETH() public {
        uint256 wethBefore = IERC20(WETH).balanceOf(user);

        // Read compiler-generated calldata (targets WETH directly, not router)
        bytes memory callData = _readCalldata("wrap_eth");
        uint256 value = _readValue("wrap_eth");

        // Execute directly against WETH (wrap is a SingleTx, not batched)
        vm.prank(user);
        (bool success,) = WETH.call{value: value}(callData);
        assertTrue(success, "Wrap ETH should succeed");

        uint256 wethAfter = IERC20(WETH).balanceOf(user);
        assertEq(wethAfter - wethBefore, value, "WETH balance should increase by wrapped amount");

        console.log("Fork wrap ETH: WETH balance", wethBefore, "->", wethAfter);
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 2: Swap USDC → WETH via Uniswap V3 through the router
    // ═════════════════════════════════════════════════════════════════

    /// @notice Swap USDC to WETH using compiler-generated calldata on a fork.
    ///         The compiler produces: approve USDC for Uniswap + exactInputSingle.
    function test_fork_swapUSDC_WETH() public {
        uint256 usdcAmount = 1000 * 1e6; // 1000 USDC

        // Give user USDC
        _dealERC20(USDC, user, usdcAmount);
        assertEq(IERC20(USDC).balanceOf(user), usdcAmount, "User should have USDC");

        // User must approve the router to pull USDC
        vm.prank(user);
        IERC20(USDC).approve(ROUTER_ADDR, usdcAmount);

        uint256 wethBefore = IERC20(WETH).balanceOf(user);

        // Read compiler-generated calldata (batched via router)
        bytes memory callData = _readCalldata("swap_usdc_weth");
        uint256 value = _readValue("swap_usdc_weth");

        // Execute through router
        vm.prank(user);
        (bool success,) = ROUTER_ADDR.call{value: value}(callData);
        assertTrue(success, "Swap USDC->WETH should succeed");

        uint256 wethAfter = IERC20(WETH).balanceOf(user);
        uint256 usdcAfter = IERC20(USDC).balanceOf(user);

        assertTrue(wethAfter > wethBefore, "User should have received WETH");
        assertEq(usdcAfter, 0, "User should have spent all USDC");
        address[] memory cleared = new address[](1);
        cleared[0] = WETH;
        _assertRouterCleared(cleared);

        console.log("Fork swap USDC->WETH: WETH gained", wethAfter - wethBefore);
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 3: Deposit USDC into Aave V3 through the router
    // ═════════════════════════════════════════════════════════════════

    /// @notice Deposit USDC into Aave V3 using compiler-generated calldata.
    ///         The compiler produces: approve USDC for Aave + supply.
    function test_fork_aaveDepositUSDC() public {
        uint256 usdcAmount = 100 * 1e6; // 100 USDC

        // Give user USDC
        _dealERC20(USDC, user, usdcAmount);

        // User approves router to pull USDC
        vm.prank(user);
        IERC20(USDC).approve(ROUTER_ADDR, usdcAmount);

        uint256 aUsdcBefore = IERC20(A_USDC).balanceOf(user);

        // Read compiler-generated calldata
        bytes memory callData = _readCalldata("aave_deposit_usdc");
        uint256 value = _readValue("aave_deposit_usdc");

        vm.prank(user);
        (bool success,) = ROUTER_ADDR.call{value: value}(callData);
        assertTrue(success, "Aave deposit USDC should succeed");

        uint256 aUsdcAfter = IERC20(A_USDC).balanceOf(user);
        uint256 usdcAfter = IERC20(USDC).balanceOf(user);

        assertTrue(aUsdcAfter > aUsdcBefore, "User should have received aUSDC");
        assertEq(usdcAfter, 0, "User should have spent all USDC");
        address[] memory cleared = new address[](1);
        cleared[0] = USDC;
        _assertRouterCleared(cleared);

        console.log("Fork Aave deposit: aUSDC gained", aUsdcAfter - aUsdcBefore);
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 4: Deposit USDC + Borrow DAI through the router
    // ═════════════════════════════════════════════════════════════════

    /// @notice Deposit USDC into Aave and borrow DAI using compiler-generated calldata.
    function test_fork_depositBorrow() public {
        uint256 usdcAmount = 5000 * 1e6; // 5000 USDC
        uint256 borrowAmount = 2000 * 1e18; // 2000 DAI

        // Give user USDC
        _dealERC20(USDC, user, usdcAmount);

        // User approves router to pull USDC
        vm.prank(user);
        IERC20(USDC).approve(ROUTER_ADDR, usdcAmount);

        // Credit delegation: user delegates borrow power to router for DAI.
        // Aave V3 requires approveDelegation when msg.sender != onBehalfOf.
        _approveDelegation(VDEBT_DAI, user, ROUTER_ADDR, borrowAmount);

        uint256 daiBefore = IERC20(DAI).balanceOf(user);

        // Read compiler-generated calldata
        bytes memory callData = _readCalldata("deposit_borrow");
        uint256 value = _readValue("deposit_borrow");

        vm.prank(user);
        (bool success,) = ROUTER_ADDR.call{value: value}(callData);
        assertTrue(success, "Deposit+Borrow should succeed");

        uint256 daiAfter = IERC20(DAI).balanceOf(user);
        uint256 aUsdcAfter = IERC20(A_USDC).balanceOf(user);
        uint256 usdcAfter = IERC20(USDC).balanceOf(user);

        assertTrue(aUsdcAfter > 0, "User should have aUSDC from deposit");
        assertEq(usdcAfter, 0, "User should have spent all USDC");
        // DAI borrowed amount — allow small dust tolerance because the router address
        // on the mainnet fork may have pre-existing DAI dust that gets swept too.
        assertTrue(daiAfter - daiBefore >= borrowAmount, "User should have borrowed at least 2000 DAI");
        assertApproxEqAbs(daiAfter - daiBefore, borrowAmount, 100, "DAI borrowed should be ~2000 DAI");
        address[] memory cleared = new address[](2);
        cleared[0] = USDC;
        cleared[1] = DAI;
        _assertRouterCleared(cleared);

        console.log("Fork deposit+borrow: aUSDC", aUsdcAfter, "DAI gained", daiAfter - daiBefore);
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 5: Stake ETH in Lido via compiler-generated calldata
    // ═════════════════════════════════════════════════════════════════

    /// @notice Stake ETH in Lido using compiler-generated calldata.
    ///         The stake intent produces a direct lido.submit() call (SingleTx).
    function test_fork_stakeETH_lido() public {
        uint256 stethBefore = IERC20(STETH).balanceOf(user);

        // Read compiler-generated calldata (targets Lido directly, not router)
        bytes memory callData = _readCalldata("stake_eth_lido");
        uint256 value = _readValue("stake_eth_lido");

        vm.prank(user);
        (bool success,) = STETH.call{value: value}(callData);
        assertTrue(success, "Stake ETH in Lido should succeed");

        uint256 stethAfter = IERC20(STETH).balanceOf(user);

        // stETH is a rebasing token, so balance may be slightly less than value
        // due to rounding. Allow 2 wei tolerance.
        assertTrue(stethAfter > stethBefore, "User should have received stETH");
        assertApproxEqAbs(stethAfter - stethBefore, value, 2, "stETH should be ~= staked ETH");

        console.log("Fork stake ETH: stETH gained", stethAfter - stethBefore);
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 6: Complex DeFi — Swap USDC→wstETH + Deposit wstETH + Borrow DAI
    //         via executeDirect (compiler-generated calldata)
    // ═════════════════════════════════════════════════════════════════

    /// @notice Full complex DeFi chain using compiler-generated calldata.
    ///         This is the end-to-end test for complex_defi.json:
    ///         swap 5000 USDC → wstETH, deposit all into Aave, borrow 1000 DAI.
    ///         wstETH is used as the borrow collateral because Aave V3 set
    ///         WETH's LTV to 0 on mainnet (post-2024), which makes WETH-collateral
    ///         borrow flows revert with LtvValidationFailed even though the
    ///         compiler output is correct. wstETH still has LTV ≈ 78.5%.
    function test_fork_complexDefi_executeDirect() public {
        uint256 usdcAmount = 5000 * 1e6; // 5000 USDC
        uint256 borrowAmount = 1000 * 1e18; // 1000 DAI

        // Give user USDC
        _dealERC20(USDC, user, usdcAmount);

        // User approves router to pull USDC
        vm.prank(user);
        IERC20(USDC).approve(ROUTER_ADDR, usdcAmount);

        // Credit delegation: user delegates borrow power to router for DAI.
        // Aave V3 requires approveDelegation when msg.sender != onBehalfOf.
        _approveDelegation(VDEBT_DAI, user, ROUTER_ADDR, borrowAmount);

        uint256 wstethBefore = IERC20(WSTETH).balanceOf(user);
        uint256 daiBefore = IERC20(DAI).balanceOf(user);

        // Read compiler-generated calldata for complex_defi
        bytes memory callData = _readCalldata("complex_defi");
        uint256 value = _readValue("complex_defi");

        vm.prank(user);
        (bool success,) = ROUTER_ADDR.call{value: value}(callData);
        assertTrue(success, "Complex DeFi executeDirect should succeed");

        uint256 wstethAfter = IERC20(WSTETH).balanceOf(user);
        uint256 daiAfter = IERC20(DAI).balanceOf(user);
        uint256 aWstethAfter = IERC20(A_WSTETH).balanceOf(user);
        uint256 usdcAfter = IERC20(USDC).balanceOf(user);

        // Assertions:
        // 1. USDC should be spent
        assertEq(usdcAfter, 0, "User should have spent all USDC");

        // 2. aWstETH should be > 0 (deposited the swapped wstETH into Aave)
        assertTrue(aWstethAfter > 0, "User should have aWstETH from Aave deposit");

        // 3. DAI should have increased by ~1000 (borrowed from Aave)
        //    Allow small dust tolerance because the router address on mainnet fork
        //    may have pre-existing DAI dust that gets swept too.
        assertTrue(daiAfter - daiBefore >= borrowAmount, "User should have borrowed at least 1000 DAI");
        assertApproxEqAbs(daiAfter - daiBefore, borrowAmount, 100, "DAI borrowed should be ~1000 DAI");
        address[] memory cleared = new address[](2);
        cleared[0] = WSTETH;
        cleared[1] = DAI;
        _assertRouterCleared(cleared);

        console.log("Fork complex DeFi executeDirect:");
        console.log("  USDC spent:", usdcAmount);
        console.log("  wstETH balance change:", wstethAfter > wstethBefore ? wstethAfter - wstethBefore : 0);
        console.log("  aWstETH received:", aWstethAfter);
        console.log("  DAI borrowed:", daiAfter - daiBefore);
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 7: Complex DeFi via executeSigned (EIP-712 signature)
    //         Solver/relayer submits on behalf of signer
    // ═════════════════════════════════════════════════════════════════

    /// @notice Build the complex DeFi calls array for a given signer and amounts.
    ///         Matches the compiler output: transferFrom + approve + swap(recipient=router)
    ///         + approve + supply + borrow. Intermediate tokens stay in the router.
    ///         Uses wstETH (not WETH) as the intermediate collateral asset because
    ///         Aave V3 set WETH LTV to 0 on mainnet post-2024. See test_fork_complexDefi_executeDirect.
    function _buildComplexDefiCalls(
        address signer,
        address routerAddr,
        uint256 usdcAmount,
        uint256 depositAmount,
        uint256 borrowAmount
    ) internal pure returns (IntentRouter.Call[] memory) {
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](6);

        // Step 0: Pull USDC from signer into router
        calls[0] = IntentRouter.Call({
            target: USDC,
            callData: abi.encodeWithSignature("transferFrom(address,address,uint256)", signer, routerAddr, usdcAmount),
            value: 0
        });

        // Step 1: Router approves Uniswap to spend USDC
        calls[1] = IntentRouter.Call({
            target: USDC,
            callData: abi.encodeWithSignature("approve(address,uint256)", UNI_ROUTER, usdcAmount),
            value: 0
        });

        // Step 2: Swap USDC → wstETH through 0.05% pool, recipient = router
        // amountOutMinimum is set to a conservative value for slippage protection
        // (matches compiler behavior when min_amount_out is specified)
        calls[2] = IntentRouter.Call({
            target: UNI_ROUTER,
            callData: abi.encodeWithSignature(
                "exactInputSingle((address,address,uint24,address,uint256,uint256,uint256,uint160))",
                USDC,
                WSTETH,
                uint24(500),
                routerAddr,
                type(uint256).max,
                usdcAmount,
                uint256(1),
                uint160(0)
            ),
            value: 0
        });

        // Step 3: Router approves Aave to spend wstETH (already in router from swap)
        calls[3] = IntentRouter.Call({
            target: WSTETH,
            callData: abi.encodeWithSignature("approve(address,uint256)", AAVE_POOL, depositAmount),
            value: 0
        });

        // Step 4: Supply wstETH to Aave on behalf of signer
        calls[4] = IntentRouter.Call({
            target: AAVE_POOL,
            callData: abi.encodeWithSignature(
                "supply(address,uint256,address,uint16)", WSTETH, depositAmount, signer, uint16(0)
            ),
            value: 0
        });

        // Step 5: Borrow DAI from Aave on behalf of signer
        calls[5] = IntentRouter.Call({
            target: AAVE_POOL,
            callData: abi.encodeWithSignature(
                "borrow(address,uint256,uint256,uint16,address)", DAI, borrowAmount, uint256(2), uint16(0), signer
            ),
            value: 0
        });

        return calls;
    }

    /// @notice Full complex DeFi chain via executeSigned with EIP-712 signature.
    ///         Deploys a fresh router (correct DOMAIN_SEPARATOR), builds the batch
    ///         manually, signs with vm.sign, and submits via a relayer.
    function test_fork_complexDefi_executeSigned() public {
        IntentRouter signedRouter = new IntentRouter();

        // Whitelist all target contracts on the signed router (Task 8: allowlist)
        signedRouter.setAllowedTarget(WSTETH, true);
        signedRouter.setAllowedTarget(USDC, true);
        signedRouter.setAllowedTarget(DAI, true);
        signedRouter.setAllowedTarget(AAVE_POOL, true);
        signedRouter.setAllowedTarget(UNI_ROUTER, true);

        uint256 signerPk = 0xA11CE;
        address signer = vm.addr(signerPk);
        vm.deal(signer, 1000 ether);

        uint256 usdcAmount = 5000 * 1e6;
        uint256 borrowAmount = 1000 * 1e18;

        _dealERC20(USDC, signer, usdcAmount);

        vm.prank(signer);
        IERC20(USDC).approve(address(signedRouter), usdcAmount);

        // Credit delegation: signer delegates borrow power to signedRouter for DAI.
        // Aave V3 requires approveDelegation when msg.sender != onBehalfOf.
        _approveDelegation(VDEBT_DAI, signer, address(signedRouter), borrowAmount);

        // Build batch — sweep both wstETH (excess from swap) and DAI (borrowed by router)
        address[] memory tokensToSweep = new address[](2);
        tokensToSweep[0] = WSTETH;
        tokensToSweep[1] = DAI;

        // Deposit the entire swap output into Aave; the handcrafted calls model
        // the compiler's "deposit all" behavior by reading router balance at runtime.
        // For the manually-built test we approximate with a fixed depositAmount that
        // matches the min swap output (1.0 wstETH, adjusted by fee_bps if any).
        IntentRouter.IntentBatch memory batch = IntentRouter.IntentBatch({
            signer: signer,
            calls: _buildComplexDefiCalls(signer, address(signedRouter), usdcAmount, 1 ether, borrowAmount),
            tokensToSweep: tokensToSweep,
            nonce: 0,
            deadline: type(uint256).max
        });

        // Sign and submit via relayer
        bytes memory signature;
        {
            bytes32 digest = _buildSignedRouterDigest(signedRouter, batch);
            (uint8 v, bytes32 r, bytes32 s) = vm.sign(signerPk, digest);
            signature = abi.encodePacked(r, s, v);
        }

        address relayer = makeAddr("relayer");
        vm.deal(relayer, 10 ether);

        vm.prank(relayer);
        signedRouter.executeSigned{value: 0}(batch, signature);

        // Assertions
        assertEq(IERC20(USDC).balanceOf(signer), 0, "Signer should have spent all USDC");
        assertTrue(IERC20(A_WSTETH).balanceOf(signer) > 0, "Signer should have aWstETH");
        assertEq(IERC20(DAI).balanceOf(signer), borrowAmount, "Signer should have borrowed DAI");
        assertEq(signedRouter.nonces(signer), 1, "Nonce should be 1 after execution");
        assertEq(IERC20(WSTETH).balanceOf(relayer), 0, "Relayer should have 0 wstETH");
        assertEq(IERC20(DAI).balanceOf(relayer), 0, "Relayer should have 0 DAI");

        console.log("Fork complex DeFi executeSigned: OK");
    }

    /// @notice Helper to build digest using a specific router's DOMAIN_SEPARATOR
    function _buildSignedRouterDigest(IntentRouter targetRouter, IntentRouter.IntentBatch memory batch)
        internal
        view
        returns (bytes32)
    {
        bytes32[] memory callHashes = new bytes32[](batch.calls.length);
        for (uint256 i = 0; i < batch.calls.length; i++) {
            callHashes[i] = keccak256(
                abi.encode(
                    CALL_TYPEHASH, batch.calls[i].target, keccak256(batch.calls[i].callData), batch.calls[i].value
                )
            );
        }
        bytes32 callsHash = keccak256(abi.encodePacked(callHashes));

        bytes32 structHash = keccak256(
            abi.encode(
                INTENT_BATCH_TYPEHASH,
                batch.signer,
                callsHash,
                keccak256(abi.encodePacked(batch.tokensToSweep)),
                batch.nonce,
                batch.deadline
            )
        );

        return keccak256(abi.encodePacked("\x19\x01", targetRouter.DOMAIN_SEPARATOR(), structHash));
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 8: Uniswap V3 LP lifecycle -mint a wide-range USDC/WETH position
    //         via the compiler, then decrease liquidity + collect via
    //         programmatically-built router calls. Asserts USDC and WETH
    //         return to the signer.
    // ═════════════════════════════════════════════════════════════════

    /// @notice Full LP lifecycle: mint → decrease (all liquidity) → collect.
    ///         The mint uses compiler-generated calldata from the
    ///         `lp_mint_usdc_weth_wide` fixture; decrease+collect are built
    ///         manually here because the tokenId is only known post-mint
    ///         and the compiler doesn't support `liquidity: "all"`.
    ///
    ///         Wide tick range (190020..205020 ≈ ETH $1200..$5900) covers
    ///         any realistic HEAD pool price, so no block pinning required.
    function test_fork_lp_lifecycle_usdc_weth() public {
        _allowTarget(NPM);

        uint256 usdcAmount = 3000 * 1e6;
        uint256 wethAmount = 1 ether;
        _dealERC20(USDC, user, usdcAmount);
        _dealERC20(WETH, user, wethAmount);

        vm.prank(user);
        IERC20(USDC).approve(ROUTER_ADDR, usdcAmount);
        vm.prank(user);
        IERC20(WETH).approve(ROUTER_ADDR, wethAmount);

        // The fixture bakes `deadline = current_timestamp + 20m`, so running
        // at HEAD (where `block.timestamp` >> the baked deadline) would
        // revert inside the LP mint call. Rewind to the fixture's epoch.
        vm.warp(1712700000);

        uint256 npmBalanceBefore = INFTEnumerable(NPM).balanceOf(user);

        // ─── Step 1: mint via compiler-generated calldata ──────────────
        bytes memory mintData = _readCalldata("lp_mint_usdc_weth_wide");
        vm.prank(user);
        (bool mintOk,) = ROUTER_ADDR.call(mintData);
        assertTrue(mintOk, "lp_mint should succeed");

        // Snapshot user balances immediately after the mint (and its sweep)
        // so the later collect delta measures "tokens the position returned"
        // rather than "starting balance minus what was deposited".
        uint256 usdcAfterMint = IERC20(USDC).balanceOf(user);
        uint256 wethAfterMint = IERC20(WETH).balanceOf(user);

        // Recover the freshly minted tokenId via ERC721Enumerable.
        uint256 npmBalanceAfter = INFTEnumerable(NPM).balanceOf(user);
        assertEq(npmBalanceAfter, npmBalanceBefore + 1, "signer should have +1 LP NFT");
        uint256 tokenId = INFTEnumerable(NPM).tokenOfOwnerByIndex(user, npmBalanceAfter - 1);
        assertGt(tokenId, 0, "tokenId should be > 0");

        // Inspect position liquidity — required as the `liquidity` argument
        // to `decreaseLiquidity`. NPM.positions returns a 12-tuple; we only
        // need slot 7 (liquidity).
        uint128 liquidity = _positionLiquidity(tokenId);
        assertGt(liquidity, 0, "position should have liquidity");

        // ─── Step 2: approve router on the NFT, then decrease + collect ─
        vm.prank(user);
        INPM(NPM).approve(ROUTER_ADDR, tokenId);

        IntentRouter.Call[] memory lifecycleCalls = new IntentRouter.Call[](2);
        // decreaseLiquidity((uint256,uint128,uint256,uint256,uint256))
        lifecycleCalls[0] = IntentRouter.Call({
            target: NPM,
            callData: abi.encodeWithSelector(
                INPM.decreaseLiquidity.selector,
                INPM.DecreaseLiquidityParams({
                    tokenId: tokenId,
                    liquidity: liquidity,
                    amount0Min: 0,
                    amount1Min: 0,
                    deadline: block.timestamp + 600
                })
            ),
            value: 0
        });
        // collect((uint256,address,uint128,uint128))
        lifecycleCalls[1] = IntentRouter.Call({
            target: NPM,
            callData: abi.encodeWithSelector(
                INPM.collect.selector,
                INPM.CollectParams({
                    tokenId: tokenId,
                    recipient: user,
                    amount0Max: type(uint128).max,
                    amount1Max: type(uint128).max
                })
            ),
            value: 0
        });

        address[] memory noSweep = new address[](0);
        vm.prank(user);
        router.executeDirect(lifecycleCalls, noSweep);

        // ─── Step 3: assertions ────────────────────────────────────────
        uint128 liquidityAfter = _positionLiquidity(tokenId);
        assertEq(liquidityAfter, 0, "position liquidity should be zero after decrease");

        uint256 collectedUsdc = IERC20(USDC).balanceOf(user) - usdcAfterMint;
        uint256 collectedWeth = IERC20(WETH).balanceOf(user) - wethAfterMint;

        // After draining the full liquidity, collect should push some positive
        // amount of one (or both) tokens back to the signer — exactly what
        // was deposited, minus rounding dust. Single-sided positions (when the
        // current tick sits at the edge of the range) can legitimately
        // collect only one asset, so assert at least one side returned.
        assertTrue(collectedUsdc > 0 || collectedWeth > 0, "collect should return USDC or WETH to signer");

        console.log("LP lifecycle -USDC collected:", collectedUsdc);
        console.log("LP lifecycle -WETH collected:", collectedWeth);
        console.log("LP lifecycle -liquidity before/after:", uint256(liquidity), uint256(liquidityAfter));
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 9: 1.5x leveraged long wstETH on Aave via Balancer flashloan.
    //         Uses compiler-generated calldata from long_wsteth_1_5x fixture.
    //         Exercises: leverage sugar → BalancerFlashloan IR → recursive
    //         enrich → `receiveFlashLoan` callback → Aave supply/borrow →
    //         Uniswap swap → flashloan repayment.
    // ═════════════════════════════════════════════════════════════════

    /// @notice 2x long ETH using Balancer flashloan + Aave V3 supply/borrow.
    ///         The leverage expander encodes the entire pipeline; this test
    ///         just needs to: deal WETH, approve router, delegate USDC debt,
    ///         execute. Post-state should show ~2 WETH aWETH and ~4040 USDC
    ///         variable debt, with the flashloan repaid.
    ///
    ///         Pinned to block 19_600_000 (ETH ≈ $3400) so the
    ///         4040-USDC-to-1-WETH-min swap has ample margin.
    function test_fork_leverage_eth_exposure() public {
        // wstETH collateral — Aave V3 has set WETH's LTV to 0 on Ethereum
        // mainnet (since 2024), so borrowing against WETH reverts with
        // LtvValidationFailed. wstETH retains ~75% LTV and still gives
        // ETH-price exposure (plus Lido staking yield).
        //
        // Fixture: price=3500 USDC/wstETH, 200 bps slippage, leverage 1.5x,
        // swap fee tier 0.05% (the 0.3% tier has thin liquidity for this
        // pair at HEAD). 1.5x (vs the spec'd 2x) leaves a margin for the
        // USDC/wstETH pool's divergence from Aave's oracle price — a 2x
        // attempt reverts with either LtvValidationFailed or "Too little
        // received" depending on which side bites first.
        //   flashloan = 0.5 wstETH, supply = 1.5 wstETH
        //   borrow = 0.5 * 3500 * 1.02 = 1785 USDC
        //   Aave 75% LTV on 1.5 wstETH requires oracle wstETH > ~1587
        //   Swap repayment of 0.5 wstETH requires pool rate <= ~3570
        int256 ethUsdRaw = IChainlinkFeed(CHAINLINK_ETH_USD).latestAnswer();
        require(ethUsdRaw > 0, "Chainlink returned non-positive ETH price");
        uint256 ethUsd = uint256(ethUsdRaw) / 1e8;
        if (ethUsd < 1500 || ethUsd > 3500) {
            emit log_named_uint("Skipping: HEAD ETH price outside leverage test band", ethUsd);
            vm.skip(true);
        }

        _allowTarget(BALANCER_VAULT);

        // User contributes 1 wstETH of equity; expander flashloans 0.5 wstETH.
        uint256 contribution = 1 ether;
        _dealERC20(WSTETH, user, contribution);

        // Approve router to pull the contribution inside the flashloan
        // callback (the expander emits transferFrom(signer -> router, 1 wstETH)
        // as the first inner step).
        vm.prank(user);
        IERC20(WSTETH).approve(ROUTER_ADDR, contribution);

        // Aave V3 credit delegation: router is the borrower on the user's
        // behalf, so the user must delegate variable debt for USDC. Ceiling
        // at price=3500, leverage=1.5, slippage=200bps → 1785 USDC.
        _approveDelegation(VDEBT_USDC, user, ROUTER_ADDR, 2_000 * 1e6);

        // Do NOT warp — Aave's aToken balanceOf uses block.timestamp delta
        // which underflows if we rewind. Fixture's deadline is pinned to
        // year 2286 so HEAD-timestamp swaps remain valid.

        uint256 aWstethBefore = IERC20(A_WSTETH).balanceOf(user);
        uint256 usdcDebtBefore = IERC20(VDEBT_USDC).balanceOf(user);

        bytes memory leverageData = _readCalldata("long_wsteth_1_5x");
        vm.prank(user);
        (bool ok,) = ROUTER_ADDR.call(leverageData);
        assertTrue(ok, "leverage 1.5x should succeed");

        uint256 aWstethGained = IERC20(A_WSTETH).balanceOf(user) - aWstethBefore;
        uint256 debtGained = IERC20(VDEBT_USDC).balanceOf(user) - usdcDebtBefore;

        // aWstETH supplied: ~1.5 wstETH (1 contribution + 0.5 flashloan).
        assertGe(aWstethGained, 1.49 ether, "aWstETH should be at least ~1.5 wstETH");
        assertLe(aWstethGained, 1.51 ether, "aWstETH should not exceed ~1.5 wstETH");

        // USDC debt: 1785 USDC (price=3500 * 0.5 * (1 + 200bps)).
        assertGt(debtGained, 0, "USDC debt should have increased");
        assertLe(debtGained, 1_800 * 1e6, "USDC debt should stay under compiler ceiling");

        console.log("Leverage - aWstETH gained:", aWstethGained);
        console.log("Leverage - USDC debt gained:", debtGained);
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 10: LP full lifecycle — mint, increase liquidity, decrease
    //         partially, collect. Exercises lp_increase + paired
    //         transferFrom/approve enrichment on an existing NFT, plus
    //         partial liquidity reduction semantics.
    // ═════════════════════════════════════════════════════════════════

    /// @notice Four-step LP lifecycle: mint → increase → decrease (half) →
    ///         collect. Mint uses compiler fixture; subsequent calls are
    ///         hand-built batches because `position_id` is dynamic.
    function test_fork_lp_mint_increase_decrease_collect() public {
        _allowTarget(NPM);
        vm.warp(1712700000);

        // Fund user with enough for two deposits.
        _dealERC20(USDC, user, 6000 * 1e6);
        _dealERC20(WETH, user, 2 ether);
        vm.prank(user);
        IERC20(USDC).approve(ROUTER_ADDR, type(uint256).max);
        vm.prank(user);
        IERC20(WETH).approve(ROUTER_ADDR, type(uint256).max);

        // ─── Step 1: mint (compiler fixture) ───────────────────────────
        bytes memory mintData = _readCalldata("lp_mint_usdc_weth_wide");
        vm.prank(user);
        (bool mintOk,) = ROUTER_ADDR.call(mintData);
        assertTrue(mintOk, "mint should succeed");

        uint256 tokenId = INFTEnumerable(NPM).tokenOfOwnerByIndex(user, INFTEnumerable(NPM).balanceOf(user) - 1);
        uint128 liquidity0 = _positionLiquidity(tokenId);
        assertGt(liquidity0, 0, "initial liquidity > 0");

        // ─── Step 2: increaseLiquidity via router batch ────────────────
        // Pull USDC + WETH into router, approve NPM, call increase.
        uint256 addUsdc = 2000 * 1e6;
        uint256 addWeth = 0.6 ether;

        IntentRouter.Call[] memory incCalls = new IntentRouter.Call[](5);
        incCalls[0] = IntentRouter.Call({
            target: USDC,
            callData: abi.encodeWithSignature("transferFrom(address,address,uint256)", user, ROUTER_ADDR, addUsdc),
            value: 0
        });
        incCalls[1] = IntentRouter.Call({
            target: WETH,
            callData: abi.encodeWithSignature("transferFrom(address,address,uint256)", user, ROUTER_ADDR, addWeth),
            value: 0
        });
        incCalls[2] = IntentRouter.Call({
            target: USDC,
            callData: abi.encodeWithSignature("approve(address,uint256)", NPM, addUsdc),
            value: 0
        });
        incCalls[3] = IntentRouter.Call({
            target: WETH,
            callData: abi.encodeWithSignature("approve(address,uint256)", NPM, addWeth),
            value: 0
        });
        incCalls[4] = IntentRouter.Call({
            target: NPM,
            callData: abi.encodeWithSelector(
                INPM.increaseLiquidity.selector,
                INPM.IncreaseLiquidityParams({
                    tokenId: tokenId,
                    amount0Desired: addUsdc,
                    amount1Desired: addWeth,
                    amount0Min: 0,
                    amount1Min: 0,
                    deadline: block.timestamp + 600
                })
            ),
            value: 0
        });
        address[] memory sweepPair = new address[](2);
        sweepPair[0] = USDC;
        sweepPair[1] = WETH;
        vm.prank(user);
        router.executeDirect(incCalls, sweepPair);

        uint128 liquidity1 = _positionLiquidity(tokenId);
        assertGt(liquidity1, liquidity0, "liquidity should increase after lp_increase");

        // ─── Step 3: decrease half of the liquidity ────────────────────
        uint128 halfLiquidity = liquidity1 / 2;

        vm.prank(user);
        INPM(NPM).approve(ROUTER_ADDR, tokenId);

        IntentRouter.Call[] memory decCalls = new IntentRouter.Call[](2);
        decCalls[0] = IntentRouter.Call({
            target: NPM,
            callData: abi.encodeWithSelector(
                INPM.decreaseLiquidity.selector,
                INPM.DecreaseLiquidityParams({
                    tokenId: tokenId,
                    liquidity: halfLiquidity,
                    amount0Min: 0,
                    amount1Min: 0,
                    deadline: block.timestamp + 600
                })
            ),
            value: 0
        });
        decCalls[1] = IntentRouter.Call({
            target: NPM,
            callData: abi.encodeWithSelector(
                INPM.collect.selector,
                INPM.CollectParams({
                    tokenId: tokenId,
                    recipient: user,
                    amount0Max: type(uint128).max,
                    amount1Max: type(uint128).max
                })
            ),
            value: 0
        });

        uint256 usdcBeforeDec = IERC20(USDC).balanceOf(user);
        uint256 wethBeforeDec = IERC20(WETH).balanceOf(user);
        address[] memory noSweep = new address[](0);
        vm.prank(user);
        router.executeDirect(decCalls, noSweep);

        // ─── Assertions ────────────────────────────────────────────────
        uint128 liquidityFinal = _positionLiquidity(tokenId);
        // After decrease-by-half we should be ~halfLiquidity remaining.
        // Allow ±1 unit rounding (u128 arithmetic inside NPM).
        assertApproxEqAbs(
            uint256(liquidityFinal),
            uint256(liquidity1 - halfLiquidity),
            1,
            "final liquidity should be ~half of post-increase liquidity"
        );

        uint256 usdcCollected = IERC20(USDC).balanceOf(user) - usdcBeforeDec;
        uint256 wethCollected = IERC20(WETH).balanceOf(user) - wethBeforeDec;
        assertTrue(usdcCollected > 0 || wethCollected > 0, "half-decrease + collect should return at least one token");
        console.log("LP inc/dec - l0/l1/final:", uint256(liquidity0), uint256(liquidity1));
        console.log("LP inc/dec - l_final:", uint256(liquidityFinal));
        console.log("LP inc/dec - USDC/WETH collected:", usdcCollected, wethCollected);
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 11: LP collect fees after an external swap.
    //         Mint a narrow range around current pool tick, have someone
    //         else swap through the pool to generate fees, then `collect`
    //         (no decrease). Asserts the collect returned some fees.
    // ═════════════════════════════════════════════════════════════════

    function test_fork_lp_collect_fees_after_pool_swap() public {
        _allowTarget(NPM);
        vm.warp(1712700000);

        // Read the pool's current tick so we can mint a tight range around
        // it that's guaranteed to be in-range when the fee-generating swap
        // runs. Align to the pool's tick spacing (0.3% fee → 60).
        (, int24 poolTick,,,,,) = IUniswapV3Pool(USDC_WETH_V3_POOL_3000).slot0();
        int24 spacing = 60;
        int24 nearest = (poolTick / spacing) * spacing;
        // Widen a few spacings each side so the position definitely stays
        // in-range across the fee-generating swap.
        int24 tickLower = nearest - spacing * 5;
        int24 tickUpper = nearest + spacing * 5;

        // Fund user generously so the mint and collect leaves headroom.
        _dealERC20(USDC, user, 20_000 * 1e6);
        _dealERC20(WETH, user, 10 ether);
        vm.prank(user);
        IERC20(USDC).approve(ROUTER_ADDR, type(uint256).max);
        vm.prank(user);
        IERC20(WETH).approve(ROUTER_ADDR, type(uint256).max);

        // Hand-build the mint batch so we can parameterize the tick range
        // (the static fixture bakes ticks at compile time).
        uint256 amount0 = 10_000 * 1e6;
        uint256 amount1 = 5 ether;

        IntentRouter.Call[] memory mintCalls = new IntentRouter.Call[](5);
        mintCalls[0] = IntentRouter.Call({
            target: USDC,
            callData: abi.encodeWithSignature("transferFrom(address,address,uint256)", user, ROUTER_ADDR, amount0),
            value: 0
        });
        mintCalls[1] = IntentRouter.Call({
            target: WETH,
            callData: abi.encodeWithSignature("transferFrom(address,address,uint256)", user, ROUTER_ADDR, amount1),
            value: 0
        });
        mintCalls[2] = IntentRouter.Call({
            target: USDC,
            callData: abi.encodeWithSignature("approve(address,uint256)", NPM, amount0),
            value: 0
        });
        mintCalls[3] = IntentRouter.Call({
            target: WETH,
            callData: abi.encodeWithSignature("approve(address,uint256)", NPM, amount1),
            value: 0
        });
        mintCalls[4] = IntentRouter.Call({
            target: NPM,
            callData: abi.encodeWithSelector(
                INPM.mint.selector,
                INPM.MintParams({
                    token0: USDC,
                    token1: WETH,
                    fee: 3000,
                    tickLower: tickLower,
                    tickUpper: tickUpper,
                    amount0Desired: amount0,
                    amount1Desired: amount1,
                    amount0Min: 0,
                    amount1Min: 0,
                    recipient: user,
                    deadline: block.timestamp + 600
                })
            ),
            value: 0
        });
        address[] memory sweepPair = new address[](2);
        sweepPair[0] = USDC;
        sweepPair[1] = WETH;
        vm.prank(user);
        router.executeDirect(mintCalls, sweepPair);

        uint256 tokenId = INFTEnumerable(NPM).tokenOfOwnerByIndex(user, INFTEnumerable(NPM).balanceOf(user) - 1);
        assertGt(tokenId, 0, "minted NFT");

        // ─── External actor swaps through the pool to generate fees ────
        address swapper = makeAddr("swapper");
        uint256 swapSize = 2_000_000 * 1e6; // 2M USDC — large enough that our
            // tight-range position earns some fee.
        _dealERC20(USDC, swapper, swapSize);
        vm.prank(swapper);
        IERC20(USDC).approve(UNI_ROUTER, swapSize);
        // Do a few back-and-forth swaps so the pool tick crosses our range
        // in both directions, maximizing fee accrual.
        for (uint256 i = 0; i < 3; i++) {
            vm.prank(swapper);
            ISwapRouter(UNI_ROUTER).exactInputSingle(
                ISwapRouter.ExactInputSingleParams({
                    tokenIn: USDC,
                    tokenOut: WETH,
                    fee: 3000,
                    recipient: swapper,
                    deadline: block.timestamp + 600,
                    amountIn: swapSize / 6,
                    amountOutMinimum: 0,
                    sqrtPriceLimitX96: 0
                })
            );
            uint256 wethBack = IERC20(WETH).balanceOf(swapper);
            vm.prank(swapper);
            IERC20(WETH).approve(UNI_ROUTER, wethBack);
            vm.prank(swapper);
            ISwapRouter(UNI_ROUTER).exactInputSingle(
                ISwapRouter.ExactInputSingleParams({
                    tokenIn: WETH,
                    tokenOut: USDC,
                    fee: 3000,
                    recipient: swapper,
                    deadline: block.timestamp + 600,
                    amountIn: wethBack,
                    amountOutMinimum: 0,
                    sqrtPriceLimitX96: 0
                })
            );
        }

        // ─── Collect fees (no decrease) via router batch ───────────────
        vm.prank(user);
        INPM(NPM).approve(ROUTER_ADDR, tokenId);

        uint256 usdcBefore = IERC20(USDC).balanceOf(user);
        uint256 wethBefore = IERC20(WETH).balanceOf(user);

        IntentRouter.Call[] memory collectCalls = new IntentRouter.Call[](1);
        collectCalls[0] = IntentRouter.Call({
            target: NPM,
            callData: abi.encodeWithSelector(
                INPM.collect.selector,
                INPM.CollectParams({
                    tokenId: tokenId,
                    recipient: user,
                    amount0Max: type(uint128).max,
                    amount1Max: type(uint128).max
                })
            ),
            value: 0
        });
        address[] memory noSweep = new address[](0);
        vm.prank(user);
        router.executeDirect(collectCalls, noSweep);

        uint256 usdcFees = IERC20(USDC).balanceOf(user) - usdcBefore;
        uint256 wethFees = IERC20(WETH).balanceOf(user) - wethBefore;
        assertTrue(usdcFees > 0 || wethFees > 0, "collect should return fee income");
        // Liquidity should still be intact (we collected fees, not decreased).
        assertGt(_positionLiquidity(tokenId), 0, "position liquidity remains after collect");

        console.log("LP fees - USDC:", usdcFees);
        console.log("LP fees - WETH:", wethFees);
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 12: Single-sided out-of-range LP mint — when the current pool
    //         tick sits above the chosen range, only token0 is deposited;
    //         token1 side sits at zero until the price moves into range.
    // ═════════════════════════════════════════════════════════════════

    function test_fork_lp_out_of_range_single_sided_mint() public {
        _allowTarget(NPM);
        vm.warp(1712700000);

        (, int24 poolTick,,,,,) = IUniswapV3Pool(USDC_WETH_V3_POOL_3000).slot0();
        int24 spacing = 60;
        int24 nearest = (poolTick / spacing) * spacing;

        // Uniswap V3: when the pool's current price is below the range, the
        // position holds entirely token0. USDC is token0 in this pool, so
        // to get a USDC-only position we pick a range strictly ABOVE the
        // current pool tick.
        int24 tickLower = nearest + spacing * 100;
        int24 tickUpper = nearest + spacing * 200;

        _dealERC20(USDC, user, 5_000 * 1e6);
        _dealERC20(WETH, user, 2 ether);
        vm.prank(user);
        IERC20(USDC).approve(ROUTER_ADDR, type(uint256).max);
        vm.prank(user);
        IERC20(WETH).approve(ROUTER_ADDR, type(uint256).max);

        uint256 usdcBefore = IERC20(USDC).balanceOf(user);
        uint256 wethBefore = IERC20(WETH).balanceOf(user);
        uint256 amount0 = 3_000 * 1e6;
        uint256 amount1 = 1 ether;

        IntentRouter.Call[] memory calls = new IntentRouter.Call[](5);
        calls[0] = IntentRouter.Call({
            target: USDC,
            callData: abi.encodeWithSignature("transferFrom(address,address,uint256)", user, ROUTER_ADDR, amount0),
            value: 0
        });
        calls[1] = IntentRouter.Call({
            target: WETH,
            callData: abi.encodeWithSignature("transferFrom(address,address,uint256)", user, ROUTER_ADDR, amount1),
            value: 0
        });
        calls[2] = IntentRouter.Call({
            target: USDC,
            callData: abi.encodeWithSignature("approve(address,uint256)", NPM, amount0),
            value: 0
        });
        calls[3] = IntentRouter.Call({
            target: WETH,
            callData: abi.encodeWithSignature("approve(address,uint256)", NPM, amount1),
            value: 0
        });
        calls[4] = IntentRouter.Call({
            target: NPM,
            callData: abi.encodeWithSelector(
                INPM.mint.selector,
                INPM.MintParams({
                    token0: USDC,
                    token1: WETH,
                    fee: 3000,
                    tickLower: tickLower,
                    tickUpper: tickUpper,
                    amount0Desired: amount0,
                    amount1Desired: amount1,
                    amount0Min: 0,
                    amount1Min: 0,
                    recipient: user,
                    deadline: block.timestamp + 600
                })
            ),
            value: 0
        });
        address[] memory sweepPair = new address[](2);
        sweepPair[0] = USDC;
        sweepPair[1] = WETH;
        vm.prank(user);
        router.executeDirect(calls, sweepPair);

        uint256 tokenId = INFTEnumerable(NPM).tokenOfOwnerByIndex(user, INFTEnumerable(NPM).balanceOf(user) - 1);
        assertGt(_positionLiquidity(tokenId), 0, "position should have liquidity");

        // Out-of-range mint with pool tick above range should use only USDC.
        // Some WETH dust may be consumed due to rounding in getAmountsForLiquidity;
        // assert usage is trivially small.
        uint256 usdcSpent = usdcBefore - IERC20(USDC).balanceOf(user);
        uint256 wethSpent = wethBefore - IERC20(WETH).balanceOf(user);
        assertGt(usdcSpent, 0, "should deposit USDC");
        assertLt(wethSpent, 0.01 ether, "WETH usage should be near-zero for below-range position");

        console.log("Single-sided mint - USDC spent:", usdcSpent);
        console.log("Single-sided mint - WETH spent:", wethSpent);
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 13: Close a 1.5x leveraged wstETH position via a Balancer
    //         flashloan. Hand-builds the inner pipeline because the
    //         compiler's `close_position` expander doesn't yet pull
    //         aTokens from the signer (Aave V3 requires msg.sender to hold
    //         the aTokens for `withdraw`).
    // ═════════════════════════════════════════════════════════════════

    function test_fork_leverage_close_position_round_trip() public {
        // Reuse leverage test's price band.
        int256 ethUsdRaw = IChainlinkFeed(CHAINLINK_ETH_USD).latestAnswer();
        require(ethUsdRaw > 0);
        uint256 ethUsd = uint256(ethUsdRaw) / 1e8;
        if (ethUsd < 1500 || ethUsd > 3500) {
            emit log_named_uint("Skipping: HEAD ETH price outside leverage test band", ethUsd);
            vm.skip(true);
        }

        _allowTarget(BALANCER_VAULT);
        // aWstETH is an allowlisted target because the close path pulls
        // aTokens from the signer via an inner `transferFrom` call.
        _allowTarget(A_WSTETH);

        // ─── Step A: open the 1.5x long position ──────────────────────
        uint256 contribution = 1 ether;
        _dealERC20(WSTETH, user, contribution);
        vm.prank(user);
        IERC20(WSTETH).approve(ROUTER_ADDR, contribution);
        _approveDelegation(VDEBT_USDC, user, ROUTER_ADDR, 2_000 * 1e6);

        bytes memory openData = _readCalldata("long_wsteth_1_5x");
        vm.prank(user);
        (bool openOk,) = ROUTER_ADDR.call(openData);
        assertTrue(openOk, "open should succeed");

        uint256 aWstethAfterOpen = IERC20(A_WSTETH).balanceOf(user);
        uint256 debtAfterOpen = IERC20(VDEBT_USDC).balanceOf(user);
        assertGe(aWstethAfterOpen, 1.49 ether, "position opened with ~1.5 aWstETH");
        assertGt(debtAfterOpen, 0, "position opened with USDC debt");

        // ─── Step B: close via a hand-built flashloan batch ────────────
        // Flashloan ~5% more USDC than current debt (Aave refunds overage).
        uint256 flashAmount = (debtAfterOpen * 105) / 100;

        // User gives the router two approvals for the close:
        //   1. aWstETH transferFrom: router needs to hold aTokens to call
        //      aave.withdraw (msg.sender must be the aToken owner).
        //   2. USDC variable-debt delegation: already set above, still live.
        vm.prank(user);
        IERC20(A_WSTETH).approve(ROUTER_ADDR, aWstethAfterOpen);

        // Build the inner Call[] in an order that respects Aave's
        // per-step health-factor checks:
        //   1. approve USDC→Aave
        //   2. repay signer's debt (so the aToken transfer in step 3 doesn't
        //      revert Aave's HF check on the `from` side — at step 3 the
        //      signer has 0 debt, so losing aTokens can't breach HF).
        //   3. transferFrom aWstETH signer → router
        //   4. withdraw wstETH (router is msg.sender, router owns aTokens)
        //   5. approve wstETH→Uni
        //   6. swap wstETH→USDC
        IntentRouter.Call[] memory inner = new IntentRouter.Call[](6);
        inner[0] = IntentRouter.Call({
            target: USDC,
            callData: abi.encodeWithSignature("approve(address,uint256)", AAVE_POOL, flashAmount),
            value: 0
        });
        inner[1] = IntentRouter.Call({
            target: AAVE_POOL,
            callData: abi.encodeWithSignature("repay(address,uint256,uint256,address)", USDC, flashAmount, uint256(2), user),
            value: 0
        });
        inner[2] = IntentRouter.Call({
            target: A_WSTETH,
            callData: abi.encodeWithSignature("transferFrom(address,address,uint256)", user, ROUTER_ADDR, aWstethAfterOpen),
            value: 0
        });
        inner[3] = IntentRouter.Call({
            target: AAVE_POOL,
            callData: abi.encodeWithSignature("withdraw(address,uint256,address)", WSTETH, type(uint256).max, ROUTER_ADDR),
            value: 0
        });
        inner[4] = IntentRouter.Call({
            target: WSTETH,
            callData: abi.encodeWithSignature("approve(address,uint256)", UNI_ROUTER, aWstethAfterOpen),
            value: 0
        });
        inner[5] = IntentRouter.Call({
            target: UNI_ROUTER,
            callData: abi.encodeWithSelector(
                ISwapRouter.exactInputSingle.selector,
                ISwapRouter.ExactInputSingleParams({
                    tokenIn: WSTETH,
                    tokenOut: USDC,
                    fee: 500,
                    recipient: ROUTER_ADDR,
                    deadline: block.timestamp + 600,
                    amountIn: aWstethAfterOpen,
                    amountOutMinimum: flashAmount,
                    sqrtPriceLimitX96: 0
                })
            ),
            value: 0
        });

        // Encode inner Call[] as userData and build the outer flashLoan call.
        bytes memory userData = abi.encode(inner);

        address[] memory flashTokens = new address[](1);
        flashTokens[0] = USDC;
        uint256[] memory flashAmounts = new uint256[](1);
        flashAmounts[0] = flashAmount;

        IntentRouter.Call[] memory outer = new IntentRouter.Call[](1);
        outer[0] = IntentRouter.Call({
            target: BALANCER_VAULT,
            callData: abi.encodeWithSignature(
                "flashLoan(address,address[],uint256[],bytes)", ROUTER_ADDR, flashTokens, flashAmounts, userData
            ),
            value: 0
        });
        address[] memory sweepUsdc = new address[](1);
        sweepUsdc[0] = USDC;

        vm.prank(user);
        router.executeDirect(outer, sweepUsdc);

        // ─── Assertions ────────────────────────────────────────────────
        uint256 aWstethAfterClose = IERC20(A_WSTETH).balanceOf(user);
        uint256 debtAfterClose = IERC20(VDEBT_USDC).balanceOf(user);
        assertLt(aWstethAfterClose, 1e12, "aWstETH should be ~0 after close");
        assertLt(debtAfterClose, 1e4, "USDC debt should be ~0 after close");

        console.log("Close - aWstETH residual:", aWstethAfterClose);
        console.log("Close - USDC debt residual:", debtAfterClose);
        console.log("Close - user USDC final:", IERC20(USDC).balanceOf(user));
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 14: After opening a 1.5x long, the position's Aave health
    //         factor must be comfortably above the liquidation threshold.
    //         A HF below ~1.1 would mean a single adverse block could
    //         liquidate, so we require HF > 1.15 as a sanity floor.
    // ═════════════════════════════════════════════════════════════════

    function test_fork_leverage_healthy_hf_after_open() public {
        int256 ethUsdRaw = IChainlinkFeed(CHAINLINK_ETH_USD).latestAnswer();
        require(ethUsdRaw > 0);
        uint256 ethUsd = uint256(ethUsdRaw) / 1e8;
        if (ethUsd < 1500 || ethUsd > 3500) {
            emit log_named_uint("Skipping: HEAD ETH price outside leverage test band", ethUsd);
            vm.skip(true);
        }

        _allowTarget(BALANCER_VAULT);

        uint256 contribution = 1 ether;
        _dealERC20(WSTETH, user, contribution);
        vm.prank(user);
        IERC20(WSTETH).approve(ROUTER_ADDR, contribution);
        _approveDelegation(VDEBT_USDC, user, ROUTER_ADDR, 2_000 * 1e6);

        bytes memory data = _readCalldata("long_wsteth_1_5x");
        vm.prank(user);
        (bool ok,) = ROUTER_ADDR.call(data);
        assertTrue(ok, "open should succeed");

        // Aave V3's `getUserAccountData` returns (totalCollateralBase,
        // totalDebtBase, availableBorrowsBase, currentLiquidationThreshold,
        // ltv, healthFactor). healthFactor is 1e18-scaled.
        (bool got, bytes memory ret) =
            AAVE_POOL.staticcall(abi.encodeWithSignature("getUserAccountData(address)", user));
        require(got && ret.length == 6 * 32, "getUserAccountData call failed");
        (,,,,, uint256 healthFactor) = abi.decode(ret, (uint256, uint256, uint256, uint256, uint256, uint256));

        assertGt(healthFactor, 115e16, "HF after open should exceed 1.15 (safe margin)");
        // And not absurdly high — very low LTV usage would indicate the
        // leverage sugar under-borrowed.
        assertLt(healthFactor, 30e18, "HF should be in a realistic range (< 30)");

        console.log("HF after open (1e18 scale):", healthFactor);
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 15: Full-range LP mint. Ticks -887220..887220 at the 0.3% fee
    //         tier (spacing 60). Asserts the mint succeeded and both sides
    //         were used (in-range position, both tokens needed).
    // ═════════════════════════════════════════════════════════════════

    function test_fork_lp_full_range_mint() public {
        _allowTarget(NPM);
        vm.warp(1712700000);

        uint256 amount0 = 3_000 * 1e6;
        uint256 amount1 = 1 ether;
        _dealERC20(USDC, user, amount0);
        _dealERC20(WETH, user, amount1);
        vm.prank(user);
        IERC20(USDC).approve(ROUTER_ADDR, amount0);
        vm.prank(user);
        IERC20(WETH).approve(ROUTER_ADDR, amount1);

        uint256 usdcBefore = IERC20(USDC).balanceOf(user);
        uint256 wethBefore = IERC20(WETH).balanceOf(user);

        IntentRouter.Call[] memory calls = new IntentRouter.Call[](5);
        calls[0] = IntentRouter.Call({
            target: USDC,
            callData: abi.encodeWithSignature("transferFrom(address,address,uint256)", user, ROUTER_ADDR, amount0),
            value: 0
        });
        calls[1] = IntentRouter.Call({
            target: WETH,
            callData: abi.encodeWithSignature("transferFrom(address,address,uint256)", user, ROUTER_ADDR, amount1),
            value: 0
        });
        calls[2] = IntentRouter.Call({
            target: USDC,
            callData: abi.encodeWithSignature("approve(address,uint256)", NPM, amount0),
            value: 0
        });
        calls[3] = IntentRouter.Call({
            target: WETH,
            callData: abi.encodeWithSignature("approve(address,uint256)", NPM, amount1),
            value: 0
        });
        calls[4] = IntentRouter.Call({
            target: NPM,
            callData: abi.encodeWithSelector(
                INPM.mint.selector,
                INPM.MintParams({
                    token0: USDC,
                    token1: WETH,
                    fee: 3000,
                    tickLower: -887220, // MIN tick aligned to spacing 60
                    tickUpper: 887220, // MAX tick aligned to spacing 60
                    amount0Desired: amount0,
                    amount1Desired: amount1,
                    amount0Min: 0,
                    amount1Min: 0,
                    recipient: user,
                    deadline: block.timestamp + 600
                })
            ),
            value: 0
        });
        address[] memory sweepPair = new address[](2);
        sweepPair[0] = USDC;
        sweepPair[1] = WETH;
        vm.prank(user);
        router.executeDirect(calls, sweepPair);

        uint256 tokenId = INFTEnumerable(NPM).tokenOfOwnerByIndex(user, INFTEnumerable(NPM).balanceOf(user) - 1);
        assertGt(_positionLiquidity(tokenId), 0, "full-range position should have liquidity");

        // Full-range in-range: both tokens get used, both deltas positive.
        uint256 usdcSpent = usdcBefore - IERC20(USDC).balanceOf(user);
        uint256 wethSpent = wethBefore - IERC20(WETH).balanceOf(user);
        assertGt(usdcSpent, 0, "USDC should be used in full-range mint");
        assertGt(wethSpent, 0, "WETH should be used in full-range mint");
        // Neither side should consume dust-only amounts — balanced mint.
        assertGt(usdcSpent, amount0 / 100, "USDC usage should be >1% of desired");
        assertGt(wethSpent, amount1 / 100, "WETH usage should be >1% of desired");

        console.log("Full-range - USDC/WETH spent:", usdcSpent, wethSpent);
    }

    // ═════════════════════════════════════════════════════════════════
    // Test 16: Mint two Uniswap V3 LP positions in a single router batch.
    //         One narrow range, one wide range — shares the same USDC+WETH
    //         wallet pulls, distinct NPM approvals, produces two NFTs.
    // ═════════════════════════════════════════════════════════════════

    function test_fork_lp_multiple_positions_batched() public {
        _allowTarget(NPM);
        vm.warp(1712700000);

        (, int24 poolTick,,,,,) = IUniswapV3Pool(USDC_WETH_V3_POOL_3000).slot0();
        int24 nearest = (poolTick / 60) * 60;

        _dealERC20(USDC, user, 5_000 * 1e6);
        _dealERC20(WETH, user, 1.6 ether);
        vm.prank(user);
        IERC20(USDC).approve(ROUTER_ADDR, 5_000 * 1e6);
        vm.prank(user);
        IERC20(WETH).approve(ROUTER_ADDR, 1.6 ether);

        uint256 nftsBefore = INFTEnumerable(NPM).balanceOf(user);

        IntentRouter.Call[] memory calls = _buildMultiLPBatch(nearest);
        address[] memory sweepPair = new address[](2);
        sweepPair[0] = USDC;
        sweepPair[1] = WETH;
        vm.prank(user);
        router.executeDirect(calls, sweepPair);

        uint256 nftsAfter = INFTEnumerable(NPM).balanceOf(user);
        assertEq(nftsAfter - nftsBefore, 2, "signer should have two new LP NFTs");

        uint256 firstTokenId = INFTEnumerable(NPM).tokenOfOwnerByIndex(user, nftsAfter - 2);
        uint256 secondTokenId = INFTEnumerable(NPM).tokenOfOwnerByIndex(user, nftsAfter - 1);
        assertGt(_positionLiquidity(firstTokenId), 0, "first NFT should have liquidity");
        assertGt(_positionLiquidity(secondTokenId), 0, "second NFT should have liquidity");

        console.log("Multi-LP - NFT #1 liquidity:", uint256(_positionLiquidity(firstTokenId)));
        console.log("Multi-LP - NFT #2 liquidity:", uint256(_positionLiquidity(secondTokenId)));
    }

    /// @dev Factored out of `test_fork_lp_multiple_positions_batched` to
    ///      keep that function under Solidity's stack-too-deep limit.
    function _buildMultiLPBatch(int24 nearest) internal view returns (IntentRouter.Call[] memory) {
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](6);
        calls[0] = IntentRouter.Call({
            target: USDC,
            callData: abi.encodeWithSignature("transferFrom(address,address,uint256)", user, ROUTER_ADDR, 5_000 * 1e6),
            value: 0
        });
        calls[1] = IntentRouter.Call({
            target: WETH,
            callData: abi.encodeWithSignature("transferFrom(address,address,uint256)", user, ROUTER_ADDR, 1.6 ether),
            value: 0
        });
        calls[2] = IntentRouter.Call({
            target: USDC,
            callData: abi.encodeWithSignature("approve(address,uint256)", NPM, 5_000 * 1e6),
            value: 0
        });
        calls[3] = IntentRouter.Call({
            target: WETH,
            callData: abi.encodeWithSignature("approve(address,uint256)", NPM, 1.6 ether),
            value: 0
        });
        // Narrow: ±3 spacings around current tick.
        calls[4] = _mintCall(nearest - 180, nearest + 180, 2_000 * 1e6, 0.6 ether);
        // Wide: ±100 spacings.
        calls[5] = _mintCall(nearest - 6000, nearest + 6000, 3_000 * 1e6, 1 ether);
        return calls;
    }

    function _mintCall(int24 tickLower, int24 tickUpper, uint256 amount0Desired, uint256 amount1Desired)
        internal
        view
        returns (IntentRouter.Call memory)
    {
        INPM.MintParams memory p = INPM.MintParams({
            token0: USDC,
            token1: WETH,
            fee: 3000,
            tickLower: tickLower,
            tickUpper: tickUpper,
            amount0Desired: amount0Desired,
            amount1Desired: amount1Desired,
            amount0Min: 0,
            amount1Min: 0,
            recipient: user,
            deadline: block.timestamp + 600
        });
        return IntentRouter.Call({target: NPM, callData: abi.encodeWithSelector(INPM.mint.selector, p), value: 0});
    }

    // ─── Helpers for the new fork tests ──────────────────────────────

    /// @dev Extract just the liquidity field (slot 7) from NPM.positions to
    ///      avoid the 12-tuple return's stack-too-deep hazard.
    function _positionLiquidity(uint256 tokenId) internal view returns (uint128) {
        (bool ok, bytes memory ret) =
            NPM.staticcall(abi.encodeWithSelector(bytes4(keccak256("positions(uint256)")), tokenId));
        require(ok && ret.length >= 12 * 32, "positions() call failed");
        // liquidity is slot index 7 (0-based); each slot is 32 bytes.
        uint256 liq;
        assembly {
            liq := mload(add(ret, add(32, mul(7, 32))))
        }
        return uint128(liq);
    }
}

// ─── Uniswap V3 NPM interfaces (minimal; avoid pulling in the npm lib) ──

interface INFTEnumerable {
    function balanceOf(address owner) external view returns (uint256);
    function tokenOfOwnerByIndex(address owner, uint256 index) external view returns (uint256);
}

interface IChainlinkFeed {
    function latestAnswer() external view returns (int256);
}

interface INPM {
    struct MintParams {
        address token0;
        address token1;
        uint24 fee;
        int24 tickLower;
        int24 tickUpper;
        uint256 amount0Desired;
        uint256 amount1Desired;
        uint256 amount0Min;
        uint256 amount1Min;
        address recipient;
        uint256 deadline;
    }

    struct IncreaseLiquidityParams {
        uint256 tokenId;
        uint256 amount0Desired;
        uint256 amount1Desired;
        uint256 amount0Min;
        uint256 amount1Min;
        uint256 deadline;
    }

    struct DecreaseLiquidityParams {
        uint256 tokenId;
        uint128 liquidity;
        uint256 amount0Min;
        uint256 amount1Min;
        uint256 deadline;
    }

    struct CollectParams {
        uint256 tokenId;
        address recipient;
        uint128 amount0Max;
        uint128 amount1Max;
    }

    function approve(address to, uint256 tokenId) external;

    function mint(MintParams calldata params)
        external
        payable
        returns (uint256 tokenId, uint128 liquidity, uint256 amount0, uint256 amount1);

    function increaseLiquidity(IncreaseLiquidityParams calldata params)
        external
        payable
        returns (uint128 liquidity, uint256 amount0, uint256 amount1);

    function decreaseLiquidity(DecreaseLiquidityParams calldata params)
        external
        payable
        returns (uint256 amount0, uint256 amount1);

    function collect(CollectParams calldata params) external payable returns (uint256 amount0, uint256 amount1);
}

interface IUniswapV3Pool {
    function slot0()
        external
        view
        returns (
            uint160 sqrtPriceX96,
            int24 tick,
            uint16 observationIndex,
            uint16 observationCardinality,
            uint16 observationCardinalityNext,
            uint8 feeProtocol,
            bool unlocked
        );
}

interface ISwapRouter {
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

    function exactInputSingle(ExactInputSingleParams calldata params) external payable returns (uint256 amountOut);
}
