# [WS-5C] API reference from OpenAPI

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/docs`, `area/api`, `size/S`
**Depends on:** WS-1A, WS-5B, WS-8A

## Context

WS-1A produces `docs/openapi.yaml`. The advisor scan endpoint (WS-8A) will be added to it. Render the spec inside the Fumadocs site (WS-5B) at `/docs/api` and keep regeneration deterministic.

## Scope

1. Install `fumadocs-openapi`; configure to read `docs/openapi.yaml` and emit MDX under `content/docs/api/`.
2. `pnpm gen:api-docs` script runs as part of `pnpm build`.
3. One MDX page per endpoint group: `chat`, `compile`, `simulate`, `execute`, `keys`, `auth`, `advisor`.
   - Each shows request/response schemas as tables, an example curl, and a try-it widget.
4. Try-it widget:
   - Preloads a free-tier API key from localStorage if logged in.
   - Targets a dedicated `api.intentos.dev` origin so try-it traffic doesn't hit production.
5. Re-run safety: regenerating with `pnpm gen:api-docs` produces deterministic output (no diff on successive runs against an unchanged spec).

## Files

- `intentOS-ui/content/docs/api/*.mdx` (auto-generated)
- `intentOS-ui/source.config.ts` — include openapi generator
- `intentOS-ui/scripts/gen-api-docs.ts` (new)
- `intentOS-ui/package.json` — script + dep

## Acceptance criteria

- [ ] `/docs/api` index lists every endpoint from `openapi.yaml`, including the advisor scan group.
- [ ] Each endpoint page shows schemas, example curl, and a try-it widget.
- [ ] Try-it widget hits `api.intentos.dev`, not production.
- [ ] Re-running `pnpm gen:api-docs` against an unchanged spec produces no diff.
