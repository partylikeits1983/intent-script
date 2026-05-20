# [WS-6D] Audit log + abuse signal

**Repo:** `partylikeits1983/intentOS-server` + `partylikeits1983/intentOS-ui`
**Labels:** `area/security`, `area/observability`, `size/M`
**Depends on:** WS-1B

## Context

Per-request audit and execution traceability matter for the public API and the executor service. The advisor surface adds one more thing to log: advisor scan + recommendation events, alongside compile/simulate/execute. This issue covers both.

## Scope

1. Rust structured logging (`tracing`):
   - `method`, `path`, `status`, `latency_ms`, `api_key_id`, `session_address`, `wallet_address`, `ip_hash`, `user_agent`, `request_id`.
   - Advisor-specific fields: `advisor.scan_id`, `advisor.recommendation_id`.
2. Execution audit records:
   - Compile request hash, signed payload hash, chain id, signer, fee quote/id, submitted tx hash, receipt status, revert reason if any.
3. Advisor audit records (new):
   - One record per `/advisor/scan`: input hash, recommendation set hash, model id, latency.
4. Sink configuration:
   - `stdout` for local; OpenTelemetry / Axiom / Logtail for production.
5. Abuse heuristics:
   - > 5 `COMPILE_ERROR` per key in 60s.
   - > 20 401s per IP hash in 60s.
   - Repeated reverted executor submissions from one key/wallet.
   - Duplicate signed-payload replay attempts.
6. `X-Request-Id` propagated on every response and through executor + advisor records.
7. Next.js chat route logs use the same request-id/error-envelope convention without logging user OpenAI keys or full prompts.

## Files

- `intentOS-server/src/logging.rs` (new)
- `intentOS-server/src/audit.rs` (new)
- `intentOS-server/src/audit/advisor.rs` (new)
- `intentOS-server/src/routes/*.rs`
- `intentOS-server/src/store/executions.rs`
- `intentOS-ui/lib/api/logger.ts`
- `intentOS-ui/.env.example`
- `intentOS-server/.env.example`

## Acceptance criteria

- [ ] Every Rust `/api/v1/*` request produces exactly one structured request log.
- [ ] Every executor submission has an auditable lifecycle from accepted → submitted → confirmed/reverted.
- [ ] Every advisor scan is recorded.
- [ ] Logs contain no secrets, raw API keys, user OpenAI keys, or full prompts.
- [ ] `X-Request-Id` present on all responses.
- [ ] Abuse signal events fire during synthetic load/replay tests.
