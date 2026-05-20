# [WS-4A] Strategy gallery — what the advisor can recommend

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/ux`, `area/ui`, `size/M`
**Depends on:** none

## Context

The empty state today is a generic "Hello there! What would you like to do onchain?". Under the advisor mission this surface should preview the strategies the advisor can recommend — concrete strategies tied to allocations the advisor will produce against external protocols (no owned vault yet; that's deferred per `PRODUCT.md`).

## Scope

1. Static catalog `lib/strategies.ts`:
   ```ts
   export const strategies = [
     { id: "morpho-usdc", title: "Morpho Blue USDC market", apy: "live", icon, description, oneLiner: "Earn yield on idle stables", networks: ["mainnet"] },
     { id: "aave-supply", title: "Aave V3 supply", apy: "live", description },
     { id: "lido-staking", title: "Stake ETH on Lido", apy: "live", description },
     { id: "uni-v3-lp", title: "Uni V3 LP", description, oneLiner: "Provide liquidity for fees" },
     { id: "leveraged-steth", title: "Leveraged stETH (Aave loop)", description, oneLiner: "ETH yield + borrowed leverage" },
     { id: "across-bridge", title: "Bridge with Across", description },
     // ...
   ]
   ```
   - APYs flagged `"live"` are filled at render time from `GET /api/v1/yields` (WS-7B). Never inline a hardcoded APY.
2. `components/strategy-gallery.tsx`:
   - Renders strategy cards with live APY + freshness pill.
   - Clicking a card asks the advisor to recommend that strategy with the user's portfolio context (calls WS-8A `/api/v1/advisor/scan?strategy=<id>`), opening a recommendation card in the thread.
   - Static catalog is the source of truth for "what the advisor offers" — predictable, fast, easy to edit.
3. Gallery rendered on the empty state of `components/assistant-ui/thread.tsx`, replacing generic welcome copy. Existing dynamic `ThreadPrimitive.Suggestions` stays below for LLM-generated follow-ups.
4. Reactive chat preserved: typing a free-form goal still works. Gallery is *one* affordance, not the only one.

## Files

- `intentOS-ui/lib/strategies.ts` (new — replaces `lib/capabilities.ts` if it existed)
- `intentOS-ui/components/strategy-gallery.tsx` (new)
- `intentOS-ui/components/strategy-card.tsx` (new)
- `intentOS-ui/components/assistant-ui/thread.tsx` — replace welcome
- `intentOS-ui/lib/advisor/scan-client.ts` — accept `strategy` param

## Acceptance criteria

- [ ] New user landing on `/app` sees the strategy gallery on the empty state.
- [ ] APYs render live from server endpoints with a freshness pill; never hardcoded.
- [ ] Clicking a strategy card produces a recommendation card in the thread sized to the user's portfolio.
- [ ] Catalog is exported and testable in isolation.
- [ ] Responsive layout works at 375px width.
- [ ] Reactive-chat smoke test still passes.
