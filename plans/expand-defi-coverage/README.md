# Expand DeFi Coverage — Sub-Task Index

This directory splits the monolithic plan in `../expand-defi-coverage.md` into discrete sub-tasks so a fresh agent can execute one phase at a time without loading 1000 lines of unrelated context.

**Read order for a fresh agent:**
1. This `README.md`
2. `00-corrections.md` — errors/imprecisions in the original plan. Read this before trusting anything in the parent plan.
3. Your assigned sub-task file (`NN-*.md`)
4. Only read `../expand-defi-coverage.md` if the sub-task file tells you to.

## Execution order (strict — do not skip)

| # | File | Status | Blocks |
|---|---|---|---|
| 01 | [01-phase0-preflight-config.md](01-phase0-preflight-config.md) | ✅ DONE (2026-04-22) | all subsequent |
| 02 | [02-phase1-router-foundations.md](02-phase1-router-foundations.md) | ✅ DONE (2026-04-22) | 03+ |
| 03 | [03-phase2-compiler-fee-awareness.md](03-phase2-compiler-fee-awareness.md) | ✅ DONE (2026-04-22) | 04+ |
| 04 | [04-phase3-lido-enhancements.md](04-phase3-lido-enhancements.md) | ✅ DONE (2026-04-23) | — (parallel-safe after 03) |
| 05 | [05-phase4-morpho-blue.md](05-phase4-morpho-blue.md) | pending | — (parallel-safe after 03) |
| 06 | [06-phase5-balancer-flashloan-aave-loop.md](06-phase5-balancer-flashloan-aave-loop.md) | pending | 07+ |
| 07 | [07-phase6-uniswap-v3-lp.md](07-phase6-uniswap-v3-lp.md) | ✅ DONE (2026-04-23) | — (parallel-safe after 02) |
| 08 | [08-phase7-across-bridging.md](08-phase7-across-bridging.md) | pending | — (parallel-safe after 03) |
| 09 | [09-phase8-integration.md](09-phase8-integration.md) | pending | final |

Sub-tasks marked *parallel-safe* touch disjoint source files; they can be tackled in any order **after** their dependency. Safer default: run them sequentially 02 → 03 → 04 → 05 → 06 → 07 → 08 → 09.

## Global rules (apply to every sub-task)

1. **Do not add Tornado Cash or Privacy Pools.** Ever.
2. **Every swap/LP/bridge must have slippage protection.** No zero-slippage allowed.
3. **Allowlist is the only thing preventing unsafe targets.** Every new protocol must be added to the deploy-time allowlist.
4. **Library is `no_std` compatible.** No `std::time`, no I/O, no network in the compiler. All config flows in as `&str`.
5. **Compiler is deterministic.** Same input → same bytes. Deadlines use `script.current_timestamp`, never `SystemTime::now()`.
6. **Max 5 outer steps, max 5 inner flashloan steps, max flashloan depth 1.**
7. **ERC20 and ERC721 transfers are already supported by the compiler.** See `00-corrections.md` §1 — do not re-implement. They're available via the `send` step in the DSL.
8. **Update `../../skills/json-dsl-spec.md` only at sub-task 09.** Don't piecemeal-update docs mid-flight.

## How each sub-task file is structured

Every file has these sections so an agent can load it cold:

- **Context** — what this phase achieves and why
- **Prerequisites** — which prior sub-tasks must be complete
- **Files to read first** — concrete paths to understand before editing
- **Implementation** — step-by-step work with paths and line numbers
- **Tests to add** — new test files and names
- **Definition of done** — a checklist, every box must be checkable
- **Verification command** — the single shell invocation that proves it works
- **Hand-off** — what the next agent needs to know
