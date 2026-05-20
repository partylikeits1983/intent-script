# [WS-1C] Rust `/api/v1/compile` endpoint

**Repo:** `partylikeits1983/intentOS-server`
**Labels:** `area/api`, `area/compiler`, `area/rust`, `size/M`
**Depends on:** WS-0A, WS-1A, WS-1B

## Context

The UI should keep compiling in-browser through WASM because it is fast and avoids a server round trip. External agents need a server endpoint that returns the same compiled output without embedding the compiler themselves.

This endpoint is Rust-native, served by `intentOS-server` (Axum + the `intent-script` crate). The browser keeps its WASM path for connected users; agent traffic must go through the Rust handler. There is no Node/Next.js fallback for this route.

## Scope

1. Add `intent-script` as a Rust dependency of `intentOS-server`:
   - use the compiler crate directly;
   - pin by git ref, submodule, or workspace dependency;
   - keep config loading compatible with `intent-script/config`.
2. Implement `POST /api/v1/compile` in Axum:
   - Auth: `require_api_key`.
   - Body: OpenAPI schema from WS-1A (`{ intent, network, allowances?, balances? }`).
   - Response: same JSON shape as browser WASM `CompileOutputJson`.
3. Preserve browser-local compile:
   - no regression to `intentOS-ui/lib/intent-compiler.ts`;
   - UI compile remains the primary connected-wallet path.
4. Add parity tests:
   - compile fixtures through browser WASM output and Rust server output;
   - assert byte-identical calldata, typed data, approvals, preview, and output type.
5. Map compiler errors to the shared error envelope and stable structured codes.

## Files

- `intentOS-server/src/routes/compile.rs` (new)
- `intentOS-server/src/compiler.rs` (new)
- `intentOS-server/src/api/types.rs`
- `intentOS-server/Cargo.toml`
- `intentOS-server/tests/compile_parity.rs`
- `intentOS-ui/lib/intent-compiler.ts` (reference only; keep browser path)

## Reuse

- `intent-script/crates/intent-script` compiler crate.
- Existing WASM/browser output types in `intentOS-ui/lib/intent-compiler-types.ts` as the TS compatibility contract.

## Acceptance criteria

- [ ] `curl -X POST $INTENTOS_API_URL/api/v1/compile -H 'Authorization: Bearer <key>' -d @fixture.json` returns a valid compiled output.
- [ ] Same fixture compiled in browser WASM and Rust server produces byte-identical calldata/typed data.
- [ ] Invalid input returns 400 with `INVALID_INPUT`.
- [ ] Compiler failures return `COMPILE_ERROR` with structured details.
- [ ] Missing/invalid API key returns 401.
- [ ] UI compile path still runs locally through browser WASM.
