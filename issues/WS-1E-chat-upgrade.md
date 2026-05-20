# [WS-1E] `/api/v1/chat` upgrade

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/api`, `area/llm`, `size/S`
**Depends on:** WS-1A; soft-blocks WS-2A

## Context

Rename the existing chat route to the v1 prefix, tighten its contract, and remove the in-body `apiKey`. The server only uses its env `OPENAI_API_KEY`. No new providers added here — BYOK is handled in WS-2B by going client-direct.

## Scope

1. Move `app/api/chat/route.ts` → `app/api/v1/chat/route.ts`.
2. Keep `app/api/chat/route.ts` as a redirect (`307` to `/api/v1/chat`) for one release cycle, then remove (tracked as follow-up).
3. Request body (zod from WS-1A) drops `apiKey` and `mode`/`endpoint` — the v1 route is server-default OpenAI only.
4. Client code that currently sends these fields (`app/assistant.tsx`, any hook using `useModelConfig`) is updated to stop sending them.
5. Response shape unchanged (assistant-ui UI message stream).

## Files

- `intentOS-ui/app/api/v1/chat/route.ts` (new, moved from existing)
- `intentOS-ui/app/api/chat/route.ts` (becomes a 307 redirect)
- `intentOS-ui/app/assistant.tsx` — drop apiKey/mode/endpoint from the chat-request payload
- `intentOS-ui/lib/api/schemas.ts` — ensure the chat schema matches the new body

## Acceptance criteria

- [ ] Old `POST /api/chat` 307s to `/api/v1/chat`; existing clients still work.
- [ ] New `POST /api/v1/chat` rejects requests containing `apiKey` with 400 (`INVALID_INPUT`).
- [ ] If server env `OPENAI_API_KEY` is missing, returns 503 with clear operator message.
- [ ] Streaming response still renders correctly in the UI.
- [ ] Existing smoke e2e (`e2e/smoke.spec.ts`) still passes.
