# [WS-7B] Live quote and advisor data feeds

**Repo:** `partylikeits1983/intentOS-server` + `partylikeits1983/intentOS-ui`
**Labels:** `area/data`, `area/defi`, `area/llm`, `size/L`
**Depends on:** WS-1A

## Context

The reactive chat used to receive only spot prices from CoinGecko, leading the LLM to guess slippage/min-output. The advisor surface needs more — yields across supported protocols, market utilization, position health — so its scan engine can emit recommendations grounded in live numbers. Same data layer serves both surfaces.

## Scope

1. Rust server-side data endpoints with typed schemas:
   - `GET /api/v1/quotes/swap` — Uniswap quote, optional 1inch route/calldata.
   - `GET /api/v1/quotes/bridge` — Across suggested relayer fee, limits, fill deadline, destination validation.
   - `GET /api/v1/lido/withdrawal-hints` — checkpoint hints for claimable withdrawal request IDs.
   - `GET /api/v1/uniswap/pool-context` — current tick, current human price, fee tier, tick spacing, suggested wide/tight ranges.
   - `GET /api/v1/yields` — APY/APR snapshots across supported protocols (Aave V3, Morpho Blue markets, Lido, selected LP opportunities).
   - **New** `GET /api/v1/positions/health` — per-wallet health-factor and utilization for Aave V3 / Morpho Blue exposure (advisor input).
2. Client helper `lib/defi-data/client.ts` with freshness metadata: `source`, `blockNumber` or timestamp, `ttlSeconds`, `isStale`.
3. Thread quote data into:
   - LLM context for the reactive chat (existing flow).
   - Advisor scan engine (WS-8A) — yields, market utilization, position health.
   - Strategy gallery (WS-4A) — APY badges.
   - Risk panel (WS-4F) — drift visualization.
4. Cache and rate limits:
   - Short TTL per query.
   - Explicit stale-data behavior; no silent fallback to fabricated values.
5. Tests with mocked providers and anvil/fork fixtures.

## Files

- `intentOS-server/src/routes/quotes.rs` (new)
- `intentOS-server/src/routes/lido.rs` (new)
- `intentOS-server/src/routes/uniswap.rs` (new)
- `intentOS-server/src/routes/yields.rs` (new)
- `intentOS-server/src/routes/positions.rs` (new)
- `intentOS-ui/lib/defi-data/client.ts` (new)
- `intentOS-ui/lib/defi-data/providers/*.ts` (new)
- `intentOS-ui/lib/system-prompt.md`
- `intentOS-ui/.env.example`
- `intentOS-server/.env.example`

## Acceptance criteria

- [ ] Swap intents use quoted min-out values with source and freshness shown.
- [ ] `via: "1inch"` is only emitted when calldata was actually fetched, never invented.
- [ ] Lido claim intents can be built from wallet request IDs without asking the user for hints.
- [ ] Across bridge intents use fetched relayer fee and deadline.
- [ ] LP mint review shows current pool price relative to requested range.
- [ ] Stale or unavailable quote data disables execution or asks the user to refresh; no silent fabrication.
- [ ] `/yields` returns APYs across all supported protocols with a `source` and freshness pill.
- [ ] `/positions/health` feeds the advisor scan end-to-end.
