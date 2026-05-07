// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { Test, console } from "forge-std/Test.sol";
import { IntentRouter } from "../src/IntentRouter.sol";
import { IERC20 } from "../src/interfaces/IERC20.sol";

/// @title IntentForkScenariosE2E
/// @notice User-flow integration tests for the JSON DSL → compiler → router
///         pipeline.  Each test:
///
///           DSL JSON  →  `cargo test --test generate_calldata`  →  .txt fixture
///                                                                 ↓
///                                                       _readCalldata + executeDirect
///                                                                 ↓
///                                                     mainnet-fork IntentRouter
///
/// Add new tests here as more user scenarios are designed.
///
/// Self-contained (does not extend IntentForkBase) because that base file's
/// ROUTER_ADDR and setUp() require are stale relative to the current
/// `config/protocols/ethereum.json`. The constants and helpers below match
/// the freshly-regenerated fixtures.
///
/// Run with:
///   make generate-calldata
///   forge test --mc IntentForkScenariosE2E --fork-url $ETH_RPC_URL -vvv
contract IntentForkScenariosE2E is Test {
    // ─── Mainnet addresses ───────────────────────────────────────────────
    // ROUTER_ADDR must match `intent_router.router` in
    // config/protocols/ethereum.json AND the address baked into the
    // .txt fixtures (see `routerAddress` in *_batch.json).
    address constant ROUTER_ADDR = 0x1F3249a661012a1DFa0b085F5716851c45023548;

    address constant WETH = 0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2;
    address constant USDC = 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48;
    address constant USDT = 0xdAC17F958D2ee523a2206206994597C13D831ec7;
    address constant WSTETH = 0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0;
    address constant STETH = 0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84;

    address constant AAVE_POOL = 0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2;
    address constant UNI_ROUTER = 0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45;
    address constant BALANCER_VAULT = 0xBA12222222228d8Ba445958a75a0704d566BF2C8;
    address constant NPM = 0xC36442b4a4522E871399CD717aBDD847Ab11FE88; // Uniswap V3 NonfungiblePositionManager

    // Aave V3 mainnet aTokens / variable-debt tokens
    address constant A_USDC = 0x98C23E9d8f34FEFb1B7BD6a91B7FF122F4e16F5c;
    address constant A_USDT = 0x23878914EFE38d27C4D67Ab83ed1b93A74D4086a;
    address constant A_WSTETH = 0x0B925eD163218f6662a35e0f0371Ac234f9E9371;
    address constant VDEBT_USDC = 0x72E95b8931767C79bA4EeE721354d6E99a61D004;
    address constant VDEBT_USDT = 0x6df1C1E379bC5a00a7b4C6e67A203333772f45A8;
    address constant VDEBT_WETH = 0xeA51d7853EEFb32b6ee06b1C12E6dcCA88Be0fFE;

    // Compiler signer — every example JSON uses this as `from`, so the
    // calldata's transferFrom calls pull from this address.
    address constant COMPILER_SIGNER = 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045;

    IntentRouter public router;
    address public user;

    function setUp() public {
        IntentRouter impl_ = new IntentRouter(BALANCER_VAULT, AAVE_POOL);
        vm.etch(ROUTER_ADDR, address(impl_).code);
        router = IntentRouter(payable(ROUTER_ADDR));

        // ReentrancyGuard._status defaults to 1 in the constructor; vm.etch
        // copies code but not storage, so re-init slot 0 = _NOT_ENTERED.
        vm.store(ROUTER_ADDR, bytes32(uint256(0)), bytes32(uint256(1)));

        // allowedTargets lives at slot 3 (slot 0 = _status, 1 = nonces,
        // 2 = owner, 3 = allowlist).
        _allowTarget(WETH);
        _allowTarget(USDC);
        _allowTarget(USDT);
        _allowTarget(WSTETH);
        _allowTarget(STETH);
        _allowTarget(AAVE_POOL);
        _allowTarget(UNI_ROUTER);
        _allowTarget(BALANCER_VAULT);
        _allowTarget(NPM);

        user = COMPILER_SIGNER;
        vm.deal(user, 1000 ether);
    }

    // ═════════════════════════════════════════════════════════════════════
    // #1 Aave: deposit 10k USDT, borrow 1 WETH
    // ═════════════════════════════════════════════════════════════════════

    function test_fork_aaveDepositUSDT_borrowWETH() public {
        uint256 usdtAmount = 10_000 * 1e6;
        uint256 borrowAmount = 1 ether;

        _dealERC20(USDT, user, usdtAmount);
        _approveUSDTBypass(user, ROUTER_ADDR, usdtAmount);
        _approveDelegation(VDEBT_WETH, user, ROUTER_ADDR, borrowAmount);

        uint256 wethBefore = IERC20(WETH).balanceOf(user);

        bytes memory callData = _readCalldata("aave_deposit_usdt_borrow_weth");
        uint256 value = _readValue("aave_deposit_usdt_borrow_weth");

        vm.prank(user);
        (bool success,) = ROUTER_ADDR.call{ value: value }(callData);
        assertTrue(success, "Aave USDT deposit + WETH borrow should succeed");

        uint256 aUsdtBal = IERC20(A_USDT).balanceOf(user);
        uint256 wethAfter = IERC20(WETH).balanceOf(user);
        uint256 usdtAfter = IERC20(USDT).balanceOf(user);
        uint256 debt = IERC20(VDEBT_WETH).balanceOf(user);

        assertGt(aUsdtBal, 0, "user should hold aUSDT after deposit");
        assertApproxEqAbs(
            wethAfter - wethBefore, borrowAmount, 10, "WETH gained should equal borrow"
        );
        assertEq(usdtAfter, 0, "all USDT should be spent");
        // Aave accrues interest at ~1 wei per block on a fresh borrow, so the
        // debt token balance can be up to a few wei above the principal.
        assertApproxEqAbs(debt, borrowAmount, 100, "WETH variable debt ~= borrow");

        address[] memory cleared = new address[](2);
        cleared[0] = USDT;
        cleared[1] = WETH;
        _assertRouterCleared(cleared);

        console.log("Test 1: aUSDT", aUsdtBal, "WETH gained", wethAfter - wethBefore);
    }

    // ═════════════════════════════════════════════════════════════════════
    // #2 Aave: deposit 5 wstETH, borrow 1k USDC
    //
    // wstETH instead of WETH because Aave V3 set WETH reserve LTV to 0 on
    // mainnet post-2024 — borrowing against WETH reverts with
    // LtvValidationFailed. wstETH still has LTV ≈ 78.5%.
    // ═════════════════════════════════════════════════════════════════════

    function test_fork_aaveDepositWSTETH_borrowUSDC() public {
        uint256 wstethAmount = 5 ether;
        uint256 borrowAmount = 1_000 * 1e6;

        _dealERC20(WSTETH, user, wstethAmount);
        vm.prank(user);
        IERC20(WSTETH).approve(ROUTER_ADDR, wstethAmount);
        _approveDelegation(VDEBT_USDC, user, ROUTER_ADDR, borrowAmount);

        uint256 usdcBefore = IERC20(USDC).balanceOf(user);

        bytes memory callData = _readCalldata("aave_deposit_wsteth_borrow_usdc");
        uint256 value = _readValue("aave_deposit_wsteth_borrow_usdc");

        vm.prank(user);
        (bool success,) = ROUTER_ADDR.call{ value: value }(callData);
        assertTrue(success, "Aave wstETH deposit + USDC borrow should succeed");

        uint256 aWstethBal = IERC20(A_WSTETH).balanceOf(user);
        uint256 usdcAfter = IERC20(USDC).balanceOf(user);
        uint256 wstethAfter = IERC20(WSTETH).balanceOf(user);
        uint256 debt = IERC20(VDEBT_USDC).balanceOf(user);

        assertGt(aWstethBal, 0, "user should hold aWSTETH after deposit");
        assertEq(wstethAfter, 0, "all wstETH should be deposited");
        assertApproxEqAbs(
            usdcAfter - usdcBefore, borrowAmount, 10, "USDC gained should equal borrow"
        );
        assertApproxEqAbs(debt, borrowAmount, 100, "USDC variable debt ~= borrow");

        address[] memory cleared = new address[](2);
        cleared[0] = WSTETH;
        cleared[1] = USDC;
        _assertRouterCleared(cleared);

        console.log("Test 2: aWSTETH", aWstethBal, "USDC gained", usdcAfter - usdcBefore);
    }

    // ═════════════════════════════════════════════════════════════════════
    // #2b Aave: deposit 10k USDC, borrow 2k USDT
    //
    // This is the exact intent shape that revealed the missing-credit-
    // delegation bug from the UI: USDC collateral, USDT debt, batched
    // through the IntentRouter as `executeDirect`. Without the
    // approveDelegation prereq the borrow reverts with custom error
    // 0x1cb19ef3 (InsufficientBorrowAllowance). Locking it in here so any
    // regression on the prereq-emission path is caught the next time we
    // touch the compiler.
    // ═════════════════════════════════════════════════════════════════════

    function test_fork_aaveDepositUSDC_borrowUSDT() public {
        uint256 usdcAmount = 10_000 * 1e6;
        uint256 borrowAmount = 2_000 * 1e6;

        _dealERC20(USDC, user, usdcAmount);
        vm.prank(user);
        IERC20(USDC).approve(ROUTER_ADDR, usdcAmount);
        _approveDelegation(VDEBT_USDT, user, ROUTER_ADDR, borrowAmount);

        uint256 usdtBefore = IERC20(USDT).balanceOf(user);

        bytes memory callData = _readCalldata("aave_deposit_usdc_borrow_usdt");
        uint256 value = _readValue("aave_deposit_usdc_borrow_usdt");

        vm.prank(user);
        (bool success,) = ROUTER_ADDR.call{ value: value }(callData);
        assertTrue(success, "Aave USDC deposit + USDT borrow should succeed");

        uint256 aUsdcBal = IERC20(A_USDC).balanceOf(user);
        uint256 usdtAfter = IERC20(USDT).balanceOf(user);
        uint256 usdcAfter = IERC20(USDC).balanceOf(user);
        uint256 debt = IERC20(VDEBT_USDT).balanceOf(user);

        assertGt(aUsdcBal, 0, "user should hold aUSDC after deposit");
        assertEq(usdcAfter, 0, "all USDC should be deposited");
        assertApproxEqAbs(
            usdtAfter - usdtBefore, borrowAmount, 10, "USDT gained should equal borrow"
        );
        // Aave accrues interest at ~1 wei per block on a fresh borrow.
        assertApproxEqAbs(debt, borrowAmount, 100, "USDT variable debt ~= borrow");

        address[] memory cleared = new address[](2);
        cleared[0] = USDC;
        cleared[1] = USDT;
        _assertRouterCleared(cleared);

        console.log("Test 2b: aUSDC", aUsdcBal, "USDT gained", usdtAfter - usdtBefore);
    }

    // ═════════════════════════════════════════════════════════════════════
    // #3 Leveraged long ETH via Balancer flashloan (hand-rolled)
    //
    //   user supplies 10k USDC of their own  →  pre-deposited as aUSDC
    //   flashloan 5k USDC from Balancer
    //     → swap 5k USDC → ~1.8 wstETH via Uniswap (0.05% pool)
    //     → deposit all received wstETH to Aave
    //     → borrow 5k USDC from Aave
    //   → repay 5k USDC flashloan to Balancer
    //
    // Net: 10k aUSDC + ~1.8 aWSTETH collateral, 5k USDC debt → leveraged
    // ETH exposure on user's $10k capital.
    //
    // Why a 5k flashloan instead of 20k: the USDC/wstETH 0.05% Uniswap pool
    // can't deliver enough wstETH for a 20k USDC swap to satisfy Aave's LTV
    // for a 20k borrow. 5k USDC sits comfortably within pool depth and
    // Aave's combined-collateral borrow capacity.
    // Why wstETH on the leveraged side: Aave V3 set WETH reserve LTV to 0
    // on mainnet post-2024, so WETH-collateral borrows revert.
    // Why pre-deposit user's USDC instead of folding it into the flashloan:
    // the compiler rejects an inner step chain whose token-flow doesn't
    // balance against the flashloaned amount.
    // ═════════════════════════════════════════════════════════════════════

    function test_fork_long_eth_3x_balancer() public {
        uint256 ownContribution = 10_000 * 1e6;
        uint256 flashAmount = 5_000 * 1e6;

        _dealERC20(USDC, user, ownContribution);
        vm.prank(user);
        IERC20(USDC).approve(ROUTER_ADDR, ownContribution);
        _approveDelegation(VDEBT_USDC, user, ROUTER_ADDR, flashAmount * 2);

        uint256 usdcBefore = IERC20(USDC).balanceOf(user);
        uint256 aUsdcBefore = IERC20(A_USDC).balanceOf(user);
        uint256 aWstethBefore = IERC20(A_WSTETH).balanceOf(user);
        uint256 debtBefore = IERC20(VDEBT_USDC).balanceOf(user);

        bytes memory callData = _readCalldata("long_eth_3x_balancer");
        uint256 value = _readValue("long_eth_3x_balancer");

        vm.prank(user);
        (bool success,) = ROUTER_ADDR.call{ value: value }(callData);
        assertTrue(success, "leveraged long via Balancer flashloan should succeed");

        uint256 usdcAfter = IERC20(USDC).balanceOf(user);
        uint256 aUsdcAfter = IERC20(A_USDC).balanceOf(user);
        uint256 aWstethAfter = IERC20(A_WSTETH).balanceOf(user);
        uint256 debtAfter = IERC20(VDEBT_USDC).balanceOf(user);

        assertEq(
            usdcBefore - usdcAfter, ownContribution, "user spent exactly their USDC contribution"
        );
        // Aave can round aToken minting down by 1-2 wei vs the supplied amount.
        assertGe(aUsdcAfter - aUsdcBefore, ownContribution - 10, "user pre-deposit landed as aUSDC");
        // Swap output varies with USDC/wstETH 0.05% pool liquidity at the fork
        // block; require at least ~0.8 wstETH (worst-case safe bound).
        assertGt(aWstethAfter - aWstethBefore, 0.8 ether, "user gained leveraged wstETH collateral");
        assertApproxEqAbs(
            debtAfter - debtBefore, flashAmount, flashAmount / 1000, "USDC debt ~= flashloan"
        );

        address[] memory cleared = new address[](2);
        cleared[0] = USDC;
        cleared[1] = WSTETH;
        _assertRouterCleared(cleared);

        console.log("Test 3: aWSTETH gained", aWstethAfter - aWstethBefore);
        console.log("Test 3: USDC debt opened", debtAfter - debtBefore);
    }

    // ═════════════════════════════════════════════════════════════════════
    // #4 1x short ETH (deposit USDC, borrow WETH, swap WETH→USDC)
    //
    // No flashloan needed at 1x. Net: 10k aUSDC collateral, 1 WETH debt,
    // ~$3k extra USDC in wallet from the WETH→USDC swap. User profits if
    // ETH falls (WETH debt becomes cheaper to repay).
    // ═════════════════════════════════════════════════════════════════════

    function test_fork_short_eth_1x() public {
        uint256 usdcCollateral = 10_000 * 1e6;
        uint256 wethBorrow = 1 ether;

        _dealERC20(USDC, user, usdcCollateral);
        vm.prank(user);
        IERC20(USDC).approve(ROUTER_ADDR, usdcCollateral);
        _approveDelegation(VDEBT_WETH, user, ROUTER_ADDR, wethBorrow);

        uint256 usdcBefore = IERC20(USDC).balanceOf(user);
        uint256 wethBefore = IERC20(WETH).balanceOf(user);

        bytes memory callData = _readCalldata("short_eth_1x");
        uint256 value = _readValue("short_eth_1x");

        vm.prank(user);
        (bool success,) = ROUTER_ADDR.call{ value: value }(callData);
        assertTrue(success, "1x short ETH should succeed");

        uint256 usdcAfter = IERC20(USDC).balanceOf(user);
        uint256 wethAfter = IERC20(WETH).balanceOf(user);
        uint256 aUsdcBal = IERC20(A_USDC).balanceOf(user);
        uint256 debt = IERC20(VDEBT_WETH).balanceOf(user);

        assertGt(aUsdcBal, 0, "user should hold aUSDC from collateral deposit");
        assertEq(wethAfter, wethBefore, "borrowed WETH should be fully swapped to USDC");
        assertApproxEqAbs(debt, wethBorrow, 100, "WETH variable debt ~= borrow");
        // After execution, user's USDC delta = (swap proceeds - 10k collateral).
        // Just require the swap produced something positive.
        assertGt(usdcAfter, usdcBefore - usdcCollateral, "swap should have produced USDC");

        address[] memory cleared = new address[](2);
        cleared[0] = USDC;
        cleared[1] = WETH;
        _assertRouterCleared(cleared);

        console.log(
            "Test 4: aUSDC", aUsdcBal, "USDC from swap", usdcAfter - (usdcBefore - usdcCollateral)
        );
    }

    // ═════════════════════════════════════════════════════════════════════
    // #4b Stake → wrap → Aave deposit → borrow, all in one signed intent.
    //
    //   100 ETH → Lido.submit() (router gets ~100 stETH)
    //          → stETH.approve(wstETH) + wstETH.wrap (router gets ~84 wstETH)
    //          → wstETH.approve(pool) + pool.supply (~79.84 wstETH)
    //          → pool.borrow 20k USDC on behalf of user
    //          → sweep leftover wstETH + USDC back to user
    //
    // Why 79.84 wstETH instead of 99.9 (= stETH `all`): a previous compiler
    // bug treated wstETH and stETH as 1:1, so an `"all"` deposit downstream
    // of `wrap stETH` asked the pool to pull more wstETH than the wrap
    // produced, reverting with `ERC20: transfer amount exceeds balance`.
    // Fixed by applying the configured stETH/wstETH pool rate inside
    // `step_produces`. Locking the regression in here.
    //
    // The user only authorizes credit delegation for USDC vDebt; no stETH
    // / wstETH approve is needed because both intermediates live entirely
    // inside the router until sweep.
    // ═════════════════════════════════════════════════════════════════════

    function test_fork_stakeWrapDepositBorrow() public {
        uint256 stakeAmount = 100 ether;
        uint256 borrowAmount = 20_000 * 1e6;

        vm.deal(user, stakeAmount + 1 ether);

        // The compiler signer (vitalik.eth on mainnet) carries stETH /
        // wstETH dust. Snapshot starting balances and assert they're
        // untouched after the call — a regression where the enricher
        // pulls stETH from the user wallet (instead of relying on the
        // staked-into-router balance) would show up as a delta here.
        uint256 stethBefore = IERC20(STETH).balanceOf(user);
        uint256 wstethBefore = IERC20(WSTETH).balanceOf(user);

        _approveDelegation(VDEBT_USDC, user, ROUTER_ADDR, borrowAmount);

        uint256 usdcBefore = IERC20(USDC).balanceOf(user);
        uint256 aWstethBefore = IERC20(A_WSTETH).balanceOf(user);

        bytes memory callData = _readCalldata("stake_wrap_deposit_borrow");
        uint256 value = _readValue("stake_wrap_deposit_borrow");
        assertEq(value, stakeAmount, "msg.value should equal stake amount");

        vm.prank(user);
        (bool success,) = ROUTER_ADDR.call{ value: value }(callData);
        assertTrue(success, "stake -> wrap -> deposit -> borrow should succeed");

        uint256 aWstethAfter = IERC20(A_WSTETH).balanceOf(user);
        uint256 usdcAfter = IERC20(USDC).balanceOf(user);
        uint256 wstethAfter = IERC20(WSTETH).balanceOf(user);
        uint256 debt = IERC20(VDEBT_USDC).balanceOf(user);

        // The supply landed as aWSTETH on the user. Aave can round aToken
        // minting down by 1-2 wei; require ≥ 78 wstETH (well below the
        // ~79.84 the compiler emits, with margin for stETH share rounding).
        assertGt(aWstethAfter - aWstethBefore, 78 ether, "supply produced aWSTETH");
        // 20k USDC borrowed + sweep back to user.
        assertApproxEqAbs(
            usdcAfter - usdcBefore,
            borrowAmount,
            10,
            "USDC gained should equal borrow"
        );
        // Aave accrues interest at ~1 wei per block on a fresh borrow.
        assertApproxEqAbs(debt, borrowAmount, 100, "USDC variable debt ~= borrow");
        // Sweep returned the slack wstETH (the router only deposited the
        // conservative-rate amount; the rest comes back).
        assertGt(wstethAfter - wstethBefore, 0, "sweep returned slack wstETH to user");
        // The slack stETH (router got ~100 stETH from staking, wrapped 99.9,
        // leftover ~0.1 stETH) is also swept back. The user's stETH balance
        // therefore goes UP, never down: a regression that pulled stETH
        // from the wallet for the wrap step would show as a *decrease*.
        assertGe(
            IERC20(STETH).balanceOf(user),
            stethBefore,
            "user stETH balance must not decrease (no transferFrom from wallet)"
        );

        // stETH is rebasing — transfer rounds down to whole shares, so the
        // router can retain 1-2 wei of stETH after sweep. Tolerate dust.
        assertLe(IERC20(STETH).balanceOf(ROUTER_ADDR), 5, "router stETH within dust");
        address[] memory cleared = new address[](2);
        cleared[0] = WSTETH;
        cleared[1] = USDC;
        _assertRouterCleared(cleared);

        console.log("Test 4b: aWSTETH gained", aWstethAfter - aWstethBefore);
        console.log("Test 4b: wstETH swept back", wstethAfter);
        console.log("Test 4b: USDC borrowed", usdcAfter - usdcBefore);
    }

    // ═════════════════════════════════════════════════════════════════════
    // #5 Uniswap V3 LP open + close (USDC/USDT 0.05%, ~0.95-1.05 range)
    //
    //   Open:  10k USDC + 10k USDT → mint LP via DSL fixture, executeDirect
    //   Close: capture runtime tokenId, call NPM.decreaseLiquidity + collect
    //          (the DSL's lp_decrease/lp_collect steps take a position_id
    //           which must be substituted at runtime, so the close half is
    //           built directly in Solidity as a thin shim — same calls the
    //           compiler would emit for a hand-written close intent.)
    //
    // Tick range: -510 (price ≈ 0.9503) to +490 (price ≈ 1.0502), tick spacing
    // 10 for the 0.05% pool. min_amount_*: 9500 each — tolerates pool skew
    // (pool may not deposit exactly 10k each if skewed off the 1:1 mark).
    // ═════════════════════════════════════════════════════════════════════

    function test_fork_lp_open_close_usdc_usdt() public {
        uint256 amt = 10_000 * 1e6;

        _dealERC20(USDC, user, amt);
        _dealERC20(USDT, user, amt);
        vm.prank(user);
        IERC20(USDC).approve(ROUTER_ADDR, amt);
        _approveUSDTBypass(user, ROUTER_ADDR, amt);

        uint256 nftBefore = _nftBalance(user);
        uint256 usdcBefore = IERC20(USDC).balanceOf(user);
        uint256 usdtBefore = IERC20(USDT).balanceOf(user);

        // ── Open via DSL ────────────────────────────────────────────────
        bytes memory mintData = _readCalldata("lp_mint_usdc_usdt_0p95_1p05");
        uint256 mintValue = _readValue("lp_mint_usdc_usdt_0p95_1p05");
        vm.prank(user);
        (bool mintOk,) = ROUTER_ADDR.call{ value: mintValue }(mintData);
        assertTrue(mintOk, "LP mint should succeed");

        assertEq(_nftBalance(user), nftBefore + 1, "user should own +1 LP NFT");
        uint256 tokenId = _latestTokenId(user);
        uint128 liquidity = _positionLiquidity(tokenId);
        assertGt(liquidity, 0, "minted position should have nonzero liquidity");

        // ── Close: decrease all liquidity + collect everything ──────────
        vm.prank(user);
        IERC721(NPM).approve(ROUTER_ADDR, tokenId);

        // Build the close calls the same way the DSL's lp_decrease + lp_collect
        // would compile. We bypass the router for the close because position_id
        // can't be baked into a static fixture.
        vm.prank(user);
        (bool decOk,) = NPM.call(
            abi.encodeWithSignature(
                "decreaseLiquidity((uint256,uint128,uint256,uint256,uint256))",
                INPM.DecreaseLiquidityParams({
                    tokenId: tokenId,
                    liquidity: liquidity,
                    amount0Min: 0,
                    amount1Min: 0,
                    deadline: block.timestamp + 600
                })
            )
        );
        assertTrue(decOk, "decreaseLiquidity should succeed");

        vm.prank(user);
        (bool colOk,) = NPM.call(
            abi.encodeWithSignature(
                "collect((uint256,address,uint128,uint128))",
                INPM.CollectParams({
                    tokenId: tokenId,
                    recipient: user,
                    amount0Max: type(uint128).max,
                    amount1Max: type(uint128).max
                })
            )
        );
        assertTrue(colOk, "collect should succeed");

        assertEq(uint256(_positionLiquidity(tokenId)), 0, "position liquidity should be drained");

        uint256 usdcAfter = IERC20(USDC).balanceOf(user);
        uint256 usdtAfter = IERC20(USDT).balanceOf(user);

        // After full open + close cycle, user should have ~ their original
        // balance back (minus negligible pool fees + dust). Allow 1% drift to
        // tolerate pool fee accounting and any rounding.
        uint256 maxLoss = amt / 100;
        assertGe(usdcAfter, usdcBefore - maxLoss, "USDC balance roughly restored");
        assertGe(usdtAfter, usdtBefore - maxLoss, "USDT balance roughly restored");

        console.log("Test 5: tokenId", tokenId, "liquidity drained");
        console.log(
            "Test 5: USDC delta", usdcBefore - usdcAfter, "USDT delta", usdtBefore - usdtAfter
        );
    }

    // ─── Helpers ─────────────────────────────────────────────────────────

    function _allowTarget(address target) internal {
        bytes32 slot = keccak256(abi.encode(target, uint256(3)));
        vm.store(ROUTER_ADDR, slot, bytes32(uint256(1)));
    }

    function _readCalldata(string memory name) internal view returns (bytes memory) {
        return vm.parseBytes(vm.readFile(string.concat("test/fixtures/", name, ".txt")));
    }

    function _readValue(string memory name) internal view returns (uint256) {
        return vm.parseUint(vm.readFile(string.concat("test/fixtures/", name, "_value.txt")));
    }

    function _dealERC20(address token, address to, uint256 amount) internal {
        deal(token, to, amount);
    }

    function _assertRouterCleared(address[] memory tokens) internal view {
        for (uint256 i = 0; i < tokens.length; i++) {
            assertEq(IERC20(tokens[i]).balanceOf(ROUTER_ADDR), 0, "router should not retain tokens");
        }
        assertEq(ROUTER_ADDR.balance, 0, "router should not retain ETH");
    }

    /// USDT.approve does not return bool — strict-IERC20 wrappers revert
    /// decoding the empty returndata. Use a raw call.
    function _approveUSDTBypass(address from, address spender, uint256 amount) internal {
        vm.prank(from);
        (bool ok,) = USDT.call(abi.encodeWithSignature("approve(address,uint256)", spender, amount));
        require(ok, "USDT approve failed");
    }

    /// Aave V3 requires credit delegation when msg.sender (router) is not
    /// the onBehalfOf (user) on a borrow.
    function _approveDelegation(
        address vDebtToken,
        address delegator,
        address delegatee,
        uint256 amount
    ) internal {
        vm.prank(delegator);
        (bool ok,) = vDebtToken.call(
            abi.encodeWithSignature("approveDelegation(address,uint256)", delegatee, amount)
        );
        require(ok, "approveDelegation failed");
    }

    // ─── Uniswap V3 NPM helpers ─────────────────────────────────────────

    function _nftBalance(address who) internal view returns (uint256) {
        (bool ok, bytes memory ret) =
            NPM.staticcall(abi.encodeWithSignature("balanceOf(address)", who));
        require(ok && ret.length == 32, "NPM.balanceOf failed");
        return abi.decode(ret, (uint256));
    }

    function _latestTokenId(address who) internal view returns (uint256) {
        uint256 bal = _nftBalance(who);
        require(bal > 0, "no NFT held");
        (bool ok, bytes memory ret) = NPM.staticcall(
            abi.encodeWithSignature("tokenOfOwnerByIndex(address,uint256)", who, bal - 1)
        );
        require(ok && ret.length == 32, "NPM.tokenOfOwnerByIndex failed");
        return abi.decode(ret, (uint256));
    }

    /// NPM.positions returns a 12-tuple; slot 7 is liquidity.
    function _positionLiquidity(uint256 tokenId) internal view returns (uint128) {
        (bool ok, bytes memory ret) =
            NPM.staticcall(abi.encodeWithSignature("positions(uint256)", tokenId));
        require(ok && ret.length >= 12 * 32, "NPM.positions failed");
        uint256 liq;
        assembly {
            liq := mload(add(ret, add(32, mul(7, 32))))
        }
        return uint128(liq);
    }
}

// ─── Minimal external interfaces used only by these tests ─────────────────

interface IERC721 {
    function approve(address to, uint256 tokenId) external;
}

interface INPM {
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
}
