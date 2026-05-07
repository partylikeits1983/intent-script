// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { Script, console } from "forge-std/Script.sol";
import { IntentRouter } from "../src/IntentRouter.sol";

/// @notice Deploys IntentRouter to Base (or a Base fork) and seeds its
///         allowlist with the Base addresses of every token and protocol the
///         compiler can currently produce calls to. Balancer is not deployed
///         on Base, so the constructor's `balancerVault` arg is `address(0)`
///         and the `receiveFlashLoan` callback is permanently disabled on
///         this deployment. Aave V3 is wired so the leverage sugar's
///         `flashLoanSimple` path executes natively.
contract DeployIntentRouterBase is Script {
    // ─── Constructor args (per-chain) ─────────────────────────────
    address constant BALANCER_VAULT_BASE = address(0);
    address constant AAVE_POOL_BASE = 0xA238Dd80C259a72e81d7e4664a9801593F98d1c5;

    // ─── Token addresses (kept in sync with config/assets/base.json) ─
    address constant WETH_BASE = 0x4200000000000000000000000000000000000006;
    address constant USDC_BASE = 0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913;
    address constant USDT_BASE = 0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2;
    address constant USDBC_BASE = 0xd9aAEc86B65D86f6A7B5B1b0c42FFA531710b6CA;
    address constant CBETH_BASE = 0x2Ae3F1Ec7F1F5012CFEab0185bfc7aa3cf0DEc22;
    address constant CBBTC_BASE = 0xcbB7C0000aB88B473b1f5aFd9ef808440eed33Bf;

    // ─── Protocol addresses (kept in sync with config/protocols/base.json) ─
    address constant UNISWAP_V3_ROUTER02_BASE = 0x2626664c2603336E57B271c5C0b26F421741e481;
    address constant UNISWAP_V3_QUOTER_BASE = 0x3d4e44Eb1374240CE5F1B871ab261CD16335B76a;
    address constant UNISWAP_V3_POSITION_MANAGER_BASE =
        0x03a520b32C04BF3bEEf7BEb72E919cf822Ed34f1;

    function run() external returns (address router) {
        uint256 pk = vm.envUint("DEPLOYER_PRIVATE_KEY");

        vm.startBroadcast(pk);

        IntentRouter r = new IntentRouter(BALANCER_VAULT_BASE, AAVE_POOL_BASE);
        router = address(r);

        // Base addresses — kept in sync with
        // intentOS-ui/lib/config/{assets,protocols}-base.json. Lido and
        // Balancer are intentionally absent (not deployed on Base).
        address[] memory targets = new address[](10);
        // Tokens
        targets[0] = WETH_BASE;
        targets[1] = USDC_BASE;
        targets[2] = USDT_BASE;
        targets[3] = USDBC_BASE;
        targets[4] = CBETH_BASE;
        targets[5] = CBBTC_BASE;
        // Protocols
        targets[6] = AAVE_POOL_BASE;
        targets[7] = UNISWAP_V3_ROUTER02_BASE;
        targets[8] = UNISWAP_V3_QUOTER_BASE;
        targets[9] = UNISWAP_V3_POSITION_MANAGER_BASE;

        r.setAllowedTargets(targets, true);

        vm.stopBroadcast();

        console.log("IntentRouter deployed at:", router);
        console.log("Allowlisted targets:", targets.length);
        console.log("Network: Base (chain id 8453, no Balancer, no Lido)");
    }
}
