// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import { Test } from "forge-std/Test.sol";
import { IntentRouter } from "../src/IntentRouter.sol";
import { IERC20 } from "../src/interfaces/IERC20.sol";

/// @title IntentForkBase
/// @notice Shared scaffolding for fork-mode tests that exercise the
///         compiler-generated EIP-712 batches end-to-end:
///
///           DSL  ─►  Rust compile()  ─►  fixture file  ─►  vm.sign  ─►  router.executeSigned
///
///         Subclasses inherit the etched-router setUp, the EIP-712 digest
///         builder, and the `<name>_batch.json` fixture reader, and only
///         have to write per-scenario funding + assertions.
///
///         The signer's private key is the standard Foundry test key
///         `vm.addr(SIGNER_PK)`. The compiler bakes that signer's address
///         into transferFrom(from=signer,to=router) calls in every example
///         JSON, so funding has to go to the same address.
abstract contract IntentForkBase is Test {
    // ─── Mainnet addresses (must match config/protocols/ethereum.json) ────
    address constant ROUTER_ADDR = 0x9fF4608bAEb3a055CcBBa85c2Aabaf6EF5c50120;
    address constant WETH = 0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2;
    address constant USDC = 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48;
    address constant USDT = 0xdAC17F958D2ee523a2206206994597C13D831ec7;
    address constant DAI = 0x6B175474E89094C44Da98b954EedeAC495271d0F;
    address constant WBTC = 0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599;
    address constant STETH = 0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84;
    address constant WSTETH = 0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0;
    address constant AAVE_POOL = 0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2;
    address constant UNI_ROUTER = 0xE592427A0AEce92De3Edee1F18E0157C05861564;
    address constant BALANCER_VAULT = 0xBA12222222228d8Ba445958a75a0704d566BF2C8;
    address constant NPM = 0xC36442b4a4522E871399CD717aBDD847Ab11FE88;

    address constant A_WETH = 0x4d5F47FA6A74757f35C14fD3a6Ef8E3C9BC514E8;
    address constant A_USDC = 0x98C23E9d8f34FEFb1B7BD6a91B7FF122F4e16F5c;
    address constant A_WBTC = 0x5Ee5bf7ae06D1Be5997A1A72006FE6C607eC6DE8;
    address constant A_WSTETH = 0x0B925eD163218f6662a35e0f0371Ac234f9E9371;

    address constant VDEBT_DAI = 0xcF8d0c70c850859266f5C338b38F9D663181C314;
    address constant VDEBT_USDC = 0x72E95b8931767C79bA4EeE721354d6E99a61D004;
    address constant VDEBT_USDT = 0x6ab707Aca953eDAeFBc4fD23bA73294241490620;
    address constant VDEBT_WETH = 0xeA51d7853EEFb32b6ee06b1C12E6dcCA88Be0fFE;

    /// @dev Foundry's `vm.addr(SIGNER_PK)` resolves to the COMPILER_SIGNER
    ///      address baked into every example DSL file. Keep both fields in
    ///      sync if the fixtures ever change `from`.
    uint256 constant SIGNER_PK = 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80;
    address constant COMPILER_SIGNER = 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045;

    IntentRouter public router;
    address public user;

    /// @dev IntentRouter EIP-712 typehashes — copy of the constants in
    ///      contracts/src/IntentRouter.sol, redeclared here so subclasses
    ///      don't need to import them.
    bytes32 internal constant CALL_TYPEHASH =
        keccak256("Call(address target,bytes callData,uint256 value)");
    bytes32 internal constant INTENT_BATCH_TYPEHASH = keccak256(
        "IntentBatch(address signer,Call[] calls,address[] tokensToSweep,uint256 nonce,uint256 deadline,uint256 totalValue)Call(address target,bytes callData,uint256 value)"
    );

    function setUp() public virtual {
        IntentRouter impl_ = new IntentRouter();
        vm.etch(ROUTER_ADDR, address(impl_).code);
        router = IntentRouter(payable(ROUTER_ADDR));

        // ReentrancyGuard._status defaults to 1 in the constructor; vm.etch
        // copies code but not storage, so we have to reinitialize that slot.
        vm.store(ROUTER_ADDR, bytes32(uint256(0)), bytes32(uint256(1)));

        // Pre-allowlist the protocol surface used across the integration suite.
        // Subclasses can call _allowTarget() for anything else they need.
        _allowTarget(WETH);
        _allowTarget(USDC);
        _allowTarget(USDT);
        _allowTarget(DAI);
        _allowTarget(WBTC);
        _allowTarget(STETH);
        _allowTarget(WSTETH);
        _allowTarget(AAVE_POOL);
        _allowTarget(UNI_ROUTER);
        _allowTarget(NPM);
        _allowTarget(BALANCER_VAULT);

        // The fixture's signer must hold its own ETH to authorize executeSigned
        // (the signer also pays the router's totalValue when it's the relayer).
        user = vm.addr(SIGNER_PK);
        require(user == COMPILER_SIGNER, "test signer pk must match COMPILER_SIGNER");
        vm.deal(user, 1000 ether);
    }

    /// @dev IntentRouter inherits ReentrancyGuard so allowedTargets lives at
    ///      slot 3 (slot 0 = _status, 1 = nonces, 2 = owner, 3 = allowlist).
    function _allowTarget(address target) internal {
        bytes32 slot = keccak256(abi.encode(target, uint256(3)));
        vm.store(ROUTER_ADDR, slot, bytes32(uint256(1)));
    }

    function _dealERC20(address token, address to, uint256 amount) internal {
        deal(token, to, amount);
    }

    /// @dev Aave V3 requires credit delegation when msg.sender (router) is
    ///      not the onBehalfOf (user) on a borrow.
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

    function _assertRouterCleared(address[] memory tokens) internal view {
        for (uint256 i = 0; i < tokens.length; i++) {
            assertEq(
                IERC20(tokens[i]).balanceOf(ROUTER_ADDR),
                0,
                "router should not retain swept token balance"
            );
        }
        assertEq(ROUTER_ADDR.balance, 0, "router should not retain ETH");
    }

    // ─── Fixture loader ───────────────────────────────────────────────────
    //
    // generate_integration_fixtures.rs writes two files per scenario:
    //   - test/fixtures/<name>_batch.bin: hex-encoded `abi.encode(IntentBatch)`
    //   - test/fixtures/<name>_batch.json: human-readable mirror
    //
    // Solidity reads the .bin form because abi.decode handles nested dynamic
    // arrays cleanly without per-field vm.parseJson*** calls.

    function _readBatch(string memory name)
        internal
        view
        returns (IntentRouter.IntentBatch memory batch)
    {
        string memory hexStr = vm.readFile(string.concat("test/fixtures/", name, "_batch.bin"));
        bytes memory raw = vm.parseBytes(hexStr);
        batch = abi.decode(raw, (IntentRouter.IntentBatch));
    }

    // ─── EIP-712 digest builder ───────────────────────────────────────────
    //
    // We re-derive the digest in Solidity (rather than reading the one
    // pre-computed by the Rust compiler) because IntentRouter's
    // DOMAIN_SEPARATOR is `immutable` — when vm.etch copies the impl's code
    // to ROUTER_ADDR, the baked-in verifyingContract is still the impl's
    // address, not ROUTER_ADDR. Using router.DOMAIN_SEPARATOR() guarantees
    // both sides agree on whichever value the etched bytecode actually
    // carries.

    function _buildDigest(IntentRouter.IntentBatch memory batch) internal view returns (bytes32) {
        bytes32[] memory callHashes = new bytes32[](batch.calls.length);
        for (uint256 i = 0; i < batch.calls.length; i++) {
            callHashes[i] = keccak256(
                abi.encode(
                    CALL_TYPEHASH,
                    batch.calls[i].target,
                    keccak256(batch.calls[i].callData),
                    batch.calls[i].value
                )
            );
        }

        bytes32 structHash = keccak256(
            abi.encode(
                INTENT_BATCH_TYPEHASH,
                batch.signer,
                keccak256(abi.encodePacked(callHashes)),
                keccak256(abi.encodePacked(batch.tokensToSweep)),
                batch.nonce,
                batch.deadline,
                batch.totalValue
            )
        );

        return keccak256(abi.encodePacked("\x19\x01", router.DOMAIN_SEPARATOR(), structHash));
    }

    function _signAndExecute(IntentRouter.IntentBatch memory batch) internal {
        bytes32 digest = _buildDigest(batch);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(SIGNER_PK, digest);
        bytes memory signature = abi.encodePacked(r, s, v);
        // The signer is also the relayer here. ETH for `totalValue` comes
        // out of their balance.
        vm.prank(user);
        router.executeSigned{ value: batch.totalValue }(batch, signature);
    }
}
