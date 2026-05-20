# intentOS — Sequential Implementation Order

Use this when **one agent** is working through the issues alone. Issues are ordered so that every prerequisite is finished before its dependents start. The north star is [PRODUCT.md](PRODUCT.md): MVP first, v1 features second, deferred work re-filed when its phase arrives.

If you have multiple agents, see [AGENT_IMPLEMENTATION_GUIDE.md](AGENT_IMPLEMENTATION_GUIDE.md) for parallelizable groupings instead.

Mark a step done only when its acceptance criteria pass and CI is green for the affected repo. Don't skip ahead — later items assume earlier ones shipped.

## Already shipped (Phase 0)

WS-0A, WS-1A, WS-1B, WS-1C, WS-1D, WS-1E, WS-2A, WS-2B, WS-3A-SERVER. Closed on GitHub.

## Phase 1 — Foundations (parallelizable, fast)

These are small but everything else assumes they pass.

1. [WS-3A-IS](WS-3A-IS-ci-pipeline.md) — CI pipeline (GHA) — compiler
2. [WS-3A-UI](WS-3A-UI-ci-pipeline.md) — CI pipeline (GHA) — UI
3. [WS-3B](WS-3B-anvil-metamask-ux.md) — Anvil/MetaMask chain-ID UX fixes
4. [WS-3D](WS-3D-compiler-test-expansion.md) — Compiler regression coverage

## Phase 2 — Advisor surface (the MVP)

This is the smallest set of issues that proves the advisor experience on real money. Ship this and the product exists.

5. [WS-7C](WS-7C-portfolio-position-context.md) — Portfolio and position context hardening
6. [WS-7B](WS-7B-live-quote-and-aux-data.md) — Live quote and advisor data feeds
7. [WS-8A](WS-8A-advisor-scan-engine.md) — Advisor scan engine (proactive on connect)
8. [WS-4E](WS-4E-strategy-recommendation-cards.md) — Recommendation card UI (single card, MVP)
9. [WS-4F](WS-4F-transaction-review-risk-panel.md) — Transaction review and risk panel
10. [WS-11B](WS-11B-manual-chatgpt-flow-polish.md) — Manual ChatGPT prompt flow polish (no-API-key onramp)
11. [WS-7A](WS-7A-llm-intent-eval-harness.md) — LLM intent-generation eval harness
12. [WS-8C](WS-8C-advisor-reasoning-eval.md) — Advisor reasoning eval harness

## Phase 3 — Full-stack tests

13. [WS-3C](WS-3C-full-stack-e2e.md) — Full-stack Playwright e2e

## Phase 4 — UX, marketing, docs (post-advisor polish)

14. [WS-4A](WS-4A-capability-gallery.md) — Strategy gallery
15. [WS-4B](WS-4B-example-prompts.md) — Demo portfolio scenarios
16. [WS-4C](WS-4C-first-run-walkthrough.md) — First-run walkthrough
17. [WS-4D](WS-4D-settings-polish.md) — Settings — advisor preferences + execution
18. [WS-5A](WS-5A-landing-page.md) — Landing at `/`
19. [WS-5B](WS-5B-fumadocs-scaffold.md) — Fumadocs scaffold
20. [WS-5C](WS-5C-api-reference.md) — API reference
21. [WS-5D](WS-5D-quickstart-cookbook.md) — Quickstart + cookbook

## Phase 5 — API hardening

22. [WS-6A](WS-6A-input-validation.md) — Input validation
23. [WS-6B](WS-6B-rate-limiting.md) — Rate limiting
24. [WS-6C](WS-6C-cors-csp.md) — CORS allowlist + CSP
25. [WS-6D](WS-6D-audit-logging.md) — Audit log + abuse signal
26. [WS-6E](WS-6E-security-review.md) — Security review pass

## Phase 6 — v1 (post-MVP, next 1–3 months)

27. [WS-8B](WS-8B-watch-alerts-nudges.md) — Watch / alerts / nudges

## Out of scope until their phase arrives

Per [PRODUCT.md](PRODUCT.md), the smart-account chassis (Safe modules), owned MetaMorpho vault + credit module, agent delegation, and treasury/DAO surface are deferred to v2/v3. Re-file fresh issues against the codebase as it exists then.

## Why this order

- **Foundations before features.** CI + anvil-fork DX + compiler regressions land first because every later issue assumes them.
- **MVP advisor as one cohesive arc.** Phase 2 is the user's MVP scope from `PRODUCT.md`: portfolio context → live data → scan → cards → risk panel → manual onramp → evals. Ship this and the product exists.
- **e2e once there's something to test.** WS-3C tests the advisor + reactive flows; doing it before the advisor surface lands wastes work.
- **UX and docs come last in the cycle.** Polishing the empty state, landing page, and developer docs only matters once the advisor surface holds together.
- **API hardening before any v1.0 tag.** WS-6A–E sweep the Rust API + UI before opening it to volume.
- **v1 watch loop separate from MVP.** WS-8B is genuinely valuable but not what we're proving in this sprint.
- **Deferred work stays out of the tracker.** Re-file when its phase arrives instead of carrying stale issues.
