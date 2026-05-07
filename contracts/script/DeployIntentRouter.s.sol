// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { Script, console } from "forge-std/Script.sol";
import { IntentRouter } from "../src/IntentRouter.sol";

/// @notice Deploys IntentRouter and seeds its allowlist with the mainnet
/// addresses of every token and protocol the compiler can currently produce
/// calls to. Without the allowlist, _executeCalls reverts with "Target not
/// allowed" and batched intents fail silently in simulation.
contract DeployIntentRouter is Script {
    function run() external returns (address router) {
        uint256 pk = vm.envUint("DEPLOYER_PRIVATE_KEY");

        vm.startBroadcast(pk);

        // L1 deployment: both flashloan providers wired. For Base, deploy
        // with `(address(0), <Base Aave Pool>)` instead — see
        // intent-script/scripts/start-anvil-base.sh for the canonical
        // per-chain constructor args.
        address balancerVault = 0xBA12222222228d8Ba445958a75a0704d566BF2C8;
        address aavePool = 0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2;
        IntentRouter r = new IntentRouter(balancerVault, aavePool);
        router = address(r);

        // Mainnet addresses — kept in sync with
        // intentOS-ui/lib/config/{assets,protocols}-ethereum.json.
        address[] memory targets = new address[](14);
        // Tokens (approve / transfer targets)
        targets[0] = 0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2; // WETH
        targets[1] = 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48; // USDC
        targets[2] = 0xdAC17F958D2ee523a2206206994597C13D831ec7; // USDT
        targets[3] = 0x6B175474E89094C44Da98b954EedeAC495271d0F; // DAI
        targets[4] = 0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599; // WBTC
        targets[5] = 0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84; // stETH
        targets[6] = 0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0; // wstETH
        // Protocols
        targets[7] = 0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2; // Aave V3 Pool
        targets[8] = 0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45; // Uniswap V3 Router
        targets[9] = 0xC36442b4a4522E871399CD717aBDD847Ab11FE88; // Uniswap V3 NonfungiblePositionManager
        targets[10] = 0x889edC2eDab5f40e902b864aD4d7AdE8E412F9B1; // Lido Withdrawal Queue
        targets[11] = 0xBA12222222228d8Ba445958a75a0704d566BF2C8; // Balancer V2 Vault (flashloans)
        targets[12] = 0xBBBBBbbBBb9cC5e90e3b3Af64bdAF62C37EEFFCb; // Morpho Blue
        targets[13] = 0x5c7BCd6E7De5423a257D81B442095A1a6ced35C5; // Across V3 SpokePool

        r.setAllowedTargets(targets, true);

        vm.stopBroadcast();

        console.log("IntentRouter deployed at:", router);
        console.log("Allowlisted targets:", targets.length);
    }
}
