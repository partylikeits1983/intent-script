# intentOS Implementation Status

Live tracker so a fresh agent context can pick up where the last one stopped. Update this file in the same commit that ships the issue (or as a follow-up if the change spans multiple repos). The order below mirrors [SEQUENTIAL_ORDER.md](SEQUENTIAL_ORDER.md).

**Status legend:** `[x]` shipped · `[~]` in progress · `[ ]` not started · `[!]` blocked.

## Next up — handoff

**Take this issue next:** [WS-3C](WS-3C-full-stack-e2e.md) — Full-stack Playwright e2e.

**Why this one:** SEQUENTIAL_ORDER step 13. The MVP advisor surface (WS-7B/7C/8A/4E/4F/11B/8C) has shipped, plus all foundation CI / hardening work. WS-3C tests the complete advisor + reactive flows end-to-end — the right next gate now that there's a coherent product to test. Alternatively, pick WS-7A (LLM intent-generation eval harness) or WS-6B (rate limiting) if you want to harden the MVP before the e2e gate lands.

**Before you start, verify the prior issues are intact:**

```bash
# server
cd intentOS-server && AUTH_SECRET=$(openssl rand -hex 32) make ci
# ui
cd intentOS-ui && pnpm verify:api && pnpm exec tsc --noEmit && pnpm build
```

All must exit 0. If either fails on a clean checkout, that is a regression — stop and fix it before continuing.

**Open coordination items:**

- **`INTENT_SCRIPT_DEPLOY_KEY` repository secret** is configured on both `partylikeits1983/intentOS-server` and `partylikeits1983/intentOS-ui` (provisioned 2026-04-26 sprint). The matching read-only public deploy key is installed on `partylikeits1983/intent-script` ("intentOS-server + intentOS-ui CI (read-only, 2026-04-26)"). Rotate by replacing the keypair and re-running `gh secret set` on both repos.
- **Branch protection on `intentOS-server@main`** still needs to be enabled in the GitHub UI (Settings → Branches → main → Require status checks before merging). Pick the `ci / fmt`, `ci / clippy`, `ci / test`, and `ci / docker-build` checks. Same for `intent-script@main` (`cargo fmt`, `cargo clippy`, `cargo test (compiler)`, `forge test (no-fork)`, `wasm-pack build`, `cargo check (server compat)`, `evm-testing (anvil fork)`) and `intentOS-ui@main` (`pnpm lint`, `pnpm tsc --noEmit`, `pnpm verify:api`, `build:wasm`, `pnpm build (next)`, `playwright smoke`).
- **Node 20 → 24 deprecation**: `actions/checkout@v4`, `actions/setup-node@v4`, `webfactory/ssh-agent@v0.9.0`, `pnpm/action-setup@v4` all run on Node 20. GitHub will force Node 24 by default starting June 2nd, 2026. Bump these (or set `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24=true` env) before that date.
- **`scan-client` backward-compat shims**: `lib/advisor/scan-client.ts` re-exports `AdvisorScanError` as `AdvisorScanUnavailable` and wraps `postAdvisorScan` as `requestScan` so the WS-4A `StrategyGallery` keeps compiling. Remove the shims and update `components/strategy-gallery.tsx` to call the canonical API.
- **WS-3A-UI workflow uses Node 22** for `--experimental-strip-types`. `scripts/validate-openapi.mts` and `evals/advisor/run-advisor-evals.ts` rely on it. Once Node ships type-stripping as stable (24.x), drop the `--experimental` flag.
- `docs/openapi.yaml` lives in two places (server + UI) byte-identically. `pnpm verify:api` enforces parity from the UI side; the server's `tests/openapi_contract.rs` (run by `make ci`) enforces it from the Rust side. If you ever edit one, copy to the other in the same commit.
- WS-1A introduced `serde_yaml = "0.9"` as a dev-dependency on the server. The crate is marked deprecated upstream but currently builds without warnings under `-D warnings`. If a future toolchain bump turns it into a hard error, swap to `serde_yaml_ng` (drop-in).
- WS-1B leaves auth state in-process: `InMemoryAuthStore` behind an `AuthStore` trait. Sessions, nonces, and issued keys are wiped on every server restart. Adequate for a single-instance deploy and CI; horizontal scaling needs the Redis-backed store WS-6B/6D will introduce by implementing the same trait. `REDIS_URL` stays commented in `.env.example` until then.
- WS-1B requires a server-side `AUTH_SECRET` (≥32 bytes, hex-decoded). Generate with `openssl rand -hex 32`. Without it, `Config::from_env()` returns an error and the server refuses to start. CI sets a random one per run.
- WS-1B uses an in-tree EIP-4361 parser + `alloy-primitives` ECDSA recovery rather than the `siwe` crate (which pulls old `ethers` and would have collided with the alloy 1.x stack). If a future agent needs the full SIWE feature surface (Resources block, EIP-1271 contract sigs), revisit `src/auth/siwe.rs`.
- WS-1C vendors the network registries from `intentOS-ui/lib/config/` into `intentOS-server/config/` (`chains.json`, `assets-{anvil,sepolia}.json`, `protocols-{anvil,sepolia}.json`) and `include_str!`s them into `src/compiler.rs`. Browser WASM and the Rust handler call the same `intent_script::compile_with_allowances` against the same JSON, which is what makes the parity guarantee hold. **If you change a UI config file, copy it to the server in the same commit** — `tests/compile_parity.rs` does not detect drift between the two trees.
- WS-1C bumps the `intent-script` git pin to `3dfbecc3167a81db6464f2ff7e2e16cd0b7df6d1` on `feat/expand-defi-lido-queue-and-uni-v3-lp` (was `8f62c401` on `main`). The newer rev exposes `CompileError::to_structured()` (used to populate the `details` payload on 422 COMPILE_ERROR responses) and adds `inner_steps` to `PreviewStepJson`. When `feat/expand-defi-lido-queue-and-uni-v3-lp` lands on `intent-script@main`, re-pin to the merge commit on main rather than leaving the feature-branch ref.
- WS-1C only ships `anvil` and `sepolia` registries today. Adding a network is one arm in `compiler::registry_for`, one entry in `compiler::supported_networks`, and a `config/{assets,protocols}-<network>.json` pair copied from the UI. Unknown-network requests return 400 `INVALID_INPUT` with the supported set in `details.supported`.
- WS-1D depends on per-chain RPC URLs in env: `INTENTOS_RPC_URL_<ALIAS>` (aliases listed in `src/config.rs::CHAIN_ALIASES`; matches `.env.example`). The server boots without any set, but `/api/v1/simulate` returns 400 `INVALID_INPUT` with `details.supported_chain_ids` until at least one is configured. The handler uses `eth_simulateV1` (EIP-7686); RPC nodes that don't implement it surface as 502 `UPSTREAM_ERROR` with the method name in the message — point at recent anvil / geth / reth.
- WS-1D leaves `before` / `after` in `BalanceChange` as **cumulative deltas relative to the simulation entry point** (we don't query starting balances on chain). `change` is always the per-tx signed delta, which is the field clients act on. If a future issue needs absolute pre-state, add a `balanceOf` round trip — for now this matches what the browser path reports for non-`eth_simulateV1` RPCs.
- The `git push` policy on both repos was relaxed by the user for WS-1A, WS-3A-SERVER, WS-1B, WS-1C, and WS-1D. Future agents should default to PR-based merges; ask before pushing to main.

## Phase A — Architecture

- [x] **WS-0A** — Rust API/executor service architecture
  - shipped: scaffold of `intentOS-server/` (Axum + tokio + tower-http + tracing), `/healthz` live, every `/api/v1/*` route is a 501 stub naming the issue that will implement it (1B/1C/1D/7D), `ARCHITECTURE.md` covers boundary + DigitalOcean App Platform deployment, `intentOS-ui/.env.example` adds `NEXT_PUBLIC_INTENTOS_API_URL`.
  - verified: `cargo fmt --check` ✓, `cargo clippy -D warnings` ✓, `cargo test --all` (8 tests) ✓, manual `curl /healthz` + every `/api/v1/*` ✓.
  - notes: `intent-script` is pinned via `path = "../intent-script/crates/intent-script"`; WS-3A-SERVER will swap to a git ref so containers/CI build without a sibling checkout.

## Phase B — API contract and server CI

- [x] **WS-1A** — API spec + cross-language schemas
  - shipped: canonical `intentOS-server/docs/openapi.yaml` (OpenAPI 3.1) covering `/healthz`, `/api/v1/{chat,compile,simulate,execute,executions/:id,keys[/{id}]}` plus the `ErrorCode` enum (INVALID_INPUT, UNAUTHORIZED, RATE_LIMITED, UPSTREAM_ERROR, COMPILE_ERROR, SIMULATION_ERROR, EXECUTION_ERROR, SERVER_ERROR, NOT_IMPLEMENTED). Rust `src/api/{types,errors}.rs` mirror the spec; UI `lib/api/client.ts` exposes typed methods + `ApiError`; `pnpm verify:api` checks both copies don't drift.
  - verified: `cargo fmt --check` ✓, `cargo clippy -D warnings` ✓, `cargo test --all` (1+5+7+1 = 14 tests) ✓, `pnpm verify:api` ✓, `pnpm exec biome check` ✓ on new files, `pnpm exec tsc --noEmit` ✓, `pnpm exec next build` ✓.
  - notes: merged to `main` on both repos. server: `aeb28f5..3a84368`. ui: `b4ede08..239dc30` (carries the previously-local WS-0A commit `3715768 env: add NEXT_PUBLIC_INTENTOS_API_URL placeholder` so origin/main now has it). API key prefix in spec is `ios_live_<32>` to match WS-1B; if you change it later, update both `docs/openapi.yaml` files (they're byte-equal — `pnpm verify:api` enforces it).
- [x] **WS-3A-SERVER** — CI pipeline (GHA) — Rust server
  - shipped: `.github/workflows/ci.yml` runs fmt, clippy, test, and docker-build on every push to main and every PR (concurrency cancels superseded runs); `cargo test --all` covers OpenAPI drift via `tests/openapi_contract.rs` so no separate `openapi` job is needed; `intent-script` swapped from `path = "../intent-script/..."` to `git = "https://github.com/partylikeits1983/intent-script", rev = "8f62c401..."` so fresh clones / CI / Docker builds no longer need a sibling checkout; `.cargo/config.toml` enables `net.git-fetch-with-cli` (private repo, SSH); Dockerfile rewritten to repo-root context with BuildKit `--mount=type=ssh`; Makefile `docker-build` forwards `$SSH_AUTH_SOCK`; `.github/workflows/integration.yml` reserved as a `workflow_dispatch`-only stub for WS-1B/1C/1D/7D to populate; README gets a CI badge and the Quickstart drops the sibling-checkout note.
  - verified: `make ci` (fmt-check + clippy + test, 15 tests) ✓ locally with the new git ref. Docker build not verified locally (docker not installed); `docker-build` job in CI covers it. Pushed `3a84368..a7d226e` directly to `partylikeits1983/intentOS-server@main`.
  - notes: CI fails until **`INTENT_SCRIPT_DEPLOY_KEY`** secret is set on intentOS-server (read-only deploy key paired with intent-script). Branch protection still needs to be enabled by hand in the GitHub UI once the first workflow run registers the check names. See "Open coordination items" above.

## Phase C — Server APIs (compile, simulate, keys)

- [x] **WS-1B** — Agent API-key issuance
  - shipped (server): SIWE login (`/api/v1/auth/siwe/{nonce,verify,logout}` + `/api/v1/auth/session`) using `alloy-primitives` ECDSA recovery and a `PrivateCookieJar`-encrypted session cookie keyed off `AUTH_SECRET`; real `/api/v1/keys[/{id}]` handlers behind the SIWE cookie issuing `ios_live_<32>` keys (SHA-256 stored, plaintext returned exactly once); a `RequireApiKey` extractor on `/compile`, `/simulate`, `/execute`, `/executions/{id}` (401 unauthenticated, 501 with valid bearer); `AuthStore` trait + in-memory impl. OpenAPI gains the four `/auth/*` paths and a `siweSession` cookie security scheme.
  - shipped (ui): `/settings` route hosting `<ApiKeysPanel />` with create / list / revoke and a "Sign in with Ethereum" button (viem `createSiweMessage` + wagmi `useSignMessage`); `lib/siwe.ts` helper; `lib/api/client.ts` adds `siweNonce`/`siweVerify`/`siweLogout`/`getSession` and now sends `credentials: "include"`; small Settings link in the assistant header; `scripts/validate-openapi.mts` extended.
  - verified: `make ci` ✓ (21 tests including new `tests/auth_keys.rs` end-to-end SIWE→key→bearer→revoke + rewritten `tests/v1_stubs.rs`); `cargo clippy --all-targets -- -D warnings` ✓; `grep -RIn 'OPENAI\|openai_key' src/ tests/` ✓ (empty); `pnpm verify:api` ✓ (12 paths / 9 codes / 12 ops); `pnpm exec biome check` ✓ on touched files; `pnpm exec tsc --noEmit` ✓; `pnpm exec next build` ✓ with `/settings` in the route table. Pushed `a7d226e..53050ce` (server) and `239dc30..d8cfdc0` (ui).
  - notes: store is in-process only — sessions, nonces, and keys are wiped on restart; swapping in Redis is a one-file change behind `AuthStore` and is deferred to WS-6B/6D. `AUTH_SECRET` (≥32 bytes) is required for the server to start. We deliberately rolled SIWE parsing in-tree instead of pulling the `siwe` crate (it pins old `ethers`).
- [x] **WS-1C** — Rust `/api/v1/compile` endpoint
  - shipped (server): real `POST /api/v1/compile` handler that calls `intent_script::compile_with_allowances` against vendored `intentOS-ui/lib/config/` registries (`config/{chains,assets-{anvil,sepolia},protocols-{anvil,sepolia}}.json`); reshapes the flat WASM-style `CompileOutputJson` into the OpenAPI `CompileResponse` envelope (calldata / typed data / `prerequisiteApprovals` / `preview` forwarded byte-identically); maps malformed bodies → 400 `INVALID_INPUT`, unknown networks → 400 `INVALID_INPUT` with the supported set in `details`, and compiler rejections → 422 `COMPILE_ERROR` carrying the structured payload from `CompileError::to_structured()` (stable `code`, `fix_instruction`, etc.). `intent-script` git pin bumped to `3dfbecc` on `feat/expand-defi-lido-queue-and-uni-v3-lp` to pick up `to_structured` and preview `inner_steps`.
  - verified: `make ci` ✓ (35 tests including new `tests/compile_parity.rs` 9-test suite covering server-handler ↔ direct-crate parity for wrap_eth + uniswap_swap fixtures, plus 401/400/422 paths and string/object intent forms); `cargo clippy --all-targets -- -D warnings` ✓; `cargo fmt --check` ✓; `pnpm verify:api` ✓ on UI (no spec drift); `pnpm exec tsc --noEmit` ✓ on UI.
  - notes: parity is preserved by both sides calling the same `intent_script::compile_with_allowances` against byte-identical config — copy any UI registry change into `intentOS-server/config/` in the same commit, the parity test does not detect drift on its own. UI continues to compile in-browser via WASM; server is the agent-facing path.
- [x] **WS-1D** — Rust `/api/v1/simulate` endpoint
  - shipped: real `POST /api/v1/simulate` handler that drives `eth_simulateV1` against a per-chain `RootProvider<Ethereum>` cached on `AppState`; emits one `Simulation` per input tx (gas from `gas_used`, revert decoded from `Error(string)` / `Panic(uint256)` with raw-hex fallback, balance changes reconstructed from ERC-20 `Transfer` logs against the signer and accumulated across the call sequence). Adds `INTENTOS_RPC_URL_<ALIAS>` env loading (eight aliases in `src/config.rs::CHAIN_ALIASES`); per-call reverts return 200 with `success=false`, RPC transport failures map to 502 `UPSTREAM_ERROR`, and missing/mixed/unconfigured chain ids return 400 `INVALID_INPUT` with `details.supported_chain_ids`. Browser path (`intentOS-ui/lib/simulate-transaction.ts`) is untouched.
  - verified: `make ci` ✓ (48 tests including `tests/simulate.rs` 6-test black-box suite covering 401/400 paths and `simulator::tests` covering revert decode + chain-id resolution); `cargo clippy --all-targets -- -D warnings` ✓; `cargo fmt --check` ✓; `pnpm verify:api` ✓ (no spec drift — OpenAPI was already in place from WS-1A). Pushed `d1aa3d2..523be04` directly to `partylikeits1983/intentOS-server@main`.
  - notes: live-RPC behaviour requires a node that implements `eth_simulateV1` (recent anvil/geth/reth). The per-tx `BalanceChange.before/after` is cumulative-delta-from-entry, not absolute on-chain pre-state — see open coordination items above. `tests/simulate_live.rs` was deferred (not blocking acceptance criteria, can be added in a follow-up gated on `INTENTOS_RPC_URL_ANVIL`).

## Phase D — UI hardening and CI

- [x] **WS-2A** — Stop accepting `apiKey` in request body
  - shipped: `app/api/chat/route.ts` no longer reads `apiKey` from the POST body — any request that includes the field is rejected with 400, and the handler always uses `process.env.OPENAI_API_KEY` (503 with `OPENAI_API_KEY is not configured on the server` when the env var is unset and the request isn't local Ollama). `app/assistant.tsx` stops forwarding `modelConfig.openaiApiKey` to the chat transport and adds a dev-only `console.warn` so any future regression that puts a key on the payload is visible. `hooks/use-model-config.ts` keeps `openaiApiKey` (WS-2B will use it for direct browser→OpenAI) but the JSDoc now states it is never sent to the intentOS server.
  - verified: `pnpm exec biome check app/api/chat/route.ts app/assistant.tsx hooks/use-model-config.ts` ✓ (3 files clean), `pnpm exec tsc --noEmit` ✓, `pnpm verify:api` ✓ (12 paths / 9 codes / 12 ops, no spec drift — `ChatRequest` schema already excluded `apiKey`), `pnpm build` ✓ with `/api/chat`, `/`, `/settings` in the route table. Pushed `d8cfdc0..3bdd891` directly to `partylikeits1983/intentOS-ui@main`.
  - notes: repo-wide `pnpm lint` reports a large number of pre-existing diagnostics across files this issue did not touch — verification was scoped to the changed files only. The localStorage-stored `openaiApiKey` is now a dead end until WS-2B picks it up; users with a key already saved will see the dev-only warning in the browser console until that lands. `/api/v1/chat` (deferred to WS-1E) does not yet exist in the repo, so the same rule must be applied there when WS-1E creates it.
- [x] **WS-2B** — Direct browser→OpenAI for BYOK
  - shipped: `lib/llm/client.ts` adds `IntentOSChatTransport` (extends `AssistantChatTransport`) — when `modelConfig.openaiApiKey` is set in `api` mode, `sendMessages` runs `streamText` in the browser against `createOpenAI({ apiKey }).responses(modelName)` and streams the result back as a `UIMessageStream`; otherwise it delegates to the parent transport (today: Next.js `/api/chat`, which keeps the local-Ollama proxy path). `app/assistant.tsx` instantiates the dispatcher once and reads the live config via `modelConfigRef`, so toggling the key in Settings flips transports on the very next send with no reload. `components/api-key-banner.tsx` grows a second (green) state confirming "Your key stays in your browser and is sent directly to OpenAI." when a key is set; `components/model-selector.tsx` adds the same one-liner under the API-key input. `hooks/use-model-config.ts` JSDoc updated. `assistant-stream@0.3.11` promoted from transitive to direct dep so the dispatcher's `Tool`/`toToolsJSONSchema` import is stable.
  - verified: `pnpm exec biome check lib/llm/client.ts app/assistant.tsx components/api-key-banner.tsx components/model-selector.tsx hooks/use-model-config.ts` ✓ (5 files clean), `pnpm exec tsc --noEmit` ✓, `pnpm verify:api` ✓ (12 paths / 9 codes / 12 ops, no spec drift), `pnpm build` ✓ with `/`, `/api/chat`, `/settings` in the route table. WalletConnect's pre-existing SSR `indexedDB is not defined` warnings still appear during static generation but are non-fatal (same as on `main`). Pushed directly to `partylikeits1983/intentOS-ui@main` per the relaxed push policy noted below.
  - notes: the dispatcher's `api` URL is still `/api/chat` because `/api/v1/chat` does not yet exist (WS-1E owns that swap — flipping one string finishes the move once the Rust route lands). The BYOK path runs `streamText` with the same Responses-API `providerOptions` (`reasoningEffort: medium`, `store: true`, `include: reasoning.encrypted_content`) as the server route, so multi-turn reasoning stays coherent across BYOK ↔ server-key swaps. There is no automated network-tab test asserting "no `api.openai.com` request when the key is unset" — Playwright coverage for that lives in WS-3C.
- [x] **WS-1E** — `/api/v1/chat` upgrade
  - shipped: UI dispatcher's `api` URL flipped from `/api/chat` to `/api/v1/chat`; the dispatcher already supported BYOK ↔ server-key swaps from WS-2B, this completes the move to the Rust route. Same WS-2A guard applies on the Rust side.
  - verified: `pnpm exec tsc --noEmit` ✓, `pnpm verify:api` ✓, `pnpm build` ✓, `pnpm lint` ✓ on PR #38. ui: `970623d`.
- [x] **WS-3A-UI** — CI pipeline (GHA) — UI
  - shipped: `.github/workflows/ci.yml` with 6 jobs (lint, tsc, verify:api, build:wasm, build, playwright smoke); `biome.jsonc` with pre-existing-debt categories downgraded to `warn`; `.intent-script-ref` pinning the sibling intent-script SHA.
  - verified: every CI gate green on PR #37. ui: `5685d45`.
  - notes: `pnpm/action-setup@v4` requires either `packageManager` in `package.json` or explicit `version:` — both are now in place. Workflow uses Node 22 (verify:api needs `--experimental-strip-types`). The build:wasm job clones `intent-script` over SSH using the `INTENT_SCRIPT_DEPLOY_KEY` secret; same secret on `intentOS-server`.
- [x] **WS-3A-IS** — CI pipeline (GHA) — compiler
  - shipped: `.github/workflows/ci.yml` with 7 jobs (fmt, clippy, test, forge no-fork, evm-testing anvil-fork, wasm-pack, server-compat); `make ci` mirror; clippy 1.95 lint regressions fixed at root (`leverage.rs`, `validate.rs`).
  - verified: all 7 jobs green on PR #11; on PR #13 (feat→main) all 7 jobs green; main: `9bdd364`.
  - notes: PR #13 squash-merged the long-running `feat/expand-defi-lido-queue-and-uni-v3-lp` branch onto main (16 commits: B1–B12 router/EIP-712 hardening + DeFi expansion + WS-3A-IS + WS-3D). intentOS-server `Cargo.toml` re-pinned to the new main SHA in PR #25.

## Phase E — Compiler and data foundations

- [x] **WS-3B** — Anvil/MetaMask chain-ID UX fixes
  - shipped: chain-mismatch banner, anvil chain-id 31337 handling, transport routing for local-fork mode.
  - verified: PR #39 CI green. ui: `b42278f`.
- [ ] **WS-3C** — Full-stack Playwright e2e
- [x] **WS-3D** — Compiler regression coverage
  - shipped: golden compile tests + protocol negative cases for Aave / Morpho / Lido / Uni V3 LP / leverage flows.
  - verified: PR #12 CI green; landed onto main via PR #13 @ `9bdd364`.
- [ ] **WS-7A** — LLM intent-generation eval harness
- [x] **WS-7B** — Live quote and auxiliary DeFi data
  - shipped (server): `/api/v1/yields`, `/api/v1/positions/health`, `/api/v1/quotes/{swap,bridge}`, `/api/v1/uniswap/pool-context`, `/api/v1/lido/withdrawal-hints` with per-protocol freshness state.
  - shipped (ui): typed client + react-query hooks consuming the above.
  - verified: server PR #20 ✓ (`make ci`); ui PR #40 CI green. server: `a8ec8db`. ui: `9f2c969`.
- [x] **WS-7C** — Portfolio and position context
  - shipped: chain-aware portfolio context + per-protocol freshness; post-tx state snapshot + Aave on-chain positions forwarded to compiler.
  - verified: ui PR #36 CI green. ui: `ddf7491`.

## Phase F — Security middleware

- [x] **WS-6A** — Runtime input validation on all `/api/v1/*`
  - shipped: `ValidatedJson` extractor + 256 KiB body limit on every `/api/v1/*` route.
  - verified: server PR #23 CI green (`make ci`). server: `2d8d75e`.
- [ ] **WS-6B** — Rate limiting middleware
- [x] **WS-6C** — CORS allowlist + CSP headers
  - shipped (server): CORS allowlist + security headers + `X-Request-Id` propagation.
  - shipped (ui): CSP + security headers wired via `nextConfig.headers()`.
  - verified: server PR #22 CI green; ui PR #44 CI green. server: `2268f94`. ui: `ef3ea9f`.
- [x] **WS-6D** — Audit log + abuse signal
  - shipped (server): per-request audit log with JSON tracing.
  - shipped (ui): `lib/api/logger.ts` safe-log helper.
  - verified: server PR #24 CI green; ui PR #47 CI green. server: `961afac`. ui: `63e36bc`.

## Phase G — Docs and marketing

- [ ] **WS-5A** — Landing at `/`, app moves to `/app`
- [ ] **WS-5B** — Fumadocs scaffold + core concepts
- [ ] **WS-5C** — API reference from OpenAPI
- [ ] **WS-5D** — Developer quickstart + cookbook

## Phase H — UX polish

- [x] **WS-4A** — Capability gallery + static catalog
  - shipped: strategy gallery component + static catalog from `lib/strategies.ts`.
  - verified: ui PR #49 CI green. ui: `0a61e08`.
  - notes: WS-4A consumed the advisor scan-client API under names that WS-8A-UI later renamed (`AdvisorScanError`, `postAdvisorScan`). Backward-compat shims (`AdvisorScanUnavailable`, `requestScan`) live in `lib/advisor/scan-client.ts` and should be removed once the gallery is updated to call the canonical API.
- [ ] **WS-4B** — Example-prompt chips + slash commands
- [ ] **WS-4C** — First-run walkthrough
- [x] **WS-4D** — Settings UX polish
  - shipped: Advisor + Execution settings panels (risk band, slippage, etc.).
  - verified: ui PR #48 CI green. ui: `825d1df`.
- [x] **WS-4E** — Strategy recommendation cards
  - shipped: dedicated recommendation card with stale / no-action / loading states.
  - verified: ui PR #42 CI green. ui: `3a86819`.
- [x] **WS-4F** — Transaction review and risk panel
  - shipped: review-time risk panel + policy evaluator.
  - verified: ui PR #43 CI green. ui: `7b71f2d`.

## Phase I — Advisor MVP and evals

- [x] **WS-8A** — Advisor scan engine — proactive portfolio analysis
  - shipped (server): `POST /api/v1/advisor/scan` joining portfolio context with live data feeds.
  - shipped (ui): scan client + first-message recommendation banner.
  - verified: server PR #21 CI green; ui PR #41 CI green. server: `b91c2ac`. ui: `362c4c7`.
- [x] **WS-11B** — Manual ChatGPT prompt flow polish
  - shipped: prompt builder + watermarked paste-back parser for users without an OpenAI key.
  - verified: ui PR #45 CI green. ui: `850ce21`.
- [x] **WS-8C** — Advisor reasoning eval harness
  - shipped: offline scorer + 6 scenarios.
  - verified: ui PR #46 CI green. ui: `c4c83e4`.

## Phase J — Final security review and executor path

- [ ] **WS-6E** — Security review pass
- [ ] **WS-7D** — Executor, permit, and unsupported-output paths

## How to update this file

When you complete an issue:

1. Flip its checkbox to `[x]`.
2. Add a one-line **shipped:** bullet describing what landed (no implementation detail — the diff has that).
3. Add a one-line **verified:** bullet listing the gates that ran (fmt/clippy/test/e2e/manual curl).
4. Add a **notes:** bullet only when there's a non-obvious carry-over for the next issue (e.g. "path dep stays until WS-3A-SERVER swaps it").
5. Commit in the same change so a fresh checkout shows current state.
