# [WS-1D] Rust `/api/v1/simulate` endpoint

**Repo:** `partylikeits1983/intentOS-server`
**Labels:** `area/api`, `area/simulation`, `area/rust`, `size/M`
**Depends on:** WS-0A, WS-1A, WS-1B

## Context

Agents want to know what a compiled intent will do before submitting it on-chain. The UI can continue simulating locally when connected to a wallet, but the public agent API should expose simulation from the Rust service.

## Scope

1. Implement `POST /api/v1/simulate` in Axum:
   - Auth: `require_api_key`.
   - Body: `{ compiledOutput, from, network, stateOverrides? }`.
   - Response: `{ success, gasUsed, balancesBefore, balancesAfter, positionsBefore, positionsAfter, assetDeltas, errors? }`.
2. Use Rust EVM/RPC tooling:
   - `alloy` provider for RPC calls, gas estimates, receipts, and chain metadata;
   - `eth_call`/state override support where the RPC supports it;
   - optional `revm` fork/backend only if it materially improves deterministic simulation.
3. Network config:
   - mainnet/L2 RPC URLs from env (`RPC_URL_<CHAIN>`);
   - anvil fork URL from env for local/staging;
   - shared chain/protocol config from `intent-script/config`.
4. Return typed simulation diagnostics:
   - decoded revert reason when possible;
   - target, selector, tx index, and fix instruction;
   - stable `SIMULATION_ERROR` envelope.
5. Keep UI simulation available:
   - browser path can still use current `lib/simulate-transaction.ts`;
   - Rust endpoint is for agents/server-side workflows.

## Files

- `intentOS-server/src/routes/simulate.rs` (new)
- `intentOS-server/src/simulation.rs` (new)
- `intentOS-server/src/evm.rs` (new)
- `intentOS-server/.env.example` — add `RPC_URL_MAINNET`, etc.
- `intentOS-ui/lib/simulate-transaction.ts` (reference; do not remove browser path)

## Acceptance criteria

- [ ] `curl` against a known compiled intent returns non-zero gas and realistic asset deltas.
- [ ] Simulation against an intent that reverts returns `SIMULATION_ERROR` with decoded revert details when available.
- [ ] Against anvil fork, gas/balance numbers match a follow-up real transaction within 2% where deterministic comparison is possible.
- [ ] Network mismatch returns 400 with a clear message.
- [ ] UI can still simulate through the browser path without calling the Rust service.
