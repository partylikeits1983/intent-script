# [WS-6A] Runtime input validation on all `/api/v1/*`

**Repo:** `partylikeits1983/intentOS-server` + `partylikeits1983/intentOS-ui`
**Labels:** `area/security`, `area/api`, `size/S`
**Depends on:** WS-1A (and lands alongside 1B/1C/1D/1E)

## Context

The production API is split across Rust and Next.js:

- Rust `intentOS-server` owns agent-facing `/api/v1/compile`, `/api/v1/simulate`, `/api/v1/execute`, `/api/v1/keys`, and related status endpoints.
- Next.js owns UI/chat helpers such as `/api/v1/chat`.

Both runtimes need real runtime validation. TypeScript interfaces and Rust structs alone are not enough unless request parsing, size limits, headers, params, and error mapping are explicit.

## Scope

1. Rust service validation:
   - validate JSON bodies through serde + typed validators;
   - validate URL params, headers, network ids, addresses, and hex strings;
   - reject oversized bodies early;
   - map validation failures to `{ error: { code: "INVALID_INPUT", ... } }`.
2. TypeScript/Next validation:
   - use generated zod or equivalent schemas from WS-1A for `/api/v1/chat` and any remaining Next API route;
   - reject unexpected fields such as `apiKey` in server chat requests.
3. Centralized helpers:
   - Rust: `extractors`/middleware for validated JSON and auth headers.
   - UI: `lib/api/handler.ts` only for Next-owned endpoints.
4. Body size limits:
   - default 256KB;
   - explicit larger caps only where justified.

## Files

- `intentOS-server/src/api/validation.rs` (new)
- `intentOS-server/src/api/errors.rs`
- `intentOS-server/src/routes/*.rs`
- `intentOS-ui/lib/api/handler.ts` (new, Next-owned endpoints only)
- `intentOS-ui/app/api/v1/chat/route.ts`

## Acceptance criteria

- [ ] Each Rust `/api/v1/*` route returns 400 + `INVALID_INPUT` for malformed body/params/headers.
- [ ] Next `/api/v1/chat` rejects malformed requests and unexpected `apiKey`.
- [ ] Oversized bodies return 413.
- [ ] Unit/integration tests cover at least one invalid-input case per route.
- [ ] No handler trusts a raw unvalidated request body.
