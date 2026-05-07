// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { Test } from "forge-std/Test.sol";
import { IntentRouter } from "../src/IntentRouter.sol";
import { WETH9 } from "../src/mocks/WETH9.sol";

/// @notice B9: executeSigned must require msg.value == batch.totalValue.
///
/// Without this, a relayer could attach extra ETH to a signed intent and
/// trigger native-value semantics on a permissive allowlisted target.
/// The digest now includes `totalValue`, so any attempt to alter it
/// after signing either breaks the signature or is caught by the
/// msg.value equality check.
contract IntentRouterTotalValueTest is Test {
    IntentRouter router;
    WETH9 weth;

    function setUp() public {
        router = new IntentRouter(0xBA12222222228d8Ba445958a75a0704d566BF2C8, 0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2);
        weth = new WETH9();
        router.setAllowedTarget(address(weth), true);
        vm.warp(1_000_000);
    }

    function _wrapBatch(uint256 signerPk, uint256 value)
        internal
        view
        returns (IntentRouter.IntentBatch memory)
    {
        IntentRouter.Call[] memory calls = new IntentRouter.Call[](1);
        calls[0] = IntentRouter.Call({
            target: address(weth),
            callData: abi.encodeWithSignature("deposit()"),
            value: value
        });
        address[] memory sweep = new address[](1);
        sweep[0] = address(weth);
        return IntentRouter.IntentBatch({
            signer: vm.addr(signerPk),
            calls: calls,
            tokensToSweep: sweep,
            nonce: 0,
            deadline: block.timestamp + 3600,
            totalValue: value
        });
    }

    function _digest(IntentRouter.IntentBatch memory batch) internal view returns (bytes32) {
        bytes32 callTypehash = keccak256("Call(address target,bytes callData,uint256 value)");
        bytes32 batchTypehash = keccak256(
            "IntentBatch(address signer,Call[] calls,address[] tokensToSweep,uint256 nonce,uint256 deadline,uint256 totalValue)Call(address target,bytes callData,uint256 value)"
        );
        bytes32[] memory hashes = new bytes32[](batch.calls.length);
        for (uint256 i = 0; i < batch.calls.length; i++) {
            hashes[i] = keccak256(
                abi.encode(
                    callTypehash,
                    batch.calls[i].target,
                    keccak256(batch.calls[i].callData),
                    batch.calls[i].value
                )
            );
        }
        bytes32 structHash = keccak256(
            abi.encode(
                batchTypehash,
                batch.signer,
                keccak256(abi.encodePacked(hashes)),
                keccak256(abi.encodePacked(batch.tokensToSweep)),
                batch.nonce,
                batch.deadline,
                batch.totalValue
            )
        );
        return keccak256(abi.encodePacked(hex"1901", router.DOMAIN_SEPARATOR(), structHash));
    }

    function _sig(uint256 pk, bytes32 digest) internal pure returns (bytes memory) {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(pk, digest);
        return abi.encodePacked(r, s, v);
    }

    function test_exact_msg_value_succeeds() public {
        uint256 pk = 0xA11CE;
        vm.deal(vm.addr(pk), 10 ether);
        IntentRouter.IntentBatch memory batch = _wrapBatch(pk, 1 ether);
        bytes memory sig = _sig(pk, _digest(batch));

        address relayer = vm.addr(0xFEE);
        vm.deal(relayer, 10 ether);
        vm.prank(relayer);
        router.executeSigned{ value: 1 ether }(batch, sig);
    }

    function test_overpayment_reverts() public {
        // Attacker attaches extra ETH on top of the signed total. The
        // signature is over totalValue=1 ether, so the digest also
        // validates — the msg.value mismatch is the only thing catching
        // this.
        uint256 pk = 0xA11CE;
        IntentRouter.IntentBatch memory batch = _wrapBatch(pk, 1 ether);
        bytes memory sig = _sig(pk, _digest(batch));

        address relayer = vm.addr(0xFEE);
        vm.deal(relayer, 10 ether);
        vm.prank(relayer);
        vm.expectRevert(bytes("msg.value mismatch"));
        router.executeSigned{ value: 2 ether }(batch, sig);
    }

    function test_underpayment_reverts() public {
        uint256 pk = 0xA11CE;
        IntentRouter.IntentBatch memory batch = _wrapBatch(pk, 1 ether);
        bytes memory sig = _sig(pk, _digest(batch));

        address relayer = vm.addr(0xFEE);
        vm.deal(relayer, 10 ether);
        vm.prank(relayer);
        vm.expectRevert(bytes("msg.value mismatch"));
        router.executeSigned{ value: 0.5 ether }(batch, sig);
    }

    function test_mutated_totalValue_breaks_signature() public {
        // Sign totalValue = 1 ether, then mutate to 2 ether after signing.
        // The digest changes, so the signature fails to recover.
        uint256 pk = 0xA11CE;
        IntentRouter.IntentBatch memory signedBatch = _wrapBatch(pk, 1 ether);
        bytes memory sig = _sig(pk, _digest(signedBatch));

        IntentRouter.IntentBatch memory mutated = signedBatch;
        mutated.totalValue = 2 ether;

        address relayer = vm.addr(0xFEE);
        vm.deal(relayer, 10 ether);
        vm.prank(relayer);
        vm.expectRevert(); // could be msg.value mismatch or invalid signature depending on amount
        router.executeSigned{ value: 2 ether }(mutated, sig);
    }
}
