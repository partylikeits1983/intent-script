# intentOS sprint handoff — 2026-04-26

Drained 21 open PRs across the three repos. Every default branch is green on its latest commit:

```
gh run list --repo partylikeits1983/intent-script  --branch main --limit 1  # ✓
gh run list --repo partylikeits1983/intentOS-server --branch main --limit 1  # ✓
gh run list --repo partylikeits1983/intentOS-ui    --branch main --limit 1  # ✓
```

All shipped issues are flipped to `[x]` in [STATUS.md](STATUS.md). The matching GitHub issues have been closed with merge SHAs.

## Shipped this pass

**intent-script (3 PRs):**
- WS-3A-IS — CI pipeline (GHA) — compiler — PR #11 → feat → main via PR #13 @ `9bdd364`
- WS-3D — Compiler regression coverage — PR #12 → feat → main via PR #13 @ `9bdd364`
- (PR #13) `feat/expand-defi-lido-queue-and-uni-v3-lp` → `main` squash-merge — 16 commits incl. WS-3A-IS, WS-3D, B1–B12 router/EIP-712 hardening, DeFi expansion, 1inch removal — @ `9bdd364`

**intentOS-server (6 PRs):**
- WS-6C — CORS allowlist + security headers + X-Request-Id — PR #22 @ `2268f94`
- WS-6A — ValidatedJson extractor + 256 KiB body limit — PR #23 @ `2d8d75e`
- WS-6D — Per-request audit log + JSON tracing — PR #24 @ `961afac`
- WS-7B — Live data feeds (yields, position health, swap/bridge/pool/lido) — PR #20 @ `a8ec8db`
- WS-8A — Advisor scan engine `POST /api/v1/advisor/scan` — PR #21 @ `b91c2ac`
- (PR #25) Repin `intent-script` git rev to `9bdd364` (post feat→main merge) — @ `4018c23`

**intentOS-ui (14 PRs):**
- WS-3A-UI — CI pipeline (GHA) — UI — PR #37 @ `5685d45`
- WS-1E — `/api/v1/chat` upgrade — PR #38 @ `970623d`
- WS-3B — Anvil/MetaMask chain-ID UX — PR #39 @ `b42278f`
- WS-7C — Portfolio + position context (chain-aware, post-tx state, Aave on-chain positions) — PR #36 @ `ddf7491`
- WS-7B (UI) — Typed client + react-query hooks for live data feeds — PR #40 @ `9f2c969`
- WS-8A (UI) — Advisor scan client + first-message recommendation banner — PR #41 @ `362c4c7`
- WS-4E — Recommendation card with stale / no-action / loading states — PR #42 @ `3a86819`
- WS-4F — Review-time risk panel + policy evaluator — PR #43 @ `7b71f2d`
- WS-6C (UI) — CSP + security headers wired via `nextConfig.headers()` — PR #44 @ `ef3ea9f`
- WS-11B — Manual ChatGPT prompt builder + watermarked paste-back parser — PR #45 @ `850ce21`
- WS-8C — Advisor reasoning eval harness (offline scorer + 6 scenarios) — PR #46 @ `c4c83e4`
- WS-6D (UI) — `lib/api/logger.ts` safe-log helper — PR #47 @ `63e36bc`
- WS-4D — Advisor + Execution settings panels — PR #48 @ `825d1df`
- WS-4A — Strategy gallery + static catalog — PR #49 @ `0a61e08`

**Total: 23 PRs merged** (21 of the original 21 open PRs, plus 2 new — intent-script #13 feat→main and intentOS-server #25 repin).

## Still open as of 2026-04-26

Grouped by phase from [INDEX.md](INDEX.md) and [SEQUENTIAL_ORDER.md](SEQUENTIAL_ORDER.md).

### MVP scope (remaining)

| WS-* | Title | Unblocks on |
|------|-------|-------------|
| WS-7A | LLM intent-generation eval harness | WS-3D (shipped) — ready to start |

### Phase 3 — Full-stack tests (remaining)

| WS-* | Title | Unblocks on |
|------|-------|-------------|
| WS-3C | Full-stack Playwright e2e (UI → anvil fork) | WS-3B (shipped) — ready to start |

### Phase 4 — UX, marketing, docs (remaining)

| WS-* | Title | Unblocks on |
|------|-------|-------------|
| WS-4B | Demo portfolio scenarios for visitors without a wallet | WS-8A (shipped) — ready to start |
| WS-4C | First-run walkthrough — connect → scan → first recommendation | WS-4A, WS-8A, WS-11B (all shipped) — ready to start |
| WS-5A | Landing at `/` — "AI financial advisor onchain" | WS-4B |
| WS-5B | Fumadocs scaffold + core concepts | none — ready to start |
| WS-5C | API reference from OpenAPI | WS-1A (shipped), WS-5B, WS-8A (shipped) |
| WS-5D | Developer quickstart + cookbook | WS-1C/1D (shipped), WS-5B, WS-8A (shipped) |

### Phase 5 — API hardening (remaining)

| WS-* | Title | Unblocks on |
|------|-------|-------------|
| WS-6B | Rate limiting middleware | WS-1B (shipped) — ready to start |
| WS-6E | Security review pass | WS-1, WS-2, WS-6, WS-8 — most shipped; ready once 6B lands |

### Phase 6 — v1 (post-MVP, next 1–3 months)

| WS-* | Title | Unblocks on |
|------|-------|-------------|
| WS-8B | Watch / alerts / nudges | WS-7B (shipped), WS-8A (shipped) — ready to start when v1 cycle opens |

### Out of scope until their phase arrives (per PRODUCT.md v2/v3)

Re-file when their phase arrives: WS-7D (Safe smart-account chassis), WS-9A/9B (MetaMorpho vault), WS-9C (credit module), WS-10A (agent delegation), WS-11A (treasury / DAO surface).

## Critical path to MVP demo

Per [INDEX.md](INDEX.md) "MVP scope" + [SEQUENTIAL_ORDER.md](SEQUENTIAL_ORDER.md) Phase 2 (advisor surface). MVP is advisor-on-existing-compiler only — no vault / Solidity / smart-account work.

```
WS-7C → WS-7B → WS-8A → WS-4E + WS-4F → WS-11B → WS-7A → WS-8C
  ✓        ✓        ✓        ✓     ✓        ✓        ✗        ✓
```

| Step | Status |
|------|--------|
| WS-7C — Portfolio and position context | ✅ shipped (`ddf7491`) |
| WS-7B — Live quote and advisor data feeds | ✅ shipped (server `a8ec8db` + ui `9f2c969`) |
| WS-8A — Advisor scan engine (proactive on connect) | ✅ shipped (server `b91c2ac` + ui `362c4c7`) |
| WS-4E — Recommendation card UI | ✅ shipped (`3a86819`) |
| WS-4F — Transaction review and risk panel | ✅ shipped (`7b71f2d`) |
| WS-11B — Manual ChatGPT prompt flow polish | ✅ shipped (`850ce21`) |
| WS-7A — LLM intent-generation eval harness | ❌ remaining — small. Builds on WS-3D's golden tests. |
| WS-8C — Advisor reasoning eval harness | ✅ shipped (`c4c83e4`) |

Then WS-3C full-stack Playwright e2e to gate any v1.0 tag.

**The advisor MVP itself is functionally complete.** The single remaining MVP-scope ticket (WS-7A) is an eval harness — useful for confidence but not a demo blocker.

## Carry-over coordination items

New / updated this pass:

- **`INTENT_SCRIPT_DEPLOY_KEY` is now provisioned** on both `intentOS-server` and `intentOS-ui`. The matching read-only public deploy key on `intent-script` is titled `intentOS-server + intentOS-ui CI (read-only, 2026-04-26)`. Rotate by replacing the keypair and re-running `gh secret set` on both repos. (No new follow-up issue needed.)
- **Branch protection** still needs to be enabled by hand on all three `main` branches (`intent-script@main`, `intentOS-server@main`, `intentOS-ui@main`). Pick the now-registered check names per repo. See STATUS.md "Open coordination items" for the exact check lists.
- **`scan-client` backward-compat shims** — `lib/advisor/scan-client.ts` exports `AdvisorScanUnavailable` (= `AdvisorScanError`) and `requestScan` (wraps `postAdvisorScan`) so the WS-4A `StrategyGallery` keeps compiling. Remove the shims and migrate `components/strategy-gallery.tsx` to the canonical API. Suggested as a follow-up issue (draft to write before `gh issue create`).
- **Node 20 → 24 deprecation** — every workflow uses `actions/checkout@v4`, `actions/setup-node@v4`, `webfactory/ssh-agent@v0.9.0`, `pnpm/action-setup@v4`, all on Node 20. GitHub will force Node 24 by default June 2nd, 2026. Either upgrade the actions or set `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24=true` env in each workflow. Suggested as a follow-up issue.
- **`--experimental-strip-types` on Node 22** — `intentOS-ui/scripts/validate-openapi.mts` and `intentOS-ui/evals/advisor/run-advisor-evals.ts` use the experimental TypeScript stripping flag. Drop the flag once Node 24.x ships type-stripping as stable.
- **Server intent-script git pin** is now at `9bdd364` (intent-script@main) per WS-1C's open coordination item. The pin can stay on `rev =` (don't switch to `branch = "main"` — non-reproducible).
- **WS-4A → WS-8A-UI API mismatch** — captured in the scan-client shim item above.

Existing items still relevant:

- `intent-script/config/protocols/anvil.json ≡ ethereum.json` byte-for-byte. (Unchanged.)
- `intentOS-server/docs/openapi.yaml ≡ intentOS-ui/docs/openapi.yaml` byte-for-byte. The `pnpm verify:api` script enforces it from the UI side; `tests/openapi_contract.rs` enforces it from the Rust side. Edit both in the same commit.
- Any `intentOS-ui/lib/config/*.json` change is mirrored into `intentOS-server/config/` in the same commit. The `tests/compile_parity.rs` does NOT detect drift on its own.

## Verification (post-cleanup, fresh main checkouts)

```
cd intentOS-server && AUTH_SECRET=$(openssl rand -hex 32) make ci   # ✓ (fmt + clippy + test)
cd intentOS-ui    && pnpm verify:api && pnpm exec tsc --noEmit       # ✓
```

Both ran clean on fresh `main` HEADs.
