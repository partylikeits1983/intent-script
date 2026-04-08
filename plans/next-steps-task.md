# Task: Intent-Script Compiler — Validation, Testing & Balance-Aware Compilation

## Context

You are working on `intent-script`, a Rust compiler that transforms human-friendly JSON DeFi intents into unsigned EVM transactions. The compiler pipeline is: **Parse → Normalize → Validate → Enrich → Lower → Plan → Build**.

Read [`plans/architecture.md`](architecture.md) for the full architecture. All existing tests pass (7/7 fork E2E, 22 integration, 11 unit).

## Objective

Implement three categories of improvements:

1. **Intent chain validation** — detect invalid/impossible step sequences at compile time
2. **Comprehensive testing** — fuzz testing, edge cases, error path coverage
3. **Balance-aware compilation** — accept user's on-chain balances as input so the compiler can infer what's possible

---

## Part 1: Intent Chain Validation

### Problem

The validator ([`crates/intent-script/src/compiler/validate.rs`](../crates/intent-script/src/compiler/validate.rs)) currently only checks:
- Signer is not zero address
- At least one step exists

It does NOT check whether the step sequence is logically valid. Invalid intents compile successfully and only fail at execution time on-chain.

### Requirements

Add validation rules to `validate.rs` (Stage C) that reject invalid intent chains at compile time:

#### Rule 1: Borrow requires prior deposit (or existing collateral)
```json
// INVALID — borrow without deposit
{ "steps": [{ "borrow": { "asset": "DAI", "amount": "1000", "from": "aave" } }] }

// VALID — deposit then borrow
{ "steps": [
  { "deposit": { "asset": "USDC", "amount": "5000", "into": "aave" } },
  { "borrow": { "asset": "DAI", "amount": "1000", "from": "aave" } }
] }
```

The validator should track which protocols have received deposits in the current intent. A borrow step is only valid if:
- There's a prior deposit step into the same protocol in this intent, OR
- The user has existing collateral (see Part 3: balance-aware mode)

#### Rule 2: Withdraw requires prior deposit (or existing position)
Same logic as borrow — can't withdraw from Aave if you haven't deposited.

#### Rule 3: Amount validation
- Amounts must be positive (> 0)
- Amounts must parse as valid decimals
- Amount strings like `"0"`, `"-1"`, `"abc"` should be rejected

#### Rule 4: Asset compatibility
- Can't deposit ETH (native) directly into Aave — must wrap to WETH first
- Can't swap from an asset to the same asset
- Wrap step asset must be native (ETH) or stETH (for wstETH wrapping)

#### Rule 5: Protocol existence
- Deposit/borrow/withdraw protocol must exist in the registry
- Swap via unsupported provider should fail (already works, but add test coverage)

### Implementation Guidance

Expand [`validate.rs`](../crates/intent-script/src/compiler/validate.rs) with a `ValidationContext` that tracks state as it walks through steps:

```rust
struct ValidationContext {
    /// Protocols that have received deposits in this intent
    deposited_protocols: HashSet<String>,
    /// Whether balance-aware mode is active (Part 3)
    user_positions: Option<UserPositions>,
}
```

Add new error variants to [`error.rs`](../crates/intent-script/src/error.rs):
```rust
#[error("Invalid intent chain: {0}")]
InvalidChain(String),
```

### Files to Modify
| File | Change |
|------|--------|
| `crates/intent-script/src/compiler/validate.rs` | Add all validation rules |
| `crates/intent-script/src/error.rs` | Add `InvalidChain` error variant |
| `crates/intent-script/tests/integration.rs` | Add tests for each validation rule |

---

## Part 2: Comprehensive Testing

### 2A: Fuzz Testing for Amount Parsing

The normalizer parses human-readable amounts like `"1.5"` into `U256` with decimal scaling. This is a critical path that should be fuzz-tested.

**File:** `crates/intent-script/src/compiler/normalize.rs` — the `parse_amount` function.

Create a new test file `crates/intent-script/tests/fuzz_amounts.rs`:

```rust
// Test cases to cover:
// - Very large amounts: "999999999999999999999"
// - Very small amounts: "0.000001" (6 decimals for USDC)
// - Amounts with trailing zeros: "1.50000"
// - Amounts with leading zeros: "001.5"
// - Maximum U256 boundary
// - Amounts that would overflow U256 after decimal scaling
// - Empty string
// - Just a dot: "."
// - Multiple dots: "1.2.3"
// - Negative amounts: "-1.5"
// - Scientific notation: "1e18"
// - Whitespace: " 1.5 "
// - Unicode: "１.５"
// - Comma separators: "1,000.50"
```

### 2B: Invalid Input Testing

Add tests to `integration.rs` for every error path:

```rust
// JSON structure errors
- Missing "network" field
- Missing "from" field
- Missing "steps" field
- Empty "from" string
- Invalid "from" (not a hex address)
- Unknown step type: { "fly": { ... } }
- Step with missing required fields: { "deposit": { "asset": "USDC" } } (no amount, no into)

// Protocol errors
- Deposit into unknown protocol: { "deposit": { "into": "compound" } }
- Borrow from unknown protocol
- Swap via unknown provider: { "swap": { "via": "sushiswap" } }

// Asset errors
- Unknown asset: { "wrap": { "asset": "SHIB" } }
- Deposit native ETH into Aave (should fail or auto-wrap)

// Amount errors
- Zero amount: "0"
- Negative amount: "-100"
- Non-numeric: "abc"
- Overflow amount

// Chain validation (from Part 1)
- Borrow without deposit
- Withdraw without deposit
- Swap same asset to itself
```

### 2C: Enricher Edge Case Tests

Test the enricher's token routing logic:

```rust
// Token already in router — no duplicate transferFrom
- Swap USDC→WETH then deposit WETH into Aave (WETH should NOT be transferFrom'd)

// Multiple borrows — each borrowed asset in sweep
- Deposit USDC, borrow DAI, borrow USDT → sweep should include both DAI and USDT

// Borrow-only intent (with balance-aware mode)
- Just borrow DAI (user already has collateral) → should still add DAI to sweep

// Single-step intents don't get router treatment
- Single deposit → should be SingleTx, not batched
- Single wrap → should be SingleTx
```

### 2D: Foundry Fuzz Tests

Add fuzz tests to `contracts/test/IntentRouter.t.sol`:

```solidity
// Fuzz the router with random call arrays
function testFuzz_executeDirect_emptyCallsReverts(uint256 seed) public { ... }

// Fuzz sweep with random token addresses
function testFuzz_sweep_unknownToken(address token) public { ... }

// Fuzz EIP-712 signature verification
function testFuzz_executeSigned_invalidSignature(bytes memory sig) public { ... }
```

### Files to Create/Modify
| File | Change |
|------|--------|
| `crates/intent-script/tests/fuzz_amounts.rs` | New: amount parsing fuzz tests |
| `crates/intent-script/tests/integration.rs` | Add invalid input tests |
| `crates/intent-script/tests/enricher_tests.rs` | New: enricher edge case tests |
| `contracts/test/IntentRouter.t.sol` | Add fuzz tests |

---

## Part 3: Balance-Aware Compilation

### Problem

Currently the compiler has no knowledge of the user's on-chain state. It can't:
- Know if the user already has Aave collateral (so they could borrow without depositing in this intent)
- Know if the user has enough tokens for the intent
- Suggest what's possible given the user's current positions

### Design

Add an optional `balances` field to the JSON input:

```json
{
  "network": "ethereum",
  "from": "0xd8dA...",
  "balances": {
    "tokens": {
      "USDC": "10000.0",
      "WETH": "5.0",
      "DAI": "0"
    },
    "aave_positions": {
      "supplied": {
        "USDC": "50000.0",
        "WETH": "10.0"
      },
      "borrowed": {
        "DAI": "5000.0"
      },
      "health_factor": "1.85"
    }
  },
  "steps": [
    { "borrow": { "asset": "DAI", "amount": "1000", "from": "aave" } }
  ]
}
```

When `balances` is provided:
1. **Validator relaxes rules** — borrow without deposit is OK if user has existing Aave collateral
2. **Validator checks feasibility** — reject if borrow would push health factor below 1.0
3. **Compiler can warn** — "User has 10000 USDC but intent needs 50000 USDC"

### Implementation

#### Step 1: Schema changes

Add to [`schema/public_ast.rs`](../crates/intent-script/src/schema/public_ast.rs):

```rust
#[derive(Debug, Deserialize, Default)]
pub struct IntentScript {
    pub network: String,
    pub from: String,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub nonce: Option<u64>,
    #[serde(default)]
    pub deadline: Option<u64>,
    #[serde(default)]
    pub balances: Option<UserBalances>,
}

#[derive(Debug, Deserialize)]
pub struct UserBalances {
    #[serde(default)]
    pub tokens: HashMap<String, String>,
    #[serde(default)]
    pub aave_positions: Option<AavePositions>,
}

#[derive(Debug, Deserialize)]
pub struct AavePositions {
    #[serde(default)]
    pub supplied: HashMap<String, String>,
    #[serde(default)]
    pub borrowed: HashMap<String, String>,
    #[serde(default)]
    pub health_factor: Option<String>,
}
```

#### Step 2: Normalize balances

In [`normalize.rs`](../crates/intent-script/src/compiler/normalize.rs), parse the balance strings into `U256` values and attach to `ResolvedIntent`:

```rust
pub struct ResolvedIntent {
    // ... existing fields ...
    pub user_balances: Option<ResolvedBalances>,
}

pub struct ResolvedBalances {
    pub tokens: HashMap<Address, U256>,
    pub aave_supplied: HashMap<Address, U256>,
    pub aave_borrowed: HashMap<Address, U256>,
    pub aave_health_factor: Option<f64>,
}
```

#### Step 3: Validator uses balances

In [`validate.rs`](../crates/intent-script/src/compiler/validate.rs):

```rust
// When checking "borrow requires deposit":
if has_borrow_step && !has_prior_deposit {
    if let Some(balances) = &intent.user_balances {
        // User has existing Aave collateral — borrow is OK
        if balances.aave_supplied.values().any(|v| *v > U256::ZERO) {
            // Valid — user has existing collateral
        } else {
            return Err(InvalidChain("Borrow requires collateral..."));
        }
    } else {
        // No balance info — strict mode, reject
        return Err(InvalidChain("Borrow requires prior deposit..."));
    }
}
```

#### Step 4: Compiler warnings

Add a `warnings: Vec<String>` field to `CompileOutput` for non-fatal issues:
- "User token balance (1000 USDC) may be insufficient for intent (5000 USDC)"
- "Borrow of 1000 DAI would reduce health factor from 1.85 to ~1.42"
- "User already has 5000 DAI borrowed — total would be 6000 DAI"

### Files to Create/Modify
| File | Change |
|------|--------|
| `crates/intent-script/src/schema/public_ast.rs` | Add `UserBalances`, `AavePositions` types |
| `crates/intent-script/src/ir/canonical.rs` | Add `ResolvedBalances` to `ResolvedIntent` |
| `crates/intent-script/src/compiler/normalize.rs` | Parse balance strings |
| `crates/intent-script/src/compiler/validate.rs` | Use balances in validation |
| `crates/intent-script/src/output.rs` | Add `warnings` to output |
| `crates/intent-script/tests/integration.rs` | Tests for balance-aware compilation |
| `crates/intent-script/examples/borrow_existing_collateral.json` | Example: borrow with existing position |

---

## Execution Order

1. **Part 1** first — validation rules are the foundation
2. **Part 2** in parallel — tests can be written alongside validation
3. **Part 3** last — builds on the validation framework from Part 1

## Verification

After all changes:
```bash
cargo test --workspace                    # All Rust tests pass
make generate-fixtures                    # Regenerate fixtures
cd contracts && forge test --mc IntentForkE2E --fork-url https://ethereum-rpc.publicnode.com -vvv  # All 7 fork tests pass
```

## Key Constraints

- The `balances` field must be **optional** — the compiler must work without it (current behavior)
- Validation errors should be **clear and actionable** — tell the user what's wrong and how to fix it
- All existing tests must continue to pass — no breaking changes
- The JSON schema must remain **LLM-friendly** — simple, no complex nesting
