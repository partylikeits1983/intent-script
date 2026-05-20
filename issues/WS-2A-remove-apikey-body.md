# [WS-2A] Stop accepting `apiKey` in request body

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/security`, `area/api`, `size/S`
**Depends on:** none (can land before WS-1E if we coordinate)

## Context

Today `app/api/chat/route.ts:38` reads an `apiKey` field from the POST body (`resolvedKey = apiKey || process.env.OPENAI_API_KEY`). That means the server can be forced to make OpenAI calls with a caller-supplied key — a pattern that's hard to rate-limit, hard to audit, and unnecessary given we hold a default key. Remove it.

**Design invariant (per user):** the server holds ONLY the default `OPENAI_API_KEY`. User keys stay in the browser. See [feedback memory on BYOK](../../.claude/projects/-Users-fermat-Desktop-intentOS/memory/feedback_no_byok_server_storage.md).

## Scope

1. Remove `apiKey` from `ChatRequestBody` in `app/api/chat/route.ts` (and in `app/api/v1/chat/route.ts` after WS-1E lands).
2. Server always uses `process.env.OPENAI_API_KEY`. If missing → 503 with a clear operator message (no passthrough of client-provided keys, ever).
3. Remove `apiKey` from the request payload in any caller code (assistant-ui hooks, model-selector, etc.).
4. In `hooks/use-model-config.ts`, keep `openaiApiKey` in the type for now (WS-2B rewires it to go direct-to-OpenAI); just stop SENDING it to our server.
5. Add a dev-only console warning if code attempts to set `apiKey` on a chat request payload.

## Files

- `intentOS-ui/app/api/chat/route.ts` (drop the field)
- `intentOS-ui/app/assistant.tsx` (stop including apiKey)
- Any other files referencing `apiKey` in a chat request body (grep: `apiKey`)

## Acceptance criteria

- [ ] No code path sends `apiKey` as part of a `POST /api/chat` or `POST /api/v1/chat` request.
- [ ] Server rejects any request containing `apiKey` in the body with 400.
- [ ] If `OPENAI_API_KEY` env is unset, 503 with a clear message ("OPENAI_API_KEY is not configured on the server").
- [ ] Existing UI flow still works when the server env var is set.
