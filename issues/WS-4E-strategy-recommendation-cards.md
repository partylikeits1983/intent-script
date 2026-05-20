# [WS-4E] Recommendation card UI

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/ux`, `area/llm`, `area/defi`, `size/M`
**Depends on:** WS-4A, WS-7B, WS-7C, WS-8A

## Context

The advisor (WS-8A) emits a recommendation; this issue is the card that renders it inside the chat thread. Per `PRODUCT.md`, MVP ships **one recommendation per session** — multi-card "safe / balanced / aggressive" comparison is a v1 follow-on, not MVP scope. Treat that as a deferred feature; this issue ships the single-card path.

## Scope (MVP)

1. `components/recommendation-card.tsx`:
   - One-line summary headline.
   - Rationale ("why this allocation?") with an expandable longer reasoning block.
   - Existing simulation preview (steps, gas, you-send / you-receive).
   - Single CTA `Sign` that hands off to the existing `finalize_intent` flow — compile/simulate/confirm remain the execution gate.
   - Inline source/freshness pills for any APY or yield numbers cited (sourced from WS-7B).
2. Card states:
   - **Loading** — while the scan is running, render a deterministic skeleton.
   - **Stale data** — if APYs / quotes / position context come back stale, render with a cautionary banner and disable Sign until refresh.
   - **No-action** — when the advisor concludes "do nothing for now," render reasoning without a Sign CTA.
3. Reactive chat preserved: cards live alongside normal LLM messages; typing a goal still works.

## v1 follow-on (out of scope here, tracked for later)

Multi-card comparison view (`recommend_strategies` path with 2–3 alternatives — safe/balanced/aggressive). Defer until MVP advisor ships.

## Files

- `intentOS-ui/components/recommendation-card.tsx` (new)
- `intentOS-ui/lib/system-prompt.md` — advisor card-rendering hints
- `intentOS-ui/components/assistant-ui/thread.tsx` — slot for first-message card
- `intentOS-ui/components/finalize-intent-tool.tsx` — handoff path

## Acceptance criteria

- [ ] When WS-8A emits a recommendation, the thread renders a single card with rationale + simulation preview + Sign.
- [ ] APY/yield numbers cited in rationale show source and freshness; stale data disables Sign.
- [ ] "No-action" recommendation renders without a misleading Sign button.
- [ ] Card degrades cleanly when portfolio context is missing — asks for the prerequisite instead of fabricating.
- [ ] Sign hands off to the existing compile/simulate/finalize flow with no shortcut around the safety pipeline.
