# [WS-1B] Agent API-key issuance

**Repo:** `partylikeits1983/intentOS-server` + `partylikeits1983/intentOS-ui`
**Labels:** `area/api`, `area/auth`, `size/M`
**Depends on:** WS-0A, WS-1A

## Context

External agents need to call intentOS programmatically. That requires our own API key system — keys WE issue, that agents put in `Authorization: Bearer <key>` headers. These are distinct from user OpenAI keys (those stay in the browser per WS-2).

API-key verification should live in the Rust service because compile/simulate/execute are agent-facing Rust endpoints. The UI only needs SIWE and a settings panel for creating/revoking keys.

## Scope

1. SIWE (Sign-In With Ethereum) session for the UI — reuses the already-connected wallet. Session cookie: httpOnly, Secure, SameSite=Lax, signed with `AUTH_SECRET`.
2. Session/API-key store: Redis or Postgres owned by `intentOS-server`. Session holds `{ address, createdAt, expiresAt }` only — **no user OpenAI keys**.
3. API-key endpoints (per WS-1A OpenAPI spec):
   - `POST /api/v1/keys` — create new key for the signed-in wallet; returns the key exactly once (prefix `ios_live_<random-32>`), stores SHA-256 hash + metadata in Redis.
   - `GET /api/v1/keys` — list keys for the wallet (id, name, prefix, createdAt, lastUsedAt, revokedAt).
   - `DELETE /api/v1/keys/:id` — revoke.
4. Rust auth middleware:
   - `requireSession(req)` → returns `{ address }` or 401.
   - `requireApiKey(req)` → reads `Authorization: Bearer`, hashes, looks up in Redis, returns `{ keyId, address }` or 401.
5. UI: new settings panel "API Keys" (`components/api-keys-panel.tsx`) — calls the Rust service to list, create, revoke. Only shown when wallet is connected.

## Files

- `intentOS-server/src/routes/keys.rs` (new)
- `intentOS-server/src/routes/auth.rs` (new — SIWE nonce + verify)
- `intentOS-server/src/auth.rs` (new)
- `intentOS-server/src/store/*` (new)
- `intentOS-ui/components/api-keys-panel.tsx` (new)
- `intentOS-ui/lib/api/client.ts`
- `intentOS-ui/.env.example` (add `NEXT_PUBLIC_INTENTOS_API_URL`)
- `intentOS-server/.env.example` (add `AUTH_SECRET`, store vars)

## Security notes

- Store **only** the SHA-256 hash of the key. The raw key is shown once at create time.
- Rotate `AUTH_SECRET` story: document in `docs/security.mdx` (WS-5B).
- Keys scoped to the issuing wallet address; revocation is immediate (no TTL grace period).

## Acceptance criteria

- [ ] Unauthenticated request to `POST /api/v1/keys` → 401.
- [ ] SIWE flow works end-to-end (nonce → signature → cookie).
- [ ] Raw key only returned once; subsequent GETs return prefix + metadata only.
- [ ] Revoked keys fail `requireApiKey` within 1s.
- [ ] No user OpenAI key is stored anywhere server-side (verified by grepping code review).
