# [WS-3A-SERVER] CI pipeline for intentOS-server

**Repo:** `partylikeits1983/intentOS-server`
**Labels:** `area/ci`, `area/rust`, `size/S`
**Depends on:** WS-0A

## Context

The Rust API/executor service will be a production-critical backend. It needs CI from the first scaffold PR, especially because it depends on compiler output parity, API contract stability, and execution safety.

## Scope

1. Add `.github/workflows/ci.yml` with jobs:
   - `fmt` — `cargo fmt --all -- --check`
   - `clippy` — `cargo clippy --all-targets -- -D warnings`
   - `test` — `cargo test --all`
   - `openapi` — generate/validate OpenAPI and fail on drift
   - `docker-build` — build the service image
2. Add optional integration job:
   - starts Redis/Postgres if selected;
   - starts anvil for compile/simulate/executor tests;
   - runs endpoint tests against a live Axum server.
3. Add `make ci` or `just ci` for local parity with CI.
4. Add status badge in server README.

## Files

- `intentOS-server/.github/workflows/ci.yml` (new)
- `intentOS-server/Makefile` or `intentOS-server/justfile` (new)
- `intentOS-server/README.md`
- `intentOS-server/Dockerfile`

## Acceptance criteria

- [ ] Every PR triggers fmt, clippy, tests, OpenAPI validation, and Docker build.
- [ ] CI fails if generated OpenAPI differs from the checked-in contract.
- [ ] Integration job documents required secrets/RPC URLs and can run locally.
- [ ] Branch protection requires the CI workflow before merge.
