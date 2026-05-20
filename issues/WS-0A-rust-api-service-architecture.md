# [WS-0A] Rust API/executor service architecture

**Repo:** `partylikeits1983/intentOS-server` (new) + coordination with `intentOS-ui` and `intent-script`
**Labels:** `area/architecture`, `area/api`, `area/execution`, `size/M`
**Depends on:** none — blocks WS-1A/1B/1C/1D/7D final shape

## Context

The browser UI should keep running the WASM compiler locally. That path is fast, avoids unnecessary server round-trips, and lets normal users compile/simulate/review before signing in the app.

External agents are different: they need a stable server API that can compile intents, simulate outputs, and optionally submit signed EIP-712 batches through an executor/relayer. That backend should be Rust, not a growing set of Next.js API routes.

## Architecture decision

Create a separate Rust service repo: `partylikeits1983/intentOS-server`.

Recommended stack:

- Rust 2024
- `axum` HTTP server
- `tower-http` for CORS/tracing/rate-limit middleware hooks
- `serde` + `utoipa` or `aide` for OpenAPI generation
- `sqlx` or Redis/Upstash for API keys, usage, idempotency, and request records
- `alloy` for EVM RPC, EIP-712, transaction submission, receipts, and chain primitives
- direct dependency on `intent-script` crates for compilation, not Node WASM

## Deployment decision

Primary recommendation for v1: run the Rust service on a long-lived container platform such as DigitalOcean App Platform/Droplet, Fly.io, Render, Railway, or AWS ECS.

Reasoning:

- executor/relayer work benefits from long-lived workers, queues, retries, and receipt polling;
- fee-taking execution needs careful nonce management and secure signing infrastructure;
- RPC latency and private key handling are easier to reason about in a normal Rust service;
- background jobs and idempotency are less awkward than in serverless functions.

Vercel hosts the Next.js UI only; it is **not** a target for the server. The executor path needs long-lived workers, signer state, and queues that don't fit Vercel's request lifecycle, and we don't want a second deploy story for "simple" Rust handlers — every `/api/v1/*` endpoint runs on the same Axum service. Use Vercel for the UI; use DigitalOcean App Platform (or one of the listed alternatives) for the entire Rust API.

## Scope

1. Scaffold `intentOS-server`:
   - `crates/api` or single `src/main.rs` Axum service;
   - health endpoint;
   - config/env loader;
   - structured logging/tracing;
   - Dockerfile;
   - local `docker compose` with Redis/Postgres if selected.
2. Define service boundaries:
   - UI continues to compile through browser WASM for connected users.
   - Agents call Rust API for compile/simulate.
   - Agents may execute themselves or submit signed EIP-712 payloads to the Rust executor endpoint.
   - Next.js API routes should be limited to UI-only chat/BYOK/SIWE helpers unless there is a clear reason to proxy.
3. Decide deployment target for v1:
   - default: DigitalOcean App Platform or Droplet;
   - acceptable alternatives: Fly.io/Render/Railway/AWS ECS;
   - Vercel is **not** a target for any Rust handler — the entire `/api/v1/*` surface lives on one Axum service.
4. Define repo integration:
   - pin `intent-script` as a git dependency/submodule/workspace member;
   - share OpenAPI contract with `intentOS-ui/docs/openapi.yaml`;
   - publish generated TS client for the UI/docs if needed.

## Files

- `intentOS-server/` (new repo)
- `intentOS-server/Cargo.toml`
- `intentOS-server/src/main.rs`
- `intentOS-server/src/config.rs`
- `intentOS-server/src/routes/*`
- `intentOS-server/Dockerfile`
- `intentOS-server/docker-compose.yml`
- `intentOS-ui/.env.example` — add `NEXT_PUBLIC_INTENTOS_API_URL`

## Acceptance criteria

- [x] `intentOS-server` boots locally with `cargo run` and exposes `/healthz`.
- [x] A written architecture note explains which endpoints live in Rust vs Next.js (`intentOS-server/ARCHITECTURE.md`).
- [x] UI still compiles intents locally through browser WASM.
- [x] Agent-facing compile/simulate/executor endpoints are assigned to the Rust service, not Next.js route handlers.
- [x] Deployment target for v1 is chosen and documented with required secrets, RPC URLs, and scaling assumptions (DigitalOcean App Platform).
