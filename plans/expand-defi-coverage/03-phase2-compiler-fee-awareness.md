# Sub-Task 03 — Phase 2: Compiler Fee Awareness

## Context

After sub-task 02, the router charges a fee at sweep time. If the compiler is unaware of this, any step using `"amount": "all"` downstream of a producing step will overestimate by the fee — causing runtime reverts when the router has less balance than the compiler expected.

This sub-task threads a config-loaded `fee_bps` through the registry, into the resolved IR, and into `step_produces`. No contract changes. Pure Rust.

## Prerequisites

- Sub-task 02 complete (router has fee mechanism).
- `config/protocols/{ethereum,anvil,sepolia}.json` exist.

## Files to read first

- `crates/intent-script/src/registry/loader.rs` — understand `ProtocolConfig` and how protocols load.
- `crates/intent-script/src/ir/canonical.rs` — full file; focus on `ResolvedIntent` struct (~line 14), `step_produces` (~lines 201-227), `step_consumes`.
- `crates/intent-script/src/compiler/normalize.rs` — search for `ResolvedIntent {` (construction site) and `resolve_amount_or_all` (the "all" resolver).
- `crates/intent-script/tests/` — grep `step_produces` to find all call sites.

## Implementation

### 3.1 Config schema extension

In `config/protocols/{ethereum,anvil,sepolia}.json`, add to the `intent_router` entry:

```json
"intent_router": {
  "type": "router", "version": "v1",
  "contracts": { "router": "0x…" },
  "fee_bps": 10
}
```

Note: sepolia doesn't have `intent_router` today; either add it (with `fee_bps: 0` and a placeholder address) or guard the loader against absence.

### 3.2 Registry loader

Edit `crates/intent-script/src/registry/loader.rs`:

- Add `pub fee_bps: Option<u16>` to `ProtocolConfig` (`#[serde(default)]`, defaults to `None` → treated as 0).
- Add `RegistryContext::fee_bps(&self) -> u16`:
  ```rust
  pub fn fee_bps(&self) -> u16 {
      self.protocols.get("intent_router")
          .and_then(|p| p.fee_bps)
          .unwrap_or(0)
  }
  ```

### 3.3 Thread into IR

In `crates/intent-script/src/ir/canonical.rs`, add `pub fee_bps: u16` to `ResolvedIntent`. Populate at the construction site in `normalize.rs` via `registry.fee_bps()`.

### 3.4 Fee-aware `step_produces`

Change signature:
```rust
pub fn step_produces(step: &ResolvedStep, fee_bps: u16) -> Option<(Address, U256)>
```

For every producing variant, apply the fee:
```rust
let reduced = amount * U256::from(10_000u64 - fee_bps as u64) / U256::from(10_000u64);
Some((token, reduced))
```

(Guard: `fee_bps <= 10_000` — registry loader should reject larger values; add a debug assert.)

Call-site updates:
- `compiler/normalize.rs::resolve_amount_or_all` — thread `fee_bps` from `ResolvedIntent` through.
- Every test that calls `step_produces` directly.
- `compiler/enrich.rs` if it references `step_produces`.

**Edge case (document in code comment):** tokens that get repaid inside a flashloan (sub-task 06) are NOT swept; fee does not apply. Accept slight over-conservatism in `"all"` downstream of flashloans for v1 — it undercounts by <1%, never reverts.

### 3.5 Tests

Create `crates/intent-script/tests/fee_aware_produce.rs`:

- `test_fee_aware_all_deposit_reduces_by_10bps`:
  - Script with `fee_bps=10`: swap yielding `min_amount_out=1000`, followed by deposit `"all"`.
  - Assert the deposit's resolved amount is `1000 * 9990 / 10_000 = 999`.
- `test_fee_aware_zero_fee_no_reduction`: same script with `fee_bps=0` → deposit amount is 1000.
- `test_fee_aware_does_not_affect_explicit_amount`: deposit `"500"` (explicit) is still 500, not `500 * 9990 / 10_000`.

## Definition of done

- [ ] `cargo test -p intent-script` passes.
- [ ] `cargo test -p intent-script --test fee_aware_produce` passes.
- [ ] `step_produces` has a second `fee_bps: u16` parameter everywhere.
- [ ] `RegistryContext::fee_bps()` returns 10 for ethereum, 0 for sepolia (if sepolia stays without the field).
- [ ] Existing tests in `tests/` still green after call-site updates.

## Verification

```bash
cd /Users/riemann/Desktop/intentOS/intent-script
make test
```

## Hand-off to sub-task 04

- Every future `step_produces` implementation (Morpho, LP, etc.) must accept and apply `fee_bps`.
- `ResolvedIntent` now carries `fee_bps`. Pass it into any new helper that computes "all" amounts.
- Explicit amount strings bypass the fee discount (fee is only a correction for chained "all").
