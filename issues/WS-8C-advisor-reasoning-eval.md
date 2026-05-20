# [WS-8C] Advisor reasoning eval harness

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/llm`, `area/testing`, `size/M`
**Depends on:** WS-7A, WS-8A

## Context

WS-7A (the existing intent-generation eval harness) measures whether the LLM emits compiler-valid intents. That's necessary but not sufficient for an advisor product: a recommendation can compile cleanly and still be *bad advice* — wrong opportunity, sized too aggressively, fabricated yield numbers, ignored existing exposure. This issue adds a companion eval that judges advisor *reasoning quality*.

## Scope

1. Build a portfolio-scenario corpus in `evals/advisor/scenarios/*.jsonl`:
   - Idle-stables holder, leverage holder, mixed-DeFi user, fresh wallet, high-utilization Aave borrower, expiring Lido claim, oracle-drift exposure, etc.
   - Each scenario freezes wallet balances, positions, prices, vault APYs, utilization, and timestamp.
2. Eval runner (`evals/advisor/run-advisor-evals.ts`):
   - For each scenario, invoke the WS-8A scan endpoint (with deterministic LLM seed) or run the LLM against the same context.
   - Capture the AdvisorScan envelope.
3. Scoring (`evals/advisor/scorers.ts`):
   - **Opportunity match**: did the recs hit the obvious opportunities the scenario was built around (graded against an expert-written ground-truth)?
   - **Sizing sanity**: do allocations sum to ≤ available balance, leave at least configured liquid reserve, respect risk-tolerance band?
   - **Number fidelity**: every yield/APY/health-factor in the reasoning text matches the underlying data within tolerance.
   - **Exposure awareness**: rec doesn't double up on protocols the user is already heavily in (unless explicitly justified).
   - **Refusal correctness**: scenarios where the right answer is "do nothing" or "ask for risk preference" don't get force-recommended.
4. Regression thresholds:
   - 90% opportunity-match rate on the blocking subset.
   - 0 sizing-sanity failures.
   - 0 fabricated numbers.
   - Locked regression case for every shipped advisor bug.
5. CI integration: offline scorer on fixture outputs runs in CI; live-model evals on a manual workflow (cost control).

## Files

- `intentOS-ui/evals/advisor/scenarios/*.jsonl` (new)
- `intentOS-ui/evals/advisor/run-advisor-evals.ts` (new)
- `intentOS-ui/evals/advisor/scorers.ts` (new)
- `intentOS-ui/evals/advisor/README.md` (new)
- `intentOS-ui/package.json` — add `eval:advisor`

## Acceptance criteria

- [ ] `pnpm eval:advisor` runs the corpus locally and emits a pass/fail report per scenario.
- [ ] A failed scenario points to expected reasoning, actual reasoning, and which scorer rejected it.
- [ ] Number-fidelity check catches any yield/APY/HF in the reasoning that doesn't match scenario context.
- [ ] Sizing-sanity check catches over-allocations, missing-liquidity-reserve, and risk-band violations.
- [ ] CI runs the offline scorer; live-model eval is a separate manual workflow.
- [ ] Adding new advisor instructions to the system prompt requires eval evidence.
