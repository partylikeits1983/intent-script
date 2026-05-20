# [WS-2B] Direct browser→OpenAI for BYOK

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/security`, `area/llm`, `size/M`
**Depends on:** WS-2A

## Context

After WS-2A the server stops accepting user API keys. Users who want to use their own OpenAI key (free from our rate limit, their own billing) should have the browser call OpenAI directly. Keys never leave the user's browser except to go to OpenAI. No server involvement on that path.

## Scope

1. Create `lib/llm/client.ts` that exposes a single `chat(messages, options)` primitive. It picks the transport based on `ModelConfig`:
   - `openaiApiKey` set → `@ai-sdk/openai` client created in-browser with `dangerouslyAllowBrowser: true` (the key is already in the browser; this is expected).
   - `openaiApiKey` not set → POSTs to our `/api/v1/chat` (default server path).
   - `mode === "local"` → same as today (Ollama-compatible endpoint).
2. Refactor the two consumers to use this primitive:
   - `components/finalize-intent-tool.tsx` (programmatic tool flow)
   - `components/chatgpt-flow.tsx` (manual paste flow, if it currently hits our server — it doesn't, but confirm)
   - `app/assistant.tsx` (assistant-ui runtime) — swap the `api` URL for the new dispatcher.
3. Update the API-key banner copy: "Your key stays in your browser and is sent directly to OpenAI. Clear it anytime in Settings."
4. Document the threat model in `docs/security.mdx` (this ships as part of WS-5B): exposed keys if the browser is compromised, which is the user's machine, and no worse than using OpenAI's Playground.

## Files

- `intentOS-ui/lib/llm/client.ts` (new)
- `intentOS-ui/components/finalize-intent-tool.tsx`
- `intentOS-ui/components/api-key-banner.tsx`
- `intentOS-ui/components/model-selector.tsx` (clarifying copy)
- `intentOS-ui/app/assistant.tsx`

## Reuse

- `hooks/use-model-config.ts` — no changes to the shape; just change who reads it.

## Acceptance criteria

- [ ] With a user OpenAI key set, Network tab shows `POST https://api.openai.com/v1/...` and NO `POST /api/v1/chat`.
- [ ] With no user key, Network tab shows `POST /api/v1/chat` and NO `api.openai.com` requests.
- [ ] Clearing the key in settings reverts to the server path immediately.
- [ ] Key is never sent in any request whose URL contains `/api/` on our origin.
- [ ] Banner copy clearly states "stays in your browser."
