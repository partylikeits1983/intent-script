# Intent-Script Architecture

> **Load this file** when you need to understand the compiler pipeline, module layout, key types, and data flow. This is the primary reference for making code changes.

## Compiler Pipeline

```
JSON Input → Parse → Normalize → Validate → Enrich → Lower → Plan → Build → CompileOutput
              (A)      (B)         (C)        (D)      (E)     (F)    (G)
```

The pipeline is orchestrated in `crates/intent-script/src/compiler/mod.rs:26` — the `compile()` function.

### Stage A: Parse (`serde_json`)
- Deserializes JSON into `IntentScript` public AST types
- File: `crates/intent-script/src/schema/public_ast.rs`
- Types are string-based (aliases, human amounts) — no resolution yet

### Stage B: Normalize (`compiler/normalize.rs`)
- Resolves aliases to addresses (`"USDC"` → `0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48`)
- Parses human amounts to `U256` with correct decimals (`"1000"` USDC → `1000000000` with 6 decimals)
- Maps protocol names to deployment addresses (`"aave"` → pool address)
- Resolves `"all"` amounts to guaranteed output of prior step
- Computes swap deadlines from `current_timestamp`
- Computes `amount_out_minimum` from `min_amount_out` or `price`+`slippage`
- Produces `ResolvedIntent` with `Vec<ResolvedStep>`

### Stage C: Validate (`compiler/validate.rs`)
- Checks signer is not zero address
- Validates step count (max 5)
- Validates amounts are positive
- Validates slippage protection on swaps (rejects `amount_out_minimum == 0`)
- Validates asset compatibility (no native ETH into Aave, no swap-to-self)
- Cross-step amount flow validation (consumption ≤ production)
- Aave health factor check (rejects HF < 1.2, warns HF < 1.5)
- Borrow feasibility check (warns/rejects based on collateral)
- Returns `ValidationResult { warnings: Vec<String> }`

### Stage D: Enrich (`compiler/enrich.rs`)
The most complex stage. When a router is available and there are multiple steps:
- Inserts `Erc20TransferFrom` to pull user tokens into the router
- Inserts `Erc20Approve` for protocol interactions (exact amounts)
- Redirects swap recipients to the router (intermediate tokens stay in router)
- Tracks `tokens_in_router` set to avoid unnecessary transfers
- Tracks `tokens_to_sweep` for borrowed assets and swap outputs
- Adds borrowed assets to sweep list (Aave sends borrowed tokens to `msg.sender` = router)

### Stage E: Lower (`compiler/lower.rs`)
- Dispatches each `ResolvedStep` to the appropriate adapter
- Each adapter ABI-encodes the calldata using `alloy_sol_types::sol!`
- Produces `Vec<ConcreteCall>` (target address + calldata + ETH value)

### Stage F: Plan (`compiler/plan.rs`)
Decides execution strategy:
- 1 call → `Single`
- N calls + router address available → `Batched` (single tx through IntentRouter)
- N calls + no router → `Sequence`

### Stage G: Build (`compiler/build.rs`)
Produces final `CompileOutput`:
- For `Batched`: encodes `executeDirect(calls, tokensToSweep)` calldata, computes EIP-712 typed data hash, produces both `directTx` and EIP-712 signing data
- For `Single`/`Sequence`: wraps `ConcreteCall` into `UnsignedTx`

---

## Module Layout

```
crates/intent-script/
├── src/
│   ├── main.rs                    # CLI: intent-script <input.json> [--config-dir] [--pretty]
│   ├── lib.rs                     # Public API: compile(json, chains, assets, protocols) → CompileResult
│   ├── error.rs                   # CompileError enum (10 variants)
│   ├── output.rs                  # CompileOutput, UnsignedTx, Eip712IntentOutput, JSON serialization
│   ├── eip712.rs                  # EIP-712 domain separator, struct hashing (matches Solidity)
│   ├── schema/
│   │   ├── mod.rs
│   │   └── public_ast.rs          # Serde types: IntentScript, Step enum, SwapStep, etc.
│   ├── ir/
│   │   ├── mod.rs
│   │   └── canonical.rs           # ResolvedIntent, ResolvedStep enum, ConcreteCall, step_consumes(), step_produces()
│   ├── registry/
│   │   ├── mod.rs
│   │   └── loader.rs              # RegistryContext: loads chains, assets, protocols from JSON strings
│   ├── compiler/
│   │   ├── mod.rs                 # Top-level compile() pipeline orchestration
│   │   ├── normalize.rs           # Stage B: AST → canonical IR
│   │   ├── validate.rs            # Stage C: IR validation
│   │   ├── enrich.rs              # Stage D: insert approvals, transfers, track sweeps
│   │   ├── lower.rs               # Stage E: IR → concrete calls via adapters
│   │   ├── plan.rs                # Stage F: execution strategy
│   │   └── build.rs               # Stage G: final tx building + EIP-712
│   └── adapters/
│       ├── mod.rs                 # lower_step() dispatcher
│       ├── wrap.rs                # WETH deposit/withdraw
│       ├── erc20.rs               # approve, transferFrom, permit
│       ├── aave_v3.rs             # supply, borrow, withdraw
│       ├── uniswap_v3.rs          # exactInputSingle
│       ├── lido.rs                # submit (stake), wstETH.wrap
│       ├── oneinch.rs             # Calldata passthrough
│       └── send.rs                # ERC-20 transfer, ETH send, ERC-721 safeTransferFrom
├── tests/
│   ├── integration.rs             # Compiler integration tests
│   ├── enricher_tests.rs          # Enrichment-specific tests
│   ├── fuzz_amounts.rs            # Amount parsing fuzz tests
│   ├── generate_calldata.rs       # Fixture generators for Foundry tests
│   └── generate_eip712_fixtures.rs # EIP-712 batch fixture generators
└── examples/
    ├── aave_borrow.json           # deposit USDC + borrow DAI
    ├── aave_deposit.json          # deposit USDC into Aave
    ├── aave_withdraw.json         # withdraw USDC from Aave
    ├── borrow_existing_collateral.json # borrow with existing collateral
    ├── complex_defi.json          # swap USDC→WETH + deposit WETH + borrow DAI
    ├── stake_lido.json            # stake ETH in Lido
    ├── stake_lido_wsteth.json     # stake ETH + wrap stETH→wstETH
    ├── swap_1inch.json            # swap via 1inch (needs calldata)
    ├── swap_uniswap.json          # swap USDC→WETH via Uniswap
    ├── swap_uniswap_slippage.json # swap with price+slippage params
    └── wrap_eth.json              # wrap ETH→WETH
```

### Solidity Router

```
contracts/
├── src/
│   ├── IntentRouter.sol           # Main router: executeDirect, executeSigned, allowlist, sweep
│   └── interfaces/
│       ├── IERC20.sol
│       └── IWETH.sol
└── test/
    ├── IntentRouter.t.sol         # Unit tests (mock-based)
    ├── IntentRouterCalldata.t.sol  # Calldata verification tests
    ├── IntentForkE2E.t.sol        # Fork E2E tests against mainnet
    └── IntentForkTests.t.sol      # Local mock-based integration tests
```

---

## Key Types

### Public AST (`schema/public_ast.rs`)

```rust
pub struct IntentScript {
    pub network: String,              // "ethereum"
    pub from: String,                 // "0x..." signer EOA
    pub steps: Vec<Step>,             // ordered action steps
    pub nonce: Option<u64>,           // EIP-712 replay protection
    pub deadline: Option<u64>,        // EIP-712 expiry timestamp
    pub current_timestamp: Option<u64>, // for deadline computation
    pub balances: Option<UserBalances>, // optional balance info for validation
}

pub enum Step {
    Swap(SwapStep), Deposit(DepositStep), Borrow(BorrowStep),
    Withdraw(WithdrawStep), Wrap(WrapStep), Unwrap(UnwrapStep),
    Stake(StakeStep), Send(SendStep), Custom(serde_json::Value),
}
```

### Canonical IR (`ir/canonical.rs`)

```rust
pub struct ResolvedIntent {
    pub chain_id: u64,
    pub signer: Address,
    pub steps: Vec<ResolvedStep>,
    pub tokens_to_sweep: Vec<Address>,
    pub nonce: u64,
    pub deadline: u64,
    pub user_balances: Option<ResolvedBalances>,
}

pub enum ResolvedStep {
    Wrap { wrapped_token, amount },
    Unwrap { wrapped_token, amount },
    Erc20Approve { token, spender, amount },           // auto-inserted
    Erc20TransferFrom { token, from, to, amount },     // auto-inserted
    AaveV3Supply { pool, asset, amount, on_behalf_of },
    AaveV3Borrow { pool, asset, amount, rate_mode, on_behalf_of },
    AaveV3Withdraw { pool, asset, amount, to },
    UniswapV3Swap { router, token_in, token_out, amount_in, fee, recipient, deadline, amount_out_minimum },
    LidoStake { lido, amount, referral },
    WstETHWrap { wsteth, steth, amount },
    OneInchSwap { router, token_in, token_out, amount_in, calldata },
    Erc20Permit { token, owner, spender, value, deadline },
    SendErc20 { token, to, amount },
    SendEth { to, amount },
    SendErc721 { contract, from, to, token_id },
}
```

### Concrete Call (`ir/canonical.rs`)

```rust
pub struct ConcreteCall {
    pub to: Address,          // target contract
    pub calldata: Bytes,      // ABI-encoded calldata
    pub value: U256,          // ETH value
    pub description: String,  // human-readable description
}
```

### Compile Output (`output.rs`)

```rust
pub enum CompileOutput {
    SingleTx(UnsignedTx),
    Eip712Intent(Eip712IntentOutput),  // batched + EIP-712 typed data
    TxSequence(Vec<UnsignedTx>),
    RequiresExecutor { reason: String },
}

pub struct CompileResult {
    pub output: CompileOutput,
    pub warnings: Vec<String>,
}
```

### Error Types (`error.rs`)

```rust
pub enum CompileError {
    UnknownNetwork(String),
    UnknownAsset { asset, network },
    UnknownProtocol { protocol, network },
    InvalidAmount(String),
    InvalidAddress(String),
    Config(String),
    UnsupportedStep(String),
    Validation(String),
    InvalidChain(String),    // also used for cross-step validation errors
    Adapter(String),
    Json(String),
}
```

---

## Public API

The library exposes a single entry point in `lib.rs`:

```rust
pub fn compile(
    json_input: &str,       // the intent JSON
    chains_json: &str,      // contents of config/chains.json
    assets_json: &str,      // contents of config/assets/{network}.json
    protocols_json: &str,   // contents of config/protocols/{network}.json
) -> Result<CompileResult>
```

The caller (CLI binary or frontend) reads config files and passes their contents as strings. The library does no file I/O.

---

## Token Flow Through Router (Batched Execution)

When multiple steps are batched through the IntentRouter:

1. **User approves router** — `token.approve(router, amount)` (prerequisite, not in batch)
2. **Credit delegation** — `variableDebtToken.approveDelegation(router, amount)` (prerequisite for borrows)
3. **Batch executes**:
   - `transferFrom(user, router, amount)` — pull tokens into router
   - `approve(protocol, amount)` — router approves protocol
   - Protocol calls (swap, supply, borrow, etc.)
   - Intermediate tokens stay in router (swap output, borrowed tokens)
4. **Sweep** — router transfers all remaining token balances back to user
5. **ETH refund** — any remaining ETH in router sent back to user

### Key Insight: Aave V3 Borrow Token Flow
Aave V3's `borrow()` sends borrowed tokens to `msg.sender` (the router), not `onBehalfOf` (the user). The enricher adds the borrowed asset to `tokens_to_sweep` so the router sweeps it back.

---

## Dependencies

```toml
# Rust (crates/intent-script/Cargo.toml)
alloy-primitives = "1"          # Address, U256, Bytes
alloy-sol-types = "1"           # sol! macro for ABI encoding
serde = "1" (derive, alloc)     # JSON deserialization
serde_json = "1"                # JSON parsing
hashbrown = "0.15"              # no-std HashMap/HashSet
hex = "0.4"                     # hex encoding
clap = "4" (optional, std only) # CLI argument parsing

# Solidity (contracts/foundry.toml)
forge-std (via git submodule)
```

## `no_std` Design

The library is `no_std` compatible:
- `lib.rs` has `#![cfg_attr(not(feature = "std"), no_std)]` with `extern crate alloc`
- Uses `hashbrown::HashMap` instead of `std::collections::HashMap`
- Uses `alloc::string::String`, `alloc::vec::Vec`, `alloc::format!`
- No file I/O, no `SystemTime`, no `std::sync`
- The CLI binary (`main.rs`) requires the `std` feature for file reading and `clap`
