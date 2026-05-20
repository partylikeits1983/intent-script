# [WS-11B] Manual ChatGPT prompt flow — productize the no-API-key onramp

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/ux`, `area/llm`, `size/S`
**Depends on:** WS-8A

## Context

Mission: "for users who don't want to share an LLM API key with the app, a manual flow copies a context-rich prompt to run in ChatGPT and paste the response back — keys stay client-side; the server stores none." The components exist (`chatgpt-flow.tsx`, `chatgpt-prompt-card.tsx`) but were built around the reactive chat. With the advisor surface (WS-8A) producing recommendations, this flow needs to be the actual onramp: a no-API-key user lands, gets a context-rich prompt to paste into ChatGPT, pastes the response back, and gets the same recommendation cards as the BYOK/server-LLM users.

## Scope

1. Prompt generator: take the WS-8A scan input (portfolio + live data) and emit a single shareable prompt that ChatGPT can answer to produce a structured `AdvisorScan` payload.
   - Includes the JSON schema for the response so ChatGPT returns parseable output.
   - Includes example one-shot to anchor formatting.
   - Watermarked with a session id so a user can't paste an old/foreign response.
2. Paste-back parser:
   - Accepts the ChatGPT response, validates schema, surfaces clear errors when ChatGPT freelanced.
   - Recovers gracefully when ChatGPT wraps JSON in prose — extract first valid JSON block.
3. UI surfaces (extend existing components):
   - Onboarding: detect no API key + no server LLM access → show the manual flow with a "what is this?" expander.
   - "Copy prompt" → toast confirms; "Paste response" textarea with live-validation and a "use these recommendations" button.
   - Recommendations from manual flow render through the same card components as BYOK (WS-4E).
4. Safety:
   - Manual responses still flow through the same compile + simulate path; ChatGPT freelancing intent details cannot bypass compiler invariants.
   - Watermark mismatch shows a clear error rather than silently using the wrong response.

## Files

- `intentOS-ui/components/chatgpt-flow.tsx`
- `intentOS-ui/components/chatgpt-prompt-card.tsx`
- `intentOS-ui/lib/manual-prompt/build.ts` (new)
- `intentOS-ui/lib/manual-prompt/parse.ts` (new)
- `intentOS-ui/lib/manual-prompt/schema.ts` (new — shared with WS-8A AdvisorScan envelope)

## Acceptance criteria

- [ ] No-API-key user can connect a wallet, copy a prompt, paste a ChatGPT response, and see the same card surface as a BYOK user.
- [ ] Pasted response with mismatched watermark fails closed.
- [ ] Pasted response with wrapped/prosaic JSON is recovered or fails with a clear "ChatGPT didn't return valid JSON" message.
- [ ] Manual-flow recommendations execute through the same compile + simulate + sign path; no shortcut.
- [ ] Server never receives the user's prompt or ChatGPT response.
