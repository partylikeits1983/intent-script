# [WS-7C] Portfolio and position context hardening

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/data`, `area/portfolio`, `area/defi`, `size/L`
**Depends on:** WS-3B

## Context

The assistant can only create correct complex transactions if it has accurate wallet and protocol state. The current UI reads a useful subset of balances and positions, but parts are hardcoded to Ethereum mainnet addresses and static market lists. Production readiness needs chain-aware, protocol-aware position context that the LLM, compiler, simulator, and review panel all share.

## Scope

1. Replace hardcoded protocol/token address lists in UI data hooks with generated config from `intent-script/config` or a shared package.
2. Make all portfolio reads chain-aware:
   - anvil fork uses chain 31337 with mainnet-fork addresses;
   - mainnet uses chain 1;
   - unsupported chains show an explicit unsupported state.
3. Expand indexed context:
   - wallet token balances for every configured asset;
   - Aave supplied/borrowed assets, health factor, available borrow, LTV;
   - Morpho positions for every configured market, including collateral and borrow/supply shares converted to assets;
   - Uni V3 NFT positions with token symbols, fee, range, liquidity, owed fees, current in/out-of-range status;
   - Lido withdrawal NFTs with finalized/claimed status;
   - bridge-relevant destination address defaults.
4. Produce one canonical `PortfolioContext` object consumed by:
   - LLM system prompt;
   - compiler `balances` payload;
   - simulation pre/post comparison;
   - recommendation cards;
   - transaction review panel.
5. Add freshness and failure metadata:
   - per-protocol loading/error states;
   - block number;
   - stale indicator;
   - "not indexed" distinction from "zero position."

## Files

- `intentOS-ui/lib/portfolio-summary.ts`
- `intentOS-ui/lib/token-balances.ts`
- `intentOS-ui/lib/generate-export-md.ts`
- `intentOS-ui/lib/portfolio-context.ts` (new)
- `intentOS-ui/lib/config/*`
- `intentOS-ui/components/portfolio-overlay.tsx`
- `intentOS-ui/components/token-balances-panel.tsx`
- `intentOS-ui/components/finalize-intent-tool.tsx`

## Acceptance criteria

- [ ] No UI portfolio hook hardcodes protocol addresses that already exist in shared config.
- [ ] The LLM context includes every supported position type with enough fields to close, reduce, claim, or adjust it without asking for data already on-chain.
- [ ] Morpho market context is generated from config and supports more than one market.
- [ ] Uni V3 close/decrease prompts can resolve position ID, liquidity, token pair, and range from context.
- [ ] Lido claim prompts can resolve request IDs and finalized status from context.
- [ ] Stale or failed reads are represented explicitly in UI and prompt context.
