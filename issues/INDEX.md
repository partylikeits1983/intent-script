# intentOS Issue Index

North star: [PRODUCT.md](PRODUCT.md). When an issue conflicts with the product doc, the product doc wins.

**Repo legend:** `[UI]` = `partylikeits1983/intentOS-ui`, `[IS]` = `partylikeits1983/intent-script`, `[SERVER]` = `partylikeits1983/intentOS-server`.

Agents implementing these issues should first read [AGENT_IMPLEMENTATION_GUIDE.md](AGENT_IMPLEMENTATION_GUIDE.md). A single agent working alone should follow [SEQUENTIAL_ORDER.md](SEQUENTIAL_ORDER.md).

## MVP scope (this sprint)

The advisor surface and the few infra pieces it needs.

| ID | Title | Repo | Depends on | Phase |
|----|-------|------|------------|-------|
| [WS-7C](WS-7C-portfolio-position-context.md) | Portfolio and position context hardening | UI | 3B | MVP |
| [WS-7B](WS-7B-live-quote-and-aux-data.md) | Live quote and advisor data feeds | SERVER/UI | 1A | MVP |
| [WS-8A](WS-8A-advisor-scan-engine.md) | Advisor scan engine — proactive portfolio analysis | UI/SERVER | 7B, 7C | MVP |
| [WS-4E](WS-4E-strategy-recommendation-cards.md) | Recommendation card UI | UI | 4A, 7B, 7C, 8A | MVP |
| [WS-4F](WS-4F-transaction-review-risk-panel.md) | Transaction review and risk panel | UI | 7B, 7C | MVP |
| [WS-11B](WS-11B-manual-chatgpt-flow-polish.md) | Manual ChatGPT prompt flow polish | UI | 8A | MVP |
| [WS-8C](WS-8C-advisor-reasoning-eval.md) | Advisor reasoning eval harness | UI | 7A, 8A | MVP |

## Foundations (parallel — small but blocking quality)

| ID | Title | Repo | Depends on |
|----|-------|------|------------|
| [WS-3A-IS](WS-3A-IS-ci-pipeline.md) | CI pipeline (GHA) — compiler | IS | — |
| [WS-3A-UI](WS-3A-UI-ci-pipeline.md) | CI pipeline (GHA) — UI | UI | — |
| [WS-3B](WS-3B-anvil-metamask-ux.md) | Anvil/MetaMask chain-ID UX fixes | UI | — |
| [WS-3C](WS-3C-full-stack-e2e.md) | Full-stack Playwright e2e | UI | 3B |
| [WS-3D](WS-3D-compiler-test-expansion.md) | Compiler regression coverage for complex DeFi | IS | — |
| [WS-7A](WS-7A-llm-intent-eval-harness.md) | LLM intent-generation eval harness | UI | 3D |

## UX, docs, marketing

| ID | Title | Repo | Depends on |
|----|-------|------|------------|
| [WS-4A](WS-4A-capability-gallery.md) | Strategy gallery — what the advisor can recommend | UI | — |
| [WS-4B](WS-4B-example-prompts.md) | Demo portfolio scenarios for visitors without a wallet | UI | 8A |
| [WS-4C](WS-4C-first-run-walkthrough.md) | First-run walkthrough — connect → scan → first recommendation | UI | 4A, 8A, 11B |
| [WS-4D](WS-4D-settings-polish.md) | Settings — advisor preferences + execution controls | UI | — |
| [WS-5A](WS-5A-landing-page.md) | Landing at `/` — "AI financial advisor onchain" | UI | 4B |
| [WS-5B](WS-5B-fumadocs-scaffold.md) | Fumadocs scaffold + core concepts | UI | — |
| [WS-5C](WS-5C-api-reference.md) | API reference from OpenAPI | UI | 1A, 5B, 8A |
| [WS-5D](WS-5D-quickstart-cookbook.md) | Developer quickstart + cookbook | UI | 1C, 1D, 5B, 8A |

## API hardening

| ID | Title | Repo | Depends on |
|----|-------|------|------------|
| [WS-6A](WS-6A-input-validation.md) | Runtime input validation on all `/api/v1/*` | SERVER/UI | 1A |
| [WS-6B](WS-6B-rate-limiting.md) | Rate limiting middleware | SERVER/UI | 1B |
| [WS-6C](WS-6C-cors-csp.md) | CORS allowlist + CSP headers | SERVER/UI | — |
| [WS-6D](WS-6D-audit-logging.md) | Audit log + abuse signal | SERVER/UI | 1B |
| [WS-6E](WS-6E-security-review.md) | Security review pass | SERVER/UI/IS | WS-1, WS-2, WS-6, WS-8 |

## v1 (post-MVP, next 1–3 months)

| ID | Title | Repo | Depends on | Phase |
|----|-------|------|------------|-------|
| [WS-8B](WS-8B-watch-alerts-nudges.md) | Watch / alerts / nudges | SERVER/UI | 7B, 8A | v1 |

## Shipped (closed on GitHub)

| ID | Title | Shipped in |
|----|-------|------------|
| [WS-0A](WS-0A-rust-api-service-architecture.md) | Rust API/executor service architecture | `intentOS-server@aeb28f5` |
| [WS-1A](WS-1A-api-spec.md) | API spec + cross-language schemas | `f04dc20` (server), `b0d9b4b` (UI) |
| [WS-1B](WS-1B-agent-api-keys.md) | Agent API-key issuance | `53050ce` (server), `d8cfdc0` (UI) |
| [WS-1C](WS-1C-compile-endpoint.md) | Rust `/api/v1/compile` endpoint | `d1aa3d2` |
| [WS-1D](WS-1D-simulate-endpoint.md) | Rust `/api/v1/simulate` endpoint | `523be04` |
| [WS-1E](WS-1E-chat-upgrade.md) | `/api/v1/chat` upgrade | (rolled into WS-2A/2B) |
| [WS-2A](WS-2A-remove-apikey-body.md) | Stop accepting `apiKey` in request body | `3bdd891` |
| [WS-2B](WS-2B-direct-browser-byok.md) | Direct browser→OpenAI for BYOK | `5164254` |
| [WS-3A-SERVER](WS-3A-SERVER-ci-pipeline.md) | CI pipeline (GHA) — Rust server | `a7d226e` |

## Deferred to v2/v3 (closed on GitHub on 2026-04-25 per `PRODUCT.md`)

These features remain on the long-term roadmap but are not currently tracked as open issues. When their phase comes up, re-file fresh issues against the codebase as it exists then.

| ID | Title | Phase per PRODUCT.md |
|----|-------|----------------------|
| WS-7D | Safe smart-account chassis | v2 (3–9 months) |
| WS-9A | MetaMorpho vault as first-class compile step | v2 |
| WS-9B | Owned MetaMorpho vault deployment + curation | v2 |
| WS-9C | Credit module — productized working-capital line | v3 (9–18 months) |
| WS-10A | Agent delegation — scoped permissions | v3 |
| WS-11A | Treasury / DAO surface | v2 |

---

**MVP critical path:** 7C → 7B → 8A → 4E/4F → 11B → 8C. Foundations (3A-IS / 3A-UI / 3B / 3C / 3D / 7A) run in parallel. UX (4A/4B/4C/4D) and docs (5A/5B/5C/5D) come once advisor ships. API hardening (6A–E) gates a v1.0 tag.
