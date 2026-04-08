# Intent-Script Architecture

## Overview

A Rust compiler that takes human-friendly JSON intent descriptions and produces unsigned EVM transactions (or EIP-712 typed data for relayer submission). The compiler is the "complexity sink" — it hides protocol details, token addresses, decimals, approvals, token routing, and calldata generation from the JSON input.

The system has two main components:
1. **Rust compiler** (`crates/intent-script/`) — transforms JSON intents into calldata
2. **Solidity router** (`contracts/src/IntentRouter.sol`) — executes batched calls on-chain and sweeps tokens back to the user

## Current Capabilities

### Supported Actions
| Action | JSON Key | Protocol | Notes |
|--------|----------|----------|-------|
| Wrap ETH→WETH | `wrap` | WETH9 | `deposit()` with ETH value |
| Unwrap WETH→ETH | `unwrap` | WETH9 | `withdraw(amount)` |
| Wrap stETH→wstETH | `wrap` (asset=stETH) | Lido | `wstETH.wrap(amount)` |
| Stake ETH→stETH | `stake` | Lido | `lido.submit{value}(referral)` |
| Deposit into Aave | `deposit` | Aave V3 | `pool.supply(asset, amount, onBehalfOf, 0)` |
| Borrow from Aave | `borrow` | Aave V3 | `pool.borrow(asset, amount, rateMode, 0, onBehalfOf)` |
| Withdraw from Aave | `withdraw` | Aave V3 | `pool.withdraw(asset, amount, to)` |
| Swap via Uniswap | `swap` | Uniswap V3 | `router.exactInputSingle(params)` |
| Swap via 1inch | `swap` (via=1inch) | 1inch v6 | Passthrough pre-fetched calldata |

### Supported Networks
- Ethereum mainnet (chain ID 1) — fully configured with assets and protocols
- Sepolia, Base, Arbitrum — chain configs exist but no asset/protocol configs yet

### Execution Modes
| Mode | When | Output |
|------|------|--------|
| `SingleTx` | 1 call (e.g., wrap ETH) | Single unsigned tx |
| `Eip712Intent` | 2+ calls with router | Batched `executeDirect()` tx + EIP-712 typed data for `executeSigned()` |
| `TxSequence` | 2+ calls, no router | Multiple unsigned txs |

---

## Public JSON Schema (LLM-facing)

```json
{
  "network": "ethereum",
  "from": "0xYourEOA",
  "nonce": 0,
  "deadline": 0,
  "steps": [
    { "swap": { "from": "USDC", "amount": "5000", "to": "WETH" } },
    { "deposit": { "asset": "WETH", "amount": "2.0", "into": "aave" } },
    { "borrow": { "asset": "DAI", "amount": "1000", "from": "aave" } }
  ]
}
```

### Design Principles
- **Aliases over addresses** — `"ETH"`, `"USDC"`, `"aave"`, `"uniswap"`
- **Human-readable amounts** — `"1.5"`, `"10000"` (compiler handles decimals)
- **Sequential steps** — compiler infers dependencies, approvals, token routing
- **Minimal required keys** — no ABI, calldata, addresses, or decimals in the JSON
- **Automatic enrichment** — compiler inserts `transferFrom`, `approve`, token sweeps

---

## Compiler Pipeline

```
JSON Input → Parse → Normalize → Validate → Enrich → Lower → Plan → Build → CompileOutput
              (A)      (B)         (C)        (D)      (E)     (F)    (G)
```

### Stage A: Parse (`serde_json`)
Deserializes JSON into `IntentScript` public AST types.

### Stage B: Normalize (`compiler/normalize.rs`)
Resolves aliases to addresses, parses human amounts to `U256` with correct decimals, maps protocol names to deployment addresses. Produces `ResolvedIntent` with `Vec<ResolvedStep>`.

### Stage C: Validate (`compiler/validate.rs`)
Checks signer is not zero address, intent has at least one step. **Currently minimal — see next-steps for expansion.**

### Stage D: Enrich (`compiler/enrich.rs`)
The most complex stage. When a router is available and there are multiple steps:
- Inserts `Erc20TransferFrom` to pull user tokens into the router
- Inserts `Erc20Approve` for protocol interactions
- Redirects swap recipients to the router (intermediate tokens stay in router)
- Tracks `tokens_in_router` to avoid unnecessary transfers
- Tracks `tokens_to_sweep` for borrowed assets and swap outputs
- Adds borrowed assets to sweep list (Aave sends borrowed tokens to `msg.sender` = router)

### Stage E: Lower (`compiler/lower.rs`)
Dispatches each `ResolvedStep` to the appropriate adapter, which ABI-encodes the calldata. Produces `Vec<ConcreteCall>`.

### Stage F: Plan (`compiler/plan.rs`)
Decides execution strategy:
- 1 call → `Single`
- N calls + router → `Batched` (single tx through IntentRouter)
- N calls + no router → `Sequence`

### Stage G: Build (`compiler/build.rs`)
Produces final `CompileOutput`:
- For `Batched`: encodes `executeDirect(calls, tokensToSweep)` calldata, computes EIP-712 typed data hash
- For `Single`/`Sequence`: wraps `ConcreteCall` into `UnsignedTx`

---

## Module Layout

```
crates/intent-script/
├── src/
│   ├── main.rs                    # CLI: intent-script <input.json> [--config-dir] [--pretty]
│   ├── lib.rs                     # Public API: compile(json, config_dir) → CompileOutput
│   ├── error.rs                   # CompileError enum (thiserror)
│   ├── output.rs                  # CompileOutput, UnsignedTx, Eip712IntentOutput, JSON serialization
│   ├── eip712.rs                  # EIP-712 domain separator, struct hashing (matches Solidity)
│   ├── schema/
│   │   ├── mod.rs
│   │   └── public_ast.rs          # Serde types: IntentScript, Step enum, SwapStep, etc.
│   ├── ir/
│   │   ├── mod.rs
│   │   └── canonical.rs           # ResolvedIntent, ResolvedStep enum, ConcreteCall
│   ├── registry/
│   │   ├── mod.rs
│   │   └── loader.rs              # RegistryContext: loads chains.json, assets/, protocols/
│   ├── compiler/
│   │   ├── mod.rs                 # Top-level compile() pipeline
│   │   ├── normalize.rs           # Stage B: AST → canonical IR
│   │   ├── validate.rs            # Stage C: IR validation (minimal)
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
│       └── oneinch.rs             # Calldata passthrough
├── tests/
│   ├── integration.rs             # 22 compiler integration tests
│   ├── generate_calldata.rs       # 9 fixture generators for Foundry tests
│   └── generate_eip712_fixtures.rs # 6 EIP-712 batch fixture generators
└── examples/
    ├── aave_borrow.json           # deposit USDC + borrow DAI
    ├── aave_deposit.json          # deposit USDC into Aave
    ├── aave_withdraw.json         # withdraw USDC from Aave
    ├── complex_defi.json          # swap USDC→WETH + deposit WETH + borrow DAI
    ├── stake_lido.json            # stake ETH in Lido
    ├── stake_lido_wsteth.json     # stake ETH + wrap stETH→wstETH
    ├── swap_1inch.json            # swap via 1inch (needs calldata)
    ├── swap_uniswap.json          # swap USDC→WETH via Uniswap
    └── wrap_eth.json              # wrap ETH→WETH
```

---

## Solidity Router

```
contracts/
├── src/
│   ├── IntentRouter.sol           # Main router: executeDirect, executeSigned, sweep
│   └── interfaces/
│       ├── IERC20.sol
│       └── IWETH.sol
└── test/
    ├── IntentRouter.t.sol         # Unit tests (mock-based)
    ├── IntentRouterCalldata.t.sol  # Calldata verification tests
    ├── IntentForkE2E.t.sol        # 7 fork E2E tests against mainnet
    └── IntentForkTests.t.sol      # Legacy fork tests
```

### IntentRouter Contract
- `executeDirect(Call[] calls, address[] tokensToSweep)` — user submits directly
- `executeSigned(IntentBatch batch, bytes signature)` — relayer submits with EIP-712 sig
- `_sweep(tokens, recipient)` — transfers all token balances back to user
- `_refundETH(recipient)` — returns any remaining ETH
- EIP-712 replay protection via nonces and deadlines

---

## Key Types

### ResolvedStep (IR)
```rust
enum ResolvedStep {
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
}
```

### CompileOutput
```rust
enum CompileOutput {
    SingleTx(UnsignedTx),
    Eip712Intent(Eip712IntentOutput),  // batched + EIP-712 typed data
    TxSequence(Vec<UnsignedTx>),
    RequiresExecutor { reason: String },
}
```

---

## Config Files

```
config/
├── chains.json              # ethereum, sepolia, base, arbitrum
├── assets/
│   └── ethereum.json        # ETH, WETH, USDC, USDT, DAI, WBTC, stETH, wstETH
└── protocols/
    └── ethereum.json         # aave (pool), uniswap (router, quoter), lido (steth, wsteth), 1inch (router), intent_router (router)
```

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

### Key Insight: Aave V3 Borrow Token Flow
Aave V3's `borrow()` sends borrowed tokens to `msg.sender` (the router), not `onBehalfOf` (the user). The enricher adds the borrowed asset to `tokens_to_sweep` so the router sweeps it back.

---

## Test Infrastructure

### Rust Tests
| Test File | Count | What |
|-----------|-------|------|
| `integration.rs` | 22 | Compiler pipeline end-to-end |
| `generate_calldata.rs` | 9 | Generate fixture files for Foundry |
| `generate_eip712_fixtures.rs` | 6 | Generate EIP-712 batch fixtures |
| Unit tests (various) | 11 | EIP-712 hashing, amount parsing, etc. |

### Foundry Tests
| Test File | Count | What |
|-----------|-------|------|
| `IntentForkE2E.t.sol` | 7 | Fork E2E against mainnet (wrap, swap, deposit, borrow, stake, complex DeFi) |
| `IntentRouter.t.sol` | — | Unit tests with mocks |
| `IntentRouterCalldata.t.sol` | — | Calldata verification |

### Running Tests
```bash
make test              # All Rust tests
make test-foundry      # Foundry unit tests
make generate-fixtures # Regenerate calldata + EIP-712 fixtures
make test-fork-e2e     # Fork E2E tests (requires ETH_RPC_URL)
make test-e2e          # Everything including fork tests
```

---

## Resolved Issues

### 1. Missing `transferFrom` in Batched Calldata
**Issue**: Router couldn't pull tokens from user.
**Fix**: Enricher now inserts `transferFrom(user, router, amount)` for tokens not already in the router.

### 2. Aave V3 Borrow Credit Delegation
**Issue**: `borrow(onBehalfOf=user)` reverts when `msg.sender=router` because no credit delegation.
**Fix**: (a) Tests add `approveDelegation` prerequisite. (b) Enricher adds borrowed asset to `tokens_to_sweep` since Aave sends borrowed tokens to `msg.sender` (router).

---

## Dependencies

```toml
# Rust (crates/intent-script/Cargo.toml)
alloy-primitives = "1"
alloy-sol-types = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
clap = { version = "4", features = ["derive"] }
hex = "0.4"

# Solidity (contracts/foundry.toml)
forge-std (via git submodule)
```
