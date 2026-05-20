# [WS-1A] API spec + zod schemas

**Repo:** `partylikeits1983/intentOS-server` + `partylikeits1983/intentOS-ui`
**Labels:** `area/api`, `type/scaffolding`, `size/M`
**Depends on:** WS-0A — blocker for 1B, 1C, 1D, 1E, 5C, 6A, 7D

## Context

intentOS today exposes exactly one Next.js API route (`app/api/chat/route.ts`) with no schema, no validation, no contract. The production architecture splits responsibilities:

- browser UI keeps local WASM compilation for normal users;
- Rust `intentOS-server` exposes agent-facing compile/simulate/executor APIs;
- Next.js keeps UI/chat helpers where they belong.

Before we build those endpoints, we need a single source of truth for request/response shapes across Rust and TypeScript.

## Scope

1. Write `docs/openapi.yaml` (OpenAPI 3.1) covering every planned endpoint:
   - `POST /api/v1/chat` — streaming chat (Next.js UI service)
   - `POST /api/v1/compile` — NL/DSL intent → compiled tx output
   - `POST /api/v1/simulate` — compiled tx → simulation result
   - `POST /api/v1/execute` — submit signed EIP-712 intent for server execution/relaying
   - `GET /api/v1/executions/:id` — executor status/receipt
   - `GET|POST|DELETE /api/v1/keys` — agent API-key CRUD (implemented in 1B)
2. In Rust, generate or validate OpenAPI from typed request/response structs using `utoipa` or `aide`.
3. In TypeScript, generate client types/zod validators from the OpenAPI spec instead of hand-maintaining a second contract.
4. Define one typed error envelope `{ error: { code: string; message: string; details?: unknown; request_id?: string } }`. Enumerate codes: `INVALID_INPUT`, `UNAUTHORIZED`, `RATE_LIMITED`, `UPSTREAM_ERROR`, `COMPILE_ERROR`, `SIMULATION_ERROR`, `EXECUTION_ERROR`, `SERVER_ERROR`.
5. Add contract checks in both repos:
   - Rust: `cargo test` validates generated OpenAPI.
   - UI: `pnpm verify:api` validates generated TS client/types against the checked-in spec.

## Files

- `intentOS-server/openapi.yaml` or generated OpenAPI artifact (new)
- `intentOS-server/src/api/types.rs` (new)
- `intentOS-server/src/api/errors.rs` (new)
- `intentOS-ui/docs/openapi.yaml` (synced copy or generated docs input)
- `intentOS-ui/lib/api/client.ts` (generated or thin wrapper)
- `intentOS-ui/scripts/validate-openapi.ts` (new)
- `intentOS-ui/package.json` (add `verify:api` script and OpenAPI client-generation deps)

## Reuse

- Existing types in `lib/intent-compiler-types.ts`, `lib/intent-errors.ts` — reference, don't duplicate.

## Acceptance criteria

- [x] OpenAPI spec validates against the OpenAPI 3.1 JSON Schema.
- [x] Rust API types and TS client types are generated from or checked against the same contract.
- [x] `pnpm verify:api` exits 0 on a clean tree.
- [x] Error envelope is defined in exactly one place and reused.
- [x] No route handlers in this PR (contract-only).
