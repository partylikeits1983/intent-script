# [WS-3D] Compiler regression coverage for complex DeFi

**Repo:** `partylikeits1983/intent-script`
**Labels:** `area/testing`, `area/compiler`, `area/defi`, `size/L`
**Depends on:** none

## Context

The compiler is the safety boundary for LLM output. Existing tests cover Aave V3, Uni V3, Lido, Morpho Blue, Balancer, Across. The advisor (WS-8A) leans on every supported step, so any gaps in the regression matrix become advisor bugs in production. This issue brings coverage up to the level of confidence the advisor demands.

## Scope

1. Extend existing test suites (do not add another broad integration dump):
   - `preview_tests.rs` — fee-aware previews for LP, Lido withdrawal, bridge, long/short, close-position.
   - `planner_mode_tests.rs` — router-required flows, native-value batches, `requires_executor` boundaries, direct-vs-signed execution mode selection.
   - `protocol_negative_tests.rs` — unsupported Morpho leverage, invalid Across destination/fee, Lido claim hint mismatches, invalid LP price ranges, self-swaps, stale deadlines.
   - `adversarial_intents_tests.rs` — recipient pinning, malicious calldata passthrough, over-broad approvals, unsafe sweep/fee cases.
2. Golden compiler fixtures for at least these user-facing flows:
   - Swap → Aave deposit (advisor's typical "park USDC at yield" recommendation).
   - Aave deposit → borrow.
   - Levered ETH long and close-position.
   - Uni V3 LP mint, increase, decrease+collect.
   - Lido stake, request withdrawal, claim withdrawal.
   - Across bridge.
   - Morpho Blue supply / borrow / withdraw against a configured market.
3. Each golden fixture asserts:
   - Output type (`single_tx`, `tx_sequence`, `eip712_intent`, `requires_executor`).
   - Expected call targets/selectors.
   - Prerequisite approvals.
   - Preview inputs/outputs.
   - Stable structured error code for rejected cases.
4. Fork-level execution coverage:
   - At least one Foundry/anvil fork test per protocol family.
   - No "compiled only" pass for flows whose calldata could revert because of protocol state.
5. Keep test helpers shared and typed. Avoid brittle string-only assertions when error types expose structured fields.

## Files

- `intent-script/crates/intent-script/tests/preview_tests.rs`
- `intent-script/crates/intent-script/tests/planner_mode_tests.rs`
- `intent-script/crates/intent-script/tests/protocol_negative_tests.rs`
- `intent-script/crates/intent-script/tests/adversarial_intents_tests.rs`
- `intent-script/crates/intent-script/tests/integration.rs`
- `intent-script/contracts/test/IntentFork*.t.sol`

## Acceptance criteria

- [ ] `cargo test -p intent-script` covers every compiler-supported step at least once in a success case and at least once in a relevant failure case.
- [ ] Every complex DeFi flow listed above has a golden fixture with output-type, target/selector, approval, and preview assertions.
- [ ] Fork tests execute Aave V3, Uni V3 swap/LP, Lido, Morpho Blue, Balancer flashloan leverage, and Across where a practical fork assertion exists.
- [ ] Structured compiler errors are asserted by code/stage/path where available.
