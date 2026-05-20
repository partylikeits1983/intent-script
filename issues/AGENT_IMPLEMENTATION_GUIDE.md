# intentOS Agent Implementation Guide

This directory contains GitHub issue drafts for production-readiness work across:

- `partylikeits1983/intentOS-ui`
- `partylikeits1983/intent-script`
- `partylikeits1983/intentOS-server` (new Rust service)

Multiple agents may work in parallel. Read this file before starting any issue.

## Architecture Direction

The UI keeps browser-WASM compilation for connected users. Do not move the normal UI compile path behind a server round trip.

The Rust server owns agent-facing and execution-facing APIs:

- `/api/v1/compile`
- `/api/v1/simulate`
- `/api/v1/execute`
- `/api/v1/executions/:id`
- API-key/session endpoints
- quote/helper-data endpoints where server-side credentials, caching, or RPC access are needed

Next.js API routes are limited to UI helpers (chat streaming, SIWE callbacks, BYOK helpers). **Every `/api/v1/*` route is served by the Rust Axum service** — never proxy or re-implement an `/api/v1/*` route inside Next.js, even temporarily. If an agent-facing endpoint is needed, add it to `intentOS-server`.

## Recommended Work Order

Start with:

1. `WS-0A` — Rust API/executor service architecture.
2. `WS-1A` — cross-language API contract.
3. `WS-3A-SERVER` — server CI as soon as the server repo exists.
4. `WS-1B`, `WS-1C`, `WS-1D` — API keys, compile, simulate.
5. `WS-6A`, `WS-6B`, `WS-6D` — validation, rate limiting, audit logging.
6. `WS-7D` — signed EIP-712 relay/executor path.

Parallel early work:

- `WS-3D` — compiler regression coverage.
- `WS-7A` — LLM intent-generation eval harness.
- `WS-7B` / `WS-7C` — quote/helper data and portfolio context, once the API contract boundary is clear.

Second wave:

- UI polish/onboarding/docs/marketing issues: `WS-4*`, `WS-5*`.
- These matter, but should not outrun compile/simulate/execution correctness.

## Parallel Work Rules

- Claim one issue or a narrow slice of one issue before editing.
- Keep write ownership narrow. Avoid touching unrelated files.
- If an issue spans repos, make separate commits per repo where practical.
- Do not rewrite another agent's work. If you encounter conflicting changes, adapt or stop and coordinate.
- Prefer shared contracts over duplicated shapes. API request/response shapes should trace back to `WS-1A`.
- Do not introduce a second compiler path unless the issue calls for it. Browser UI uses WASM; Rust server uses Rust crates directly.

## Before Pushing

Before pushing code, run the relevant checks for the repo you changed.

For `intentOS-ui`:

```bash
pnpm lint
pnpm build
pnpm test:e2e
```

If the change affects only docs/issues and the app build is irrelevant, say that explicitly in the PR/commit notes.

For `intent-script`:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test -p intent-script
cd contracts && forge test
```

Run fork tests when the issue touches fork-only behavior, protocol integration, or router execution.

For `intentOS-server`:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all
```

Also run OpenAPI generation/validation and Docker build once those scripts exist.

If a required check cannot run locally, document exactly why and what remains unverified.

## Commit Attribution

Commit as the repository owner/user, not as Claude, Codex, or any AI-agent identity.

Use the user's configured git identity. Do not add generated-by trailers such as:

- `Co-Authored-By: Claude ...`
- `Co-Authored-By: Codex ...`
- `Generated with ...`

Commit messages should describe the product/code change plainly.

## Quality Bar

Production readiness here means:

- user or agent intent compiles deterministically;
- simulations are realistic and explain failures;
- execution paths are explicit and auditable;
- risky transactions are surfaced before signing;
- LLM behavior is measured by evals, not assumed;
- API boundaries are stable and tested;
- no secrets, private keys, API keys, or raw user prompts leak into logs.

If a change improves polish but weakens one of those guarantees, do not ship it.
