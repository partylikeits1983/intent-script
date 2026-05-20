# [WS-8A] Advisor scan engine — proactive portfolio analysis

**Repo:** `partylikeits1983/intentOS-ui` + `partylikeits1983/intentOS-server`
**Labels:** `area/llm`, `area/data`, `area/portfolio`, `size/L`
**Depends on:** WS-7B, WS-7C

## Context

The advisor mission requires that on connect, intentOS opens with a proactive, opinionated recommendation — "$42k idle USDC; given your wstETH and Aave exposure, I'd put $25k in Morpho USDC at 5.1%, $12k in a leveraged stETH strategy at 7.8%, keep $5k liquid". Today the chat is reactive: it waits for the user to type. This issue builds the proactive scan loop that turns a freshly-loaded portfolio into a first message in the thread.

This is the **MVP heart** per `PRODUCT.md`. Single recommendation per session is enough to ship. Multi-recommendation portfolio views and active monitoring are deferred (see WS-8B for the v1 watch loop).

## Scope

1. Server endpoint `POST /api/v1/advisor/scan`:
   - Input: `{ wallet, chainId }` (auth via SIWE session or bearer key).
   - Joins portfolio context (WS-7C) with live data feeds (WS-7B): vault APYs, market utilization, oracle drift, position health.
   - Returns a typed `AdvisorScan` envelope: `{ summary, recommendations[], risks[], dataFreshness }` where each recommendation is `{ id, headline, reasoning, allocation: { source, target, amount, expectedYield }, intentDraft }`.
2. UI integration (`app/app/page.tsx` + `components/assistant-ui/thread.tsx`):
   - On wallet connect, fire `/scan` automatically (debounced, single-flight per wallet+chain).
   - Render the result as the first assistant message in the thread — opinionated text + recommendation cards (WS-4E).
   - "Refresh recommendations" affordance in header re-runs the scan.
3. LLM call:
   - Server-side LLM with the same compiler-aware tool schema; uses live data and portfolio context as system context.
   - For BYOK users (no server LLM key): scan computes the *non-LLM* allocation candidates and the UI runs the LLM client-side over them.
   - For no-API-key users: hand off to the manual ChatGPT flow (WS-11B) with the scan payload pre-formatted as the prompt.
4. Caching and freshness:
   - Cache scans per wallet+chain for 60s.
   - Mark each recommendation with `dataFreshness.{source, blockNumber, ttlSeconds}` so stale recs are visible.
5. The reactive chat path stays — the scan is *also* available; users can still type a goal and get the existing flow.

## Files

- `intentOS-server/src/routes/advisor.rs` (new)
- `intentOS-server/src/advisor/scan.rs` (new)
- `intentOS-server/src/advisor/llm.rs` (new — server-side LLM client)
- `intentOS-ui/lib/advisor/scan-client.ts` (new)
- `intentOS-ui/components/advisor/scan-result.tsx` (new — first-message renderer)
- `intentOS-ui/app/app/page.tsx` — fire scan on connect
- `intentOS-ui/components/assistant-ui/thread.tsx` — accept seeded first message
- `intentOS-ui/lib/system-prompt.md` — advisor reasoning instructions (new section)

## Acceptance criteria

- [ ] On wallet connect with a non-empty portfolio, the chat opens with a generated `AdvisorScan` first message within 5s p95.
- [ ] The scan never invents balances, positions, or yields — every number traces to a freshness-tagged source from WS-7B/7C.
- [ ] Recommendations include concrete `intentDraft` payloads compatible with `/api/v1/compile`.
- [ ] BYOK users see scans computed client-side over server-provided candidates; server never sees their LLM key.
- [ ] Manual-ChatGPT users get a pre-formatted prompt that, when pasted back, reconstructs the same recommendation set (handoff in WS-11B).
- [ ] Reactive chat (typing a goal directly) still works unchanged.
- [ ] Cache is single-flight per wallet+chain; concurrent loads do not multi-call the LLM.
