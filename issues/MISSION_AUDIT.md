# intentOS issue audit — advisor-onchain pivot (2026-04-25)

## Why this exists

The product mission shifted from "intent-execution chat with a safety compiler" to **"onchain AI financial advisor"** — proactive portfolio scan, opinionated allocation recommendations with reasoning, watch-and-nudge over time, owned MetaMorpho vaults + paired credit module as the financial primitive, Safe-based smart-account chassis with custom modules and scoped agent delegation, three audiences (retail / treasury+DAO / agent devs).

Several existing `WS-*` issues were scoped pre-pivot and need to be rescoped, replaced, or supplemented with new issues. This file is the audit. **Nothing in this file has been applied to the issue MD files yet** — it is a proposal awaiting approval.

The user's hard constraint: **the existing reactive chat path stays**. The advisor surface adds proactivity on top.

## Already shipped (per `git log` on each repo)

| ID | Status | Notes |
|----|--------|-------|
| WS-0A | ✅ shipped | `intentOS-server` scaffolded (axum, healthz, config, Dockerfile, docker-compose) |
| WS-1A | ✅ shipped | OpenAPI 3.1 spec locked; UI typed client + verify:api script |
| WS-1B | ✅ shipped | SIWE login + API-key store + bearer middleware (server) and settings panel + SIWE sign-in (UI) |
| WS-1C | ✅ shipped | `/api/v1/compile` wired to `intent-script` crate |
| WS-1D | ✅ shipped | `/api/v1/simulate` via `eth_simulateV1` |
| WS-2A | ✅ shipped | `apiKey` removed from `/api/chat` body |
| WS-2B | ✅ shipped | Direct browser→OpenAI BYOK transport |
| WS-3A-SERVER | ✅ shipped | GitHub Actions CI for the Rust server |

The remaining 25 issues are the audit target.

## Verdict legend

- **KEEP** — scope still right under new mission, no edits needed.
- **RESCOPE** — keep the issue, edit the body to align with advisor mission.
- **REPLACE** — close the existing draft, file a new one in its place.
- **REMOVE** — delete; new mission makes it irrelevant.
- **NEW** — add a brand-new issue not in current set.

## Verdicts on existing 25 unshipped issues

| ID | Verdict | One-line reason |
|----|---------|-----------------|
| WS-1E (chat upgrade) | **RESCOPE** | Becomes "advisor surface upgrade": chat is the advisor entry point, not just a follower. Wire portfolio scan + recommendation cards into the chat thread. |
| WS-3A-IS (IS CI) | **KEEP** | Pure infra. |
| WS-3A-UI (UI CI) | **KEEP** | Pure infra. |
| WS-3B (anvil/MM UX) | **KEEP** | Local-dev DX. |
| WS-3C (full-stack e2e) | **RESCOPE** | Add advisor-flow scenarios: portfolio scan → recommendation card → one-sig execution → watch alert. Today the issue only covers reactive intent → execute. |
| WS-3D (compiler regression) | **RESCOPE** | Add coverage for new first-class step: MetaMorpho vault deposit/withdraw and credit-module drawdown. Existing protocol coverage stays. |
| WS-4A (capability gallery) | **RESCOPE** | Pivot from "what intents we support" to **strategy gallery** — what the advisor can recommend (curated MetaMorpho vaults, leveraged stETH, Aave looping, etc.), surfaced as recommendation chips inside the chat. |
| WS-4B (example prompts) | **RESCOPE** | Pivot from "type these prompts" to **portfolio scenarios** for first-time users without a wallet — "what would the advisor say if your portfolio looked like X?" Static demo personas. |
| WS-4C (first-run walkthrough) | **RESCOPE** | Walkthrough is now: connect wallet → advisor scans → first proactive recommendation → manual ChatGPT fallback if no API key. Less feature-tour, more advisor-first-touch. |
| WS-4D (settings polish) | **RESCOPE** | Add advisor preference fields: risk tolerance band, asset whitelists/blacklists, alert cadence, auto-rebalance opt-in. |
| WS-4E (recommendation cards) | **KEEP, ELEVATE** | Already advisor-shaped. Move from "polish" tier to core. Acceptance criteria stand. |
| WS-4F (transaction review/risk panel) | **KEEP, ELEVATE** | Already advisor-shaped. Add one section: surface vault management fee + credit-spread mechanics so users see how the product earns. |
| WS-5A (landing page) | **RESCOPE** | Full content rewrite: "AI financial advisor onchain" not "intent compiler chat". Three-audience tabs (retail / treasury / devs). Routing change (`/` → marketing, `/app` → advisor) stays. |
| WS-5B (fumadocs scaffold) | **KEEP** | Doc infra is mission-neutral. |
| WS-5C (API reference) | **RESCOPE** | Once the API spec gains advisor endpoints (see NEW issues below), add them to the auto-generated reference. |
| WS-5D (quickstart cookbook) | **RESCOPE** | Cookbook examples shift from "compile a swap" to "build a yield agent that uses /recommendations + /execute". Existing compile/simulate examples stay as foundations. |
| WS-6A (input validation) | **KEEP** | Security middleware scope-neutral. |
| WS-6B (rate limiting) | **KEEP** | Security middleware scope-neutral. |
| WS-6C (CORS/CSP) | **KEEP** | Security middleware scope-neutral. |
| WS-6D (audit logging) | **RESCOPE** | Expand audit log scope to include advisor recommendations and Safe-module actions, not just `/compile`/`/simulate` calls. Material for accounting export later. |
| WS-6E (security review) | **RESCOPE** | Expand checklist to cover: Safe modules, MetaMorpho vault contracts, credit module, agent-delegation permission scoping. Existing API/compiler review items stay. |
| WS-7A (LLM intent eval) | **RESCOPE** | Existing eval covers "did the LLM emit valid `intent-script`". Expand (or split — see NEW issues) to also evaluate **advisor reasoning quality**: did it identify a real opportunity? did it size sensibly? did its yield/risk numbers match reality? |
| WS-7B (live quote/aux data) | **RESCOPE** | Expand from "quote helper for a swap" to **advisor data feeds**: vault APYs, market utilization, oracle drift, position health. This is what feeds the proactive scan. |
| WS-7C (portfolio context) | **KEEP** | Already aligned — UI has a portfolio overlay; this issue hardens it. Becomes prerequisite for the advisor scan engine. |
| WS-7D (executor/permit paths) | **RESCOPE, ELEVATE** | Becomes the **Safe smart-account chassis** issue: signer integration, custom modules (vault deposits, credit drawdowns, intent execution, agent delegation), scoped permissions. Existing executor/permit work folds in. This is no longer a "polish" wrap-up — it's the chassis the advisor commits onto. |

**Net for existing issues:** 0 removed, 14 rescoped, 11 kept (4 of which elevated in priority). No replacements.

## New issues to add (NEW)

Mission-bearing gaps not covered by any existing draft. Each gets its own `WS-*.md` draft, reviewed before `gh issue create` (per the standing review rule).

| Proposed ID | Title | Repo(s) | Why it's new |
|-------------|-------|---------|--------------|
| WS-8A | Advisor scan engine — proactive portfolio analysis | UI + SERVER | The core "advisor reads your wallet and opens with a recommendation" loop. UI today is reactive only. |
| WS-8B | Watch / alerts / nudges service | SERVER + UI | "Yields shift, utilization spikes, oracle prices drift → useful nudge with one-sig rebalance attached." No infra for this today. |
| WS-8C | Advisor reasoning eval harness | UI (or SERVER) | Companion to WS-7A: judges the *quality* of advice (correct opportunity, sensible sizing, accurate yield numbers), not just whether the intent compiles. |
| WS-9A | MetaMorpho vault deposit/withdraw as first-class compile step | IS | `intent-script` has Morpho Blue *market* deposit but no MetaMorpho *vault* step. Required before the advisor can recommend the owned primitive. |
| WS-9B | Owned MetaMorpho vaults — deployment + curation | IS (contracts) + SERVER (metadata) | The owned financial primitive itself. Curated allocator config, fee config (10–50 bps), vault metadata API. |
| WS-9C | Credit module — productized working-capital line | IS + SERVER | Pairs with owned vaults: collateralized Morpho/Aave borrow surfaced as a single "credit line" product, not a generic borrow flow. |
| WS-10A | Safe smart-account chassis — signer + base modules | IS (contracts) + UI | Wire Safe signer in UI, add Safe-aware path through `IntentRouter`, register the four modules (vault, credit, intent, agent-delegation). Today UI just checks a Safe flag. |
| WS-10B | Agent delegation — scoped permissions inside the smart account | IS + SERVER + UI | Third-party agents (yield, liq protection, tax-loss harvest) as first-class participants with constraint-checked permissions. Differentiator vs. competitors. |
| WS-11A | Treasury / DAO surface | UI + SERVER | Multi-sig recovery, role-based perms, accounting & audit export. Targets the second audience and unlocks the credit-module use case at scale. |
| WS-11B | Manual ChatGPT prompt flow polish | UI | Component already exists (`chatgpt-flow.tsx`); productize the no-API-key path so it surfaces context-rich prompts that match the advisor's recommendations. Mission says this is the on-ramp for users who don't share a key. |

**Net new:** 10 issues spread across IS / UI / SERVER. Five of them (WS-9A/B/C, WS-10A/B) are mostly contract/Rust work — biggest engineering footprint.

## Suggested updates to `SEQUENTIAL_ORDER.md`

Phase B/C/D stay (they shipped). Insert two new phases between current Phase E (compiler/data foundations) and Phase F (security middleware):

```
Phase E.5 — Mission primitives
  • WS-9A — MetaMorpho vault as first-class compile step
  • WS-9B — Owned MetaMorpho vault deployment
  • WS-10A — Safe smart-account chassis
  • WS-9C — Credit module
  • WS-10B — Agent delegation

Phase E.6 — Advisor surface
  • WS-7C — Portfolio context (existing, prereq)
  • WS-7B — Live data feeds (rescoped)
  • WS-8A — Advisor scan engine
  • WS-4E — Recommendation cards (existing, elevated)
  • WS-4F — Transaction review + risk panel (existing, elevated)
  • WS-8B — Watch / alerts
  • WS-11B — Manual ChatGPT flow polish
  • WS-1E — Chat-as-advisor surface (rescoped)
  • WS-7A — Intent eval (existing, rescoped)
  • WS-8C — Advisor reasoning eval
```

Phase F (security middleware) and Phase G (docs) stay. Phase H (UX polish) and Phase I (security review + executor) stay, except WS-7D moves into Phase E.5 as WS-10A.

## What I need from you to proceed

Before I edit any of the 14 RESCOPE issue files or write the 10 NEW issue drafts, confirm:

1. The NEW issue IDs I picked (`WS-8*` advisor surface, `WS-9*` financial primitives, `WS-10*` chassis, `WS-11*` org/manual surface) match how you want to slot them. Happy to renumber.
2. WS-7D rescope-into-WS-10A is correct — i.e. you want chassis work expanded beyond just executor/permit/unsupported-output paths.
3. Treasury/DAO surface (WS-11A) belongs in this batch vs. a later push. It's a real audience but slower-burning than the retail advisor.
4. The owned MetaMorpho vault deployment (WS-9B) is in scope for *this* set of issues, given it has a contract footprint and probably a separate deploy story.

After your nod, I'll:
- Rewrite the 14 RESCOPE issue bodies in place.
- Draft the 10 NEW `WS-*.md` files.
- Update `INDEX.md` and `SEQUENTIAL_ORDER.md`.
- Hold off on `gh issue create` until you re-review the rescopes (per the standing review rule).
