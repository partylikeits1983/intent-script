# Sub-Task 08 — Phase 7: Across Bridging (Single-Sided)

## Context

Add Across V3 as the single supported bridge. Single-sided means this sub-task only emits the **source-chain** call (`depositV3` on the SpokePool). Receiving on the destination chain is a separate intent authored separately.

## Prerequisites

- Sub-task 03 complete (`step_produces` fee-aware — though bridge produces nothing locally; the call-site still needs the arg).

## Files to read first

- `crates/intent-script/src/adapters/uniswap_v3.rs` — pattern for a single-target adapter.
- `config/chains.json` — source for `to_chain` → `chain_id` lookup (must include Arbitrum, Optimism, Base, Polygon entries you want to support).
- Across V3 SpokePool source for `depositV3` signature.

## Implementation

### 8.1 Config

`config/protocols/ethereum.json`:
```json
"across": {
  "type": "bridge", "version": "v3",
  "contracts": { "spoke_pool": "0x5c7BCd6E7De5423a257D81B442095A1a6ced35C5" }
}
```

If you want to bridge from L2s later, add per-chain SpokePool addresses to `arbitrum.json`, `base.json` — but that's beyond v1.

### 8.2 DSL

```json
{ "bridge": { "via": "across",
              "asset": "USDC",
              "amount": "1000",
              "to_chain": "arbitrum",
              "recipient": "0x…",
              "relayer_fee_bps": "5" } }
```

### 8.3 Schema

```rust
pub enum Step { …, Bridge(BridgeStep) }

pub struct BridgeStep {
    pub via: String,
    pub asset: String, pub amount: String,
    pub to_chain: String, pub recipient: String,
    pub relayer_fee_bps: String,
}
```

### 8.4 IR

```rust
pub enum ResolvedStep { …,
    AcrossDepositV3 {
        spoke_pool: Address,
        depositor: Address,
        recipient: Address,
        input_token: Address,
        output_token: Address,        // == input_token for v1 (canonical same-token bridge)
        input_amount: U256,
        output_amount: U256,          // input_amount * (10_000 - relayer_fee_bps) / 10_000
        destination_chain_id: U256,
        exclusive_relayer: Address,   // Address::ZERO (open to any relayer)
        quote_timestamp: u32,
        fill_deadline: u32,
        exclusivity_deadline: u32,
        message: Bytes,               // empty
    },
}
```

### 8.5 Normalize

- `to_chain` → chain_id via `config/chains.json`. Reject if absent.
- `output_amount = input_amount * (10_000 - relayer_fee_bps) / 10_000`.
- `quote_timestamp = script.current_timestamp` (require it in the top-level script; reject if missing).
- `fill_deadline = quote_timestamp + 4 * 3600` (4h).
- `exclusivity_deadline = 0` (no exclusive relayer).
- `output_token = input_token` for v1.
- `depositor = signer`, `recipient = parsed recipient address`.

### 8.6 Validate

- `relayer_fee_bps ≤ 50` (0.5% cap).
- `recipient != Address::ZERO`.
- `to_chain` exists in `chains.json`.
- Reject `asset == "ETH"` (Across expects WETH). Users must compose `wrap` + `bridge` manually.

### 8.7 Enrich

Standard transferFrom + approve(spoke_pool, amount). No sweep (tokens are in flight cross-chain).

### 8.8 Adapter `adapters/across.rs` (NEW)

```rust
alloy_sol_types::sol! {
    function depositV3(
        address depositor, address recipient,
        address inputToken, address outputToken,
        uint256 inputAmount, uint256 outputAmount,
        uint256 destinationChainId, address exclusiveRelayer,
        uint32 quoteTimestamp, uint32 fillDeadline, uint32 exclusivityDeadline,
        bytes message
    ) external payable;
}
```

### 8.9 Dispatch

`adapters/mod.rs` — register one new arm.

### 8.10 Tests

- `tests/integration.rs`: `test_bridge_across_usdc_to_arbitrum`.
- `tests/generate_calldata.rs`: `bridge_usdc_to_arbitrum.txt`.
- `contracts/test/BridgeFork.t.sol` (NEW): fork mainnet, deposit; assert `FundsDeposited` event with expected params.

### 8.11 Allowlist

Add Across SpokePool to deploy allowlist.

### 8.12 Example

`crates/intent-script/examples/bridge_usdc_arbitrum.json`:
```json
{
  "network": "ethereum",
  "from": "0x…",
  "current_timestamp": 1714000000,
  "steps": [
    { "bridge": { "via": "across", "asset": "USDC", "amount": "1000",
                  "to_chain": "arbitrum", "recipient": "0x…",
                  "relayer_fee_bps": "5" } }
  ]
}
```

## Definition of done

- [ ] `AcrossDepositV3` IR variant compiles.
- [ ] Rejects native ETH, requires pre-wrapped WETH.
- [ ] `relayer_fee_bps ≤ 50` enforced.
- [ ] `make test && make test-foundry` green.
- [ ] `BridgeFork.t.sol` passes.

## Verification

```bash
cd /Users/riemann/Desktop/intentOS/intent-script
make test && make test-foundry
ETH_RPC_URL=… cd contracts && forge test --mc BridgeFork --fork-url $ETH_RPC_URL -vvv
```

## Hand-off to sub-task 09

- Across doesn't produce anything locally — `step_produces` returns `None` for `AcrossDepositV3`.
- `step_consumes` returns the input token (so fee math on a preceding `"all"` amount is correct).
