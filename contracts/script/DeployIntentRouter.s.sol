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

        IntentRouter r = new IntentRouter();
        router = address(r);

        // Mainnet addresses — kept in sync with
        // intentOS-ui/lib/config/{assets,protocols}-ethereum.json.
        address[] memory targets = new address[](10);
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
        targets[8] = 0xE592427A0AEce92De3Edee1F18E0157C05861564; // Uniswap V3 Router
        targets[9] = 0x111111125421cA6dc452d289314280a0f8842A65; // 1inch v6 Router

        r.setAllowedTargets(targets, true);

        vm.stopBroadcast();

        console.log("IntentRouter deployed at:", router);
        console.log("Allowlisted targets:", targets.length);
    }
}
