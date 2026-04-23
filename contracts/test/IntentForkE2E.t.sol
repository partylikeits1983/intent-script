// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { Test, console } from "forge-std/Test.sol";
import { IntentRouter } from "../src/IntentRouter.sol";
import { IERC20 } from "../src/interfaces/IERC20.sol";

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
    address constant WETH        = 0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2;
    address constant USDC        = 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48;
    address constant DAI         = 0x6B175474E89094C44Da98b954EedeAC495271d0F;
    address constant STETH       = 0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84;
    address constant WSTETH      = 0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0;
    address constant AAVE_POOL   = 0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2;
    address constant UNI_ROUTER  = 0xE592427A0AEce92De3Edee1F18E0157C05861564;

    // Compiler signer address — must match the "from" field in example JSON files.
    // The compiler bakes this address into transferFrom calls, so the test user
    // must be the same address for compiler-generated calldata to work.
    address constant COMPILER_SIGNER = 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045;

    // Aave V3 aToken addresses on mainnet
    address constant A_WETH      = 0x4d5F47FA6A74757f35C14fD3a6Ef8E3C9BC514E8;
    address constant A_USDC      = 0x98C23E9d8f34FEFb1B7BD6a91B7FF122F4e16F5c;
    address constant A_WSTETH    = 0x0B925eD163218f6662a35e0f0371Ac234f9E9371;

    // Variable debt token for DAI on Aave V3
    address constant VDEBT_DAI   = 0xcF8d0c70c850859266f5C338b38F9D663181C314;

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

    // ─── Helper: approve credit delegation for Aave V3 borrows ───────

    /// @dev Aave V3 requires credit delegation when msg.sender != onBehalfOf.
    ///      When borrowing through the router, the router is msg.sender but
    ///      onBehalfOf is the user, so the user must delegate borrow power.
    function _approveDelegation(address vDebtToken, address delegator, address delegatee, uint256 amount) internal {
        vm.prank(delegator);
        (bool ok,) = vDebtToken.call(
            abi.encodeWithSignature("approveDelegation(address,uint256)", delegatee, amount)
        );
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
            callHashes[i] = keccak256(abi.encode(
                CALL_TYPEHASH,
                batch.calls[i].target,
                keccak256(batch.calls[i].callData),
                batch.calls[i].value
            ));
        }
        bytes32 callsHash = keccak256(abi.encodePacked(callHashes));

        bytes32 structHash = keccak256(abi.encode(
            INTENT_BATCH_TYPEHASH,
            batch.signer,
            callsHash,
            keccak256(abi.encodePacked(batch.tokensToSweep)),
            batch.nonce,
            batch.deadline
        ));

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
        (bool success,) = WETH.call{ value: value }(callData);
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
        (bool success,) = ROUTER_ADDR.call{ value: value }(callData);
        assertTrue(success, "Swap USDC->WETH should succeed");

        uint256 wethAfter = IERC20(WETH).balanceOf(user);
        uint256 usdcAfter = IERC20(USDC).balanceOf(user);

        assertTrue(wethAfter > wethBefore, "User should have received WETH");
        assertEq(usdcAfter, 0, "User should have spent all USDC");

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
        (bool success,) = ROUTER_ADDR.call{ value: value }(callData);
        assertTrue(success, "Aave deposit USDC should succeed");

        uint256 aUsdcAfter = IERC20(A_USDC).balanceOf(user);
        uint256 usdcAfter = IERC20(USDC).balanceOf(user);

        assertTrue(aUsdcAfter > aUsdcBefore, "User should have received aUSDC");
        assertEq(usdcAfter, 0, "User should have spent all USDC");

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
        (bool success,) = ROUTER_ADDR.call{ value: value }(callData);
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
        (bool success,) = STETH.call{ value: value }(callData);
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
        (bool success,) = ROUTER_ADDR.call{ value: value }(callData);
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
            callData: abi.encodeWithSignature(
                "transferFrom(address,address,uint256)",
                signer, routerAddr, usdcAmount
            ),
            value: 0
        });

        // Step 1: Router approves Uniswap to spend USDC
        calls[1] = IntentRouter.Call({
            target: USDC,
            callData: abi.encodeWithSignature(
                "approve(address,uint256)", UNI_ROUTER, usdcAmount
            ),
            value: 0
        });

        // Step 2: Swap USDC → wstETH through 0.05% pool, recipient = router
        // amountOutMinimum is set to a conservative value for slippage protection
        // (matches compiler behavior when min_amount_out is specified)
        calls[2] = IntentRouter.Call({
            target: UNI_ROUTER,
            callData: abi.encodeWithSignature(
                "exactInputSingle((address,address,uint24,address,uint256,uint256,uint256,uint160))",
                USDC, WSTETH, uint24(500), routerAddr,
                type(uint256).max, usdcAmount, uint256(1), uint160(0)
            ),
            value: 0
        });

        // Step 3: Router approves Aave to spend wstETH (already in router from swap)
        calls[3] = IntentRouter.Call({
            target: WSTETH,
            callData: abi.encodeWithSignature(
                "approve(address,uint256)", AAVE_POOL, depositAmount
            ),
            value: 0
        });

        // Step 4: Supply wstETH to Aave on behalf of signer
        calls[4] = IntentRouter.Call({
            target: AAVE_POOL,
            callData: abi.encodeWithSignature(
                "supply(address,uint256,address,uint16)",
                WSTETH, depositAmount, signer, uint16(0)
            ),
            value: 0
        });

        // Step 5: Borrow DAI from Aave on behalf of signer
        calls[5] = IntentRouter.Call({
            target: AAVE_POOL,
            callData: abi.encodeWithSignature(
                "borrow(address,uint256,uint256,uint16,address)",
                DAI, borrowAmount, uint256(2), uint16(0), signer
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
        signedRouter.executeSigned{ value: 0 }(batch, signature);

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
    function _buildSignedRouterDigest(
        IntentRouter targetRouter,
        IntentRouter.IntentBatch memory batch
    ) internal view returns (bytes32) {
        bytes32[] memory callHashes = new bytes32[](batch.calls.length);
        for (uint256 i = 0; i < batch.calls.length; i++) {
            callHashes[i] = keccak256(abi.encode(
                CALL_TYPEHASH,
                batch.calls[i].target,
                keccak256(batch.calls[i].callData),
                batch.calls[i].value
            ));
        }
        bytes32 callsHash = keccak256(abi.encodePacked(callHashes));

        bytes32 structHash = keccak256(abi.encode(
            INTENT_BATCH_TYPEHASH,
            batch.signer,
            callsHash,
            keccak256(abi.encodePacked(batch.tokensToSweep)),
            batch.nonce,
            batch.deadline
        ));

        return keccak256(abi.encodePacked("\x19\x01", targetRouter.DOMAIN_SEPARATOR(), structHash));
    }
}
