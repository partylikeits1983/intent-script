# [WS-6B] Rate limiting middleware

**Repo:** `partylikeits1983/intentOS-server` + `partylikeits1983/intentOS-ui`
**Labels:** `area/security`, `area/api`, `size/M`
**Depends on:** WS-1B

## Context

No rate limits today. The public Rust API, executor endpoint, and default server LLM key are abuse surfaces. Rate limiting should primarily live in `intentOS-server`, with lighter limits on UI-owned chat endpoints.

## Scope

1. Rust service rate limiting:
   - Redis-backed sliding-window or token-bucket limiter.
   - Middleware derives tier from auth context:
     - anonymous: 10 req/min, burst 20;
     - session: 60 req/min;
     - api-key: 1000 req/min per key by default;
     - executor submit: tighter per-key and per-wallet idempotency limits.
2. Per-route overrides:
   - `/api/v1/compile` and `/api/v1/simulate`: CPU/RPC-aware limits;
   - `/api/v1/execute`: nonce/idempotency aware, no unbounded retries;
   - `/api/v1/chat`: lower anonymous/default-key budget.
3. Rate-limit response:
   - 429 with `Retry-After`;
   - shared error envelope with `RATE_LIMITED`.
4. Dev/local behavior:
   - skip or 10x multiplier in development;
   - never skip in staging/production.

## Files

- `intentOS-server/src/rate_limit.rs` (new)
- `intentOS-server/src/middleware.rs`
- `intentOS-server/.env.example`
- `intentOS-ui/middleware.ts` or chat route wrapper for UI-owned endpoints
- `intentOS-ui/.env.example`

## Acceptance criteria

- [ ] Anonymous burst of 30 req/s returns 429 after the configured burst.
- [ ] Authed API key gets the documented limit.
- [ ] Executor submissions are idempotent and cannot spam duplicate broadcasts.
- [ ] 429 responses match the WS-1A error envelope.
- [ ] Sliding-window behavior verified by integration test.
