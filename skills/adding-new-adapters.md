# Adding New Protocol Adapters

> **Load this file** when you need to add support for a new DeFi protocol or action. This is a step-by-step recipe with exact file paths and code patterns.

## The 8-Step Recipe

Every new protocol/action follows the same pattern. Here are the files to touch, in order:

| Step | File | What |
|------|------|------|
| 1 | `crates/intent-script/src/schema/public_ast.rs` | Add step variant to `Step` enum + parameter struct |
| 2 | `crates/intent-script/src/ir/canonical.rs` | Add `ResolvedStep` variant with concrete types |
| 3 | `crates/intent-script/src/compiler/normalize.rs` | Resolve aliases → addresses, parse amounts → U256 |
| 4 | `crates/intent-script/src/compiler/validate.rs` | Add validation rules for the new step |
| 5 | `crates/intent-script/src/compiler/enrich.rs` | Insert auto-generated steps (approve, transferFrom, sweeps) |
| 6 | `crates/intent-script/src/adapters/{protocol}.rs` | ABI-encode the calldata |
| 7 | `crates/intent-script/src/adapters/mod.rs` | Register the adapter dispatch |
| 8 | `config/protocols/ethereum.json` | Add protocol contract addresses |

Then write tests:
- Integration test in `crates/intent-script/tests/integration.rs`
- Fixture generator in `crates/intent-script/tests/generate_calldata.rs`
- Example JSON in `crates/intent-script/examples/`

---

## Step 1: Add to Public AST

In `crates/intent-script/src/schema/public_ast.rs`:

```rust
// Add variant to Step enum (line ~70)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    // ... existing variants ...
    MyAction(MyActionStep),    // ← add here
}

// Add the parameter struct
#[derive(Debug, Deserialize)]
pub struct MyActionStep {
    pub asset: String,         // token alias
    pub amount: String,        // human-readable amount
    pub into: String,          // protocol name
    // ... other fields as needed
}
```

**Key rules:**
- Use `#[serde(rename_all = "snake_case")]` on the enum — variant `MyAction` maps to JSON key `"my_action"`
- All fields are strings (aliases, human amounts) — resolution happens in normalize
- Optional fields use `#[serde(default)]` and `Option<String>`

## Step 2: Add to Canonical IR

In `crates/intent-script/src/ir/canonical.rs`:

```rust
// Add variant to ResolvedStep enum (line ~44)
pub enum ResolvedStep {
    // ... existing variants ...
    MyProtocolAction {
        contract: Address,     // resolved contract address
        asset: Address,        // resolved token address
        amount: U256,          // parsed amount with correct decimals
        on_behalf_of: Address, // typically the signer
    },
}
```

**Key rules:**
- All addresses are `alloy_primitives::Address`
- All amounts are `alloy_primitives::U256` (already scaled to token decimals)
- No strings — everything is resolved

**Also update `step_consumes()` and `step_produces()`** if the step consumes or produces tokens that other steps might reference:

```rust
// In step_consumes() — if this step takes tokens as input
pub fn step_consumes(step: &ResolvedStep) -> Option<(Address, U256)> {
    match step {
        // ... existing arms ...
        ResolvedStep::MyProtocolAction { asset, amount, .. } => Some((*asset, *amount)),
        _ => None,
    }
}

// In step_produces() — if this step guarantees token output
pub fn step_produces(step: &ResolvedStep) -> Option<(Address, U256)> {
    match step {
        // ... existing arms ...
        // Only add if the step has a guaranteed minimum output
        _ => None,
    }
}
```

## Step 3: Add Normalization

In `crates/intent-script/src/compiler/normalize.rs`, add a match arm in the step normalization section:

```rust
Step::MyAction(s) => {
    // 1. Resolve the protocol contract address
    let contract = registry.protocol_contract("my_protocol", "pool")?;
    
    // 2. Resolve the asset address and decimals
    let asset = registry.asset_address(&s.asset)?;
    let decimals = registry.asset_decimals(&s.asset)?;
    
    // 3. Parse the amount (handles "all" keyword too)
    let amount = resolve_amount_or_all(&s.amount, decimals, asset, &steps)?;
    
    // 4. Produce the resolved step
    ResolvedStep::MyProtocolAction {
        contract,
        asset,
        amount,
        on_behalf_of: signer,
    }
}
```

**Key functions available:**
- `registry.asset_address(alias)` → `Result<Address>`
- `registry.asset_decimals(alias)` → `Result<u8>`
- `registry.protocol_contract(protocol, contract_name)` → `Result<Address>`
- `parse_amount(amount_str, decimals)` → `Result<U256>`
- `resolve_amount_or_all(amount_str, decimals, token, previous_steps)` → `Result<U256>`

## Step 4: Add Validation

In `crates/intent-script/src/compiler/validate.rs`:

```rust
// In validate_amount() — add zero-amount check
ResolvedStep::MyProtocolAction { amount, .. } => {
    if *amount == U256::ZERO {
        return Err(CompileError::InvalidAmount("MyAction amount must be > 0".into()));
    }
}

// In validate_asset_compatibility() — add protocol-specific checks
ResolvedStep::MyProtocolAction { asset, .. } => {
    // Example: reject native ETH for protocols that need ERC-20
    if *asset == Address::ZERO {
        return Err(CompileError::Validation(
            "Cannot use native ETH with MyProtocol. Wrap to WETH first.".into()
        ));
    }
}
```

## Step 5: Add Enrichment

In `crates/intent-script/src/compiler/enrich.rs`, add a match arm in the enrichment loop:

```rust
ResolvedStep::MyProtocolAction { contract, asset, amount, on_behalf_of } => {
    // If batching through router and token not already in router:
    if is_batching && !tokens_in_router.contains(asset) {
        // Pull tokens from user into router
        enriched_steps.push(ResolvedStep::Erc20TransferFrom {
            token: *asset,
            from: signer,
            to: router,
            amount: *amount,
        });
        tokens_in_router.insert(*asset);
    }
    
    // Approve the protocol to spend tokens (router approves protocol)
    if is_batching {
        enriched_steps.push(ResolvedStep::Erc20Approve {
            token: *asset,
            spender: *contract,
            amount: *amount,
        });
    }
    
    // Push the actual step
    enriched_steps.push(step.clone());
    
    // If the step produces output tokens that stay in the router, track them:
    // tokens_to_sweep.push(output_token);
}
```

**Enrichment patterns by protocol type:**

| Pattern | When | Auto-inserted steps |
|---------|------|---------------------|
| **ERC-20 input** | Step takes ERC-20 tokens | `transferFrom` (if not in router) + `approve` |
| **ETH input** | Step takes native ETH | Nothing (ETH sent as `msg.value`) |
| **Output stays in router** | Swap output, borrowed tokens | Add to `tokens_to_sweep` |
| **No approval needed** | ETH staking, unwrap | Just push the step |

## Step 6: Create the Adapter

Create `crates/intent-script/src/adapters/{protocol}.rs`:

```rust
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::SolCall;

use crate::error::{CompileError, Result};
use crate::ir::canonical::{ConcreteCall, ResolvedStep};

// Define the Solidity function signature
alloy_sol_types::sol! {
    function myFunction(address asset, uint256 amount, address onBehalfOf, uint16 referralCode)
        external returns (uint256);
}

pub fn lower_my_action(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    let ResolvedStep::MyProtocolAction {
        contract, asset, amount, on_behalf_of,
    } = step else {
        return Err(CompileError::Adapter("Expected MyProtocolAction step".into()));
    };

    let calldata = myFunctionCall {
        asset: *asset,
        amount: *amount,
        onBehalfOf: *on_behalf_of,
        referralCode: 0,
    }
    .abi_encode();

    Ok(vec![ConcreteCall {
        to: *contract,
        calldata: Bytes::from(calldata),
        value: U256::ZERO,  // or amount for ETH-value calls
        description: format!("MyAction {} wei of {} via MyProtocol", amount, asset),
    }])
}
```

**Key patterns:**
- Use `alloy_sol_types::sol!` to define the function signature
- Use `.abi_encode()` to produce calldata
- Return `Vec<ConcreteCall>` (usually 1 call, but can be multiple)
- Set `value` to the ETH amount for payable functions (e.g., `lido.submit{value}()`)

## Step 7: Register the Adapter

In `crates/intent-script/src/adapters/mod.rs`:

```rust
pub mod my_protocol;  // ← add module declaration

// In the lower_step() dispatcher function:
pub fn lower_step(step: &ResolvedStep) -> Result<Vec<ConcreteCall>> {
    match step {
        // ... existing arms ...
        ResolvedStep::MyProtocolAction { .. } => my_protocol::lower_my_action(step),
    }
}
```

## Step 8: Add Protocol Config

In `config/protocols/ethereum.json`:

```json
{
  "my_protocol": {
    "type": "lending",
    "version": "v1",
    "contracts": {
      "pool": "0x1234567890abcdef1234567890abcdef12345678"
    }
  }
}
```

If the protocol uses new tokens, also add them to `config/assets/ethereum.json`:

```json
{
  "MY_TOKEN": {
    "address": "0xabcdef1234567890abcdef1234567890abcdef12",
    "decimals": 18
  }
}
```

---

## Existing Adapters as Reference

| Adapter | File | Complexity | Good reference for |
|---------|------|------------|-------------------|
| `wrap.rs` | `crates/intent-script/src/adapters/wrap.rs` | Simple | ETH-value calls, single function |
| `erc20.rs` | `crates/intent-script/src/adapters/erc20.rs` | Simple | Multiple functions in one adapter |
| `aave_v3.rs` | `crates/intent-script/src/adapters/aave_v3.rs` | Medium | Supply/borrow/withdraw pattern |
| `uniswap_v3.rs` | `crates/intent-script/src/adapters/uniswap_v3.rs` | Medium | Struct parameters, complex ABI |
| `lido.rs` | `crates/intent-script/src/adapters/lido.rs` | Medium | ETH staking + token wrapping |
| `oneinch.rs` | `crates/intent-script/src/adapters/oneinch.rs` | Simple | Calldata passthrough pattern |
| `send.rs` | `crates/intent-script/src/adapters/send.rs` | Medium | ERC-20/ETH/ERC-721 transfers |

---

## Testing Checklist

After implementing all 8 steps:

1. **Integration test** in `crates/intent-script/tests/integration.rs`:
   ```rust
   #[test]
   fn test_my_action_compiles() {
       let json = r#"{
           "network": "ethereum",
           "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
           "steps": [
               { "my_action": { "asset": "USDC", "amount": "1000", "into": "my_protocol" } }
           ]
       }"#;
       let result = compile_test(json);
       assert!(result.is_ok());
   }
   ```

2. **Fixture generator** in `crates/intent-script/tests/generate_calldata.rs` (if Foundry tests needed)

3. **Example JSON** in `crates/intent-script/examples/my_action.json`

4. Run `cargo test -p intent-script` — all tests must pass

5. If Foundry tests exist, run `cd contracts && forge test`

---

## Appendix: Patterns from existing adapters

### NFT custody — the `recipient=signer` shortcut

For steps that mint NFTs (Uniswap V3 LP `mint`, potentially Morpho/Lido NFTs), set `recipient = signer` in the normalized step so the NFT bypasses the router entirely. The router supports `onERC721Received` (so safeTransferFrom inbound works), but keeping custody out of the router removes a class of stuck-NFT bugs. Example: `adapters/uniswap_v3_lp.rs::lower_lp_mint`.

### Recursive enrich for flashloan-style steps

Steps that embed a sub-pipeline of `ResolvedStep` (e.g. `BalancerFlashloan { inner_steps: Vec<ResolvedStep> }`) must NOT store pre-lowered `ConcreteCall[]` — enrich needs the chance to walk inner steps and auto-insert approvals/transferFroms just like the outer pass. The shared `enrich_steps(&mut ...)` helper in `compiler/enrich.rs` is reusable: seed `tokens_in_router` with whatever the outer machinery pre-delivers (flashloan proceeds, upstream swap outputs) and let the same logic flow. Merge inner `required_pulls` into outer so the builder emits prerequisite approvals for user-contributed tokens. Model in `adapters/balancer.rs` + `compiler/enrich.rs::BalancerFlashloan`.

### Config-keyed markets (Morpho Blue)

For protocols with many markets addressed by id, store a `markets: Option<HashMap<String, MarketConfig>>` on `ProtocolConfig` and require the DSL to reference a market by alias (e.g. `"USDC-WETH-86"`). The `id` (`keccak256(abi.encode(MarketParams))`) is **pre-computed at config-authoring time** and stored alongside the constituent fields — keeps the compiler deterministic and offline. Adapters reconstruct the struct for calldata but can also reference the id directly when Morpho's ABI takes it. See `registry::MorphoMarketConfig`, `adapters/morpho.rs`.

### User-prerequisite NFT approval (Uniswap V3 LP decrease/collect)

Steps that operate on an existing NFT held by the signer (decrease liquidity, collect fees) require the user to have already called `NPM.approve(router, tokenId)` out-of-band. Do not synthesize an approval call inside the compiler — the caller's wallet must do this as a prerequisite tx. Document this in the JSON spec and surface a clear error message. Pattern: `adapters/uniswap_v3_lp.rs` + `skills/json-dsl-spec.md` `### lp_decrease`.

### Leverage sugar composing the flashloan primitive

High-level sugar (`long`/`short`/`close_position`) should desugar into existing IR rather than introducing new IR variants. The `compiler/leverage.rs` expander returns `ResolvedStep::BalancerFlashloan { inner_steps: vec![ AaveV3Supply, AaveV3Borrow, UniswapV3Swap ] }` — zero new IR, zero new adapter code. Validate/enrich/lower paths for the primitive handle the composition automatically. When the user contributes equity alongside the flashloan, pre-insert an explicit `Erc20TransferFrom { from: signer, to: router, amount: user_contribution }` as the first inner step; that satisfies the flashloan repayability validator (see `step_produces` special case for transferFrom) and registers `required_pulls` via the enricher's pre-existing-transferFrom match arm.
