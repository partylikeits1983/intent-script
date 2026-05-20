# [WS-7A] LLM intent-generation eval harness

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/llm`, `area/testing`, `area/compiler`, `size/L`
**Depends on:** WS-3D

## Context

Measures whether the LLM reliably emits compiler-valid, safe, executable intents from realistic user prompts in the **reactive chat** path. The advisor's reasoning quality (does it pick the right opportunity? size sensibly? cite real numbers?) is evaluated separately by WS-8C, which sits on top of this. Keep this harness tight on intent emission so it can run cheaply and frequently.

## Scope

1. Golden prompt corpus (`evals/intent-generation/*.jsonl`):
   - Direct actions: swap, deposit, borrow, repay/withdraw, send.
   - Multi-step: swap→deposit, wrap→deposit→borrow, LP decrease→collect, stake→wrap.
   - Advanced: long, short, close-position, bridge, Morpho Blue market actions, Lido claim.
   - Ambiguous: should ask a clarifying question.
   - Unsafe: should refuse or require explicit confirmation.
   - Few-shot/example-ablation cases that measure prompt-example impact on accuracy.
2. Eval runner:
   - Inject deterministic wallet, balances, prices, timestamp, positions context.
   - Call the configured model with `finalize_intent` tool schema.
   - Capture tool calls or clarifying text.
   - Compile every emitted intent through the WASM compiler.
   - Simulate executable outputs against an anvil fork when feasible.
3. Scoring:
   - Correct mode (tool call vs clarifying question vs Q&A/refusal).
   - Schema validity.
   - Compiler success.
   - Semantic match: assets/protocols/amounts.
   - No invented wallet/position/price data.
   - No forbidden fields or unsupported protocol approximations.
4. Regression thresholds:
   - 95% schema-valid.
   - 90% compiler-valid.
   - 0 critical safety failures in the blocking subset.
   - Every previous production bug locked as a regression case.
5. Prompt-size and latency metrics: input tokens, output tokens, first-token latency, total latency, compile/simulate latency after model output.
6. Redacted artifacts stored: prompt, runtime context, model output, compiler/simulation result, score, failure reason.
7. **Out of scope** here: judging the *quality* of advice. That's WS-8C.

## Files

- `intentOS-ui/evals/intent-generation/*.jsonl` (new)
- `intentOS-ui/evals/run-intent-evals.ts` (new)
- `intentOS-ui/evals/scorers.ts` (new)
- `intentOS-ui/evals/README.md` (new)
- `intentOS-ui/package.json` — `eval:intents`
- `intentOS-ui/lib/system-prompt.md`
- `intentOS-ui/lib/intent-tool-schema.ts`

## Acceptance criteria

- [ ] `pnpm eval:intents` runs the corpus locally and emits a pass/fail report.
- [ ] Runner supports offline fixture mode and live-model mode.
- [ ] CI runs the offline scorer on fixture outputs; live-model evals on a scheduled/manual workflow.
- [ ] Failed eval points to exact prompt, expected behavior, actual output, and compiler/simulation error.
- [ ] Suite reports whether extra examples improved or hurt the small-model accuracy and latency.
- [ ] Prompt additions require eval evidence.
- [ ] Release checklist blocks v1.0 if critical safety evals fail.
