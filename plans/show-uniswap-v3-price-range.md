# Show price range (not tick range) for Uniswap v3 LP positions

## Context

Today, the portfolio overlay's "Uniswap V3 LP" section displays each position's range as raw int24 ticks, e.g. `Range [200000, 210000]` (`intentOS-ui/components/portfolio-overlay.tsx:332`). Ticks are an implementation detail of Uniswap v3 and are unreadable to humans — a user holding a USDC/WETH LP cannot tell from `[200000, 210000]` whether their position is in range, near the boundary, or far out.

The compiler/intent layer already prefers human prices: `lp_mint` accepts `price_lower`/`price_upper` and the finalize-intent preview at `components/finalize-intent-tool.tsx:1300-1305` already prints prices for new mints. The portfolio panel is the last surface still showing raw ticks.

The fix is local: tick → price is a closed-form conversion (`price = 1.0001^tick * 10^(decimals0 - decimals1)`), token decimals are already resolved in `useUniV3Summary`, and the rendering site is a single `KeyValue` row.

User decisions captured in this plan:
- **Quote direction**: smart heuristic — prefer the stable token as the quote (so a USDC/WETH range reads as "1820 – 2150 USDC per WETH"); fall back to `token1` per `token0` for unknown pairs.
- **Ticks in UI**: replace entirely. Ticks stay in the markdown export so the LLM can still reference them when generating close/decrease intents.

## Approach

1. Add a small pure-math helper module for tick↔price conversion + range formatting.
2. Extend `UniV3PositionSummary` with formatted price fields, computed once inside `useUniV3Summary` where decimals are already in scope.
3. Replace the tick render in `portfolio-overlay.tsx` with the new fields.
4. Leave `lib/generate-export-md.ts` as-is (ticks still in the markdown export).

No new dependencies. All math fits in JS `number` for the tick range Uniswap actually uses (`MIN_TICK = -887272`, `MAX_TICK = 887272` → `1.0001^tick` stays within float range; precision loss at the extremes is fine for display since those map to "0" / "∞").

## Files to modify

### 1. New file: `intentOS-ui/lib/uniswap-v3-price.ts`

Pure helpers, no React/wagmi imports. Export:

- `MIN_TICK = -887272`, `MAX_TICK = 887272`
- `tickToPrice(tick: number, decimals0: number, decimals1: number): number` — returns `1.0001 ** tick * 10 ** (decimals0 - decimals1)`. This is the price of 1 unit of token0 expressed in token1 (token1-per-token0).
- `STABLE_SYMBOLS = new Set(["USDC", "USDT", "DAI"])` — used by the heuristic.
- `formatPriceRange(args: { tickLower: number; tickUpper: number; token0Symbol: string; token1Symbol: string; decimals0: number; decimals1: number }): { lower: string; upper: string; quoteLabel: string; isFullRange: boolean }`

  Behavior:
  - If `tickLower <= MIN_TICK + tickSpacing-ish` AND `tickUpper >= MAX_TICK - tickSpacing-ish` (use a generous threshold like `<= -887000` / `>= 887000` so we catch full-range positions even if they were minted with non-min/max ticks at full-range spacing) → return `isFullRange: true`, `lower: "0"`, `upper: "∞"`, `quoteLabel` per the heuristic below.
  - Otherwise compute `priceLower = tickToPrice(tickLower, ...)` and `priceUpper = tickToPrice(tickUpper, ...)`. These are token1-per-token0.
  - Quote-direction heuristic: if `STABLE_SYMBOLS.has(token0Symbol)` and not `token1Symbol` → invert (so `1/price`, swap lower/upper, label `${token0Symbol} per ${token1Symbol}`). Else if `STABLE_SYMBOLS.has(token1Symbol)` and not `token0Symbol` → keep direction, label `${token1Symbol} per ${token0Symbol}`. Else default to keep, label `${token1Symbol} per ${token0Symbol}`.
  - Format numbers with `formatDisplayBalance` (already used in `portfolio-summary.ts`) or a small inline formatter that picks 2–6 sig figs based on magnitude. Reuse `formatDisplayBalance` to stay consistent.

### 2. `intentOS-ui/lib/portfolio-summary.ts`

- Extend type at lines 312–323:
  ```ts
  export type UniV3PositionSummary = {
      // existing fields...
      priceLower: string;
      priceUpper: string;
      priceQuoteLabel: string;   // e.g. "USDC per WETH"
      isFullRange: boolean;
  };
  ```
  Keep `tickLower`/`tickUpper`/`rangeLabel` so `generate-export-md.ts` still works unchanged.

- In `useUniV3Summary` at lines 866–909, after `token0Decimals`/`token1Decimals` are computed (lines 885–886), call `formatPriceRange(...)` and spread the result into the returned summary object.

### 3. `intentOS-ui/components/portfolio-overlay.tsx`

- Line 332 — replace:
  ```tsx
  <KeyValue label="Range" value={`[${p.tickLower}, ${p.tickUpper}]`} mono />
  ```
  with:
  ```tsx
  <KeyValue
      label="Range"
      value={p.isFullRange ? "Full range" : `${p.priceLower} – ${p.priceUpper}`}
      mono
  />
  {!p.isFullRange ? (
      <div className="text-[10px] text-muted-foreground">{p.priceQuoteLabel}</div>
  ) : null}
  ```
  (Or fold the quote label into a single `KeyValue` value — pick whichever matches surrounding visual rhythm when implementing.)

### 4. `intentOS-ui/lib/generate-export-md.ts` — **no change**

Per the user's decision, ticks stay in the markdown export at line 195 so the LLM can produce precise close/decrease intents.

## Files to NOT modify (explicitly)

- `lib/intent-tool-schema.ts` — already uses prices for new mints.
- `components/finalize-intent-tool.tsx` — already shows prices for `lp_mint` previews.
- `intent-script/` (Rust) — no compiler change needed; this is purely a display-layer fix.

## Existing utilities being reused

- `formatDisplayBalance` from `intentOS-ui/lib/token-balances.ts` — for consistent number formatting.
- `decimalsForSymbol` (local in `portfolio-summary.ts`) — already resolves token decimals.
- `KeyValue` component (already used in `UniV3Section`).

## Verification

1. **Build & types**: `cd intentOS-ui && pnpm tsc --noEmit` — no errors.
2. **Unit-style sanity check** (optional, no test framework wired in): in a scratch node REPL or temporary test, verify:
   - `tickToPrice(0, 18, 18) === 1`
   - `tickToPrice(-200000, 6, 18) ≈ 2.06e-13` (raw direction; heuristic should invert for USDC/WETH)
   - Inverted: `1/tickToPrice(-200000, 6, 18) ≈ 4850` → reads as ~4850 USDC per WETH at that tick
3. **End-to-end UI**: `cd intentOS-ui && pnpm dev`, connect a wallet on Anvil/Sepolia that holds at least one Uniswap v3 LP NFT, open the portfolio overlay, confirm the LP section shows e.g. `1820 – 2150` with `USDC per WETH` underneath instead of `[200000, 210000]`.
4. **Edge cases to eyeball in dev**:
   - A full-range position (mint with `price_lower: "min"`, `price_upper: "max"`) → "Full range".
   - A same-decimal pair (e.g. DAI/WETH, both 18) → no decimal-shift artifacts.
   - A non-stable pair (e.g. WBTC/WETH) → falls through to `WETH per WBTC` default.
5. **Regression check**: confirm the markdown export from "Export portfolio" still shows `ticks [..., ...]` so close/decrease intents the LLM generates still compile.
