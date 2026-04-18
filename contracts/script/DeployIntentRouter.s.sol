// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { Script, console } from "forge-std/Script.sol";
import { IntentRouter } from "../src/IntentRouter.sol";

contract DeployIntentRouter is Script {
    function run() external returns (address router) {
        uint256 pk = vm.envUint("DEPLOYER_PRIVATE_KEY");
        vm.startBroadcast(pk);
        router = address(new IntentRouter());
        vm.stopBroadcast();

        console.log("IntentRouter deployed at:", router);
    }
}
