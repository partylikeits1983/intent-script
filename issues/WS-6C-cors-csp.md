# [WS-6C] CORS allowlist + CSP headers

**Repo:** `partylikeits1983/intentOS-server` + `partylikeits1983/intentOS-ui`
**Labels:** `area/security`, `size/S`
**Depends on:** none

## Context

No CORS or CSP config. For the Rust public API, we want a strict CORS allowlist. For the app, we want CSP that prevents injected scripts from ex-filtrating wallet signatures.

## Scope

1. CORS:
   - Rust `intentOS-server` adds `Access-Control-Allow-Origin` for `/api/v1/*` based on `CORS_ALLOWED_ORIGINS` env (comma-separated).
   - Next.js only applies CORS to UI-owned API routes such as `/api/v1/chat` if they are callable cross-origin.
   - Default allowlist: the intentOS-ui origin + `http://localhost:*` in dev. Public API origins added as we onboard partners.
   - Preflight: respond to `OPTIONS` with the usual headers.
2. CSP in `next.config.ts` via `headers()`:
   - `default-src 'self';`
   - `script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval' https://cdn.walletconnect.com;` (inline is needed by Next, wasm-unsafe-eval for the compiler).
   - `connect-src 'self' https://api.openai.com https://*.upstash.io https://*.walletconnect.com https://*.alchemy.com https://*.infura.io https://rpc.*.io https://eth.publicnode.com;`
   - `img-src 'self' data: https:;`
   - `style-src 'self' 'unsafe-inline';`
   - `frame-ancestors 'none';`
3. Additional headers:
   - `X-Frame-Options: DENY`
   - `Referrer-Policy: strict-origin-when-cross-origin`
   - `Permissions-Policy: camera=(), microphone=(), geolocation=()`
   - `Strict-Transport-Security: max-age=63072000; includeSubDomains; preload` (production only)

## Files

- `intentOS-server/src/cors.rs` or `tower-http` CORS layer config (new)
- `intentOS-ui/middleware.ts` (new or extended for UI-owned endpoints)
- `intentOS-ui/next.config.ts` — `headers()` export
- `intentOS-server/.env.example` — `CORS_ALLOWED_ORIGINS`
- `intentOS-ui/.env.example` — `CORS_ALLOWED_ORIGINS` if needed for chat

## Acceptance criteria

- [ ] `curl -I` against the app shows the full header set.
- [ ] A cross-origin request from a non-allowed origin to `/api/v1/*` is rejected at CORS preflight.
- [ ] CSP passes https://securityheaders.com with grade A or above.
- [ ] No CSP violations in the browser console during normal UI use (tested with dev tools open).
- [ ] Key BYOK path (`api.openai.com`) works — `connect-src` allows it.
