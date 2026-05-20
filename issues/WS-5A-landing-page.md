# [WS-5A] Landing page at `/` — "AI financial advisor onchain"

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/marketing`, `area/ui`, `size/M`
**Depends on:** WS-4B

## Context

Today `/` is the app. For a production launch, `/` needs to be a marketing surface that frames intentOS as the onchain AI financial advisor it now is — not as an "intent compiler chat". The app moves to `/app`. Landing converts visitors across three audiences: retail, treasury/DAO, and agent developers.

## Scope

1. Move existing app contents:
   - `app/page.tsx` → `app/app/page.tsx`.
   - `app/assistant.tsx` stays (still imported by the moved page).
   - Update internal links that assume `/`.
2. New landing at `app/page.tsx`:
   - **Hero**: "AI financial advisor onchain. Reads your wallet. Makes the call. Executes safely on one signature." CTA → `/app`.
   - **How it works** (3 steps): connect → advisor scans portfolio → review recommendation, sign once.
   - **Two audiences** tabbed section (treasury surface ships later — keep landing tight):
     - *Retail*: "Stop clicking through five protocol UIs. Get an opinionated recommendation, see the simulation, confirm."
     - *Developers*: "Embed the safety compiler. `/api/v1/compile`, `/simulate`, `/execute`. Build hardened DeFi agents without writing your own calldata layer."
   - **Demo personas preview** (uses WS-4B's demo-persona renderer) — interactive proof that visitors can poke before committing.
   - **Safety section**: recipient pinning, slippage caps, health-factor checks, deterministic simulation. Why an LLM advisor on real money is only useful if execution is provably safe.
   - **FAQ + footer**: links to `/docs`, GitHub, Twitter.
   - OG metadata.
3. Landing uses edge runtime; app stays on node.
4. Redirects via `middleware.ts`: `/assistant`, `/chat` → `/app`.
5. Assets: OG image at `public/og.png` (1200×630); favicon set.

## Files

- `intentOS-ui/app/page.tsx` (rewrite as landing)
- `intentOS-ui/app/app/page.tsx` (new — contains previous app body)
- `intentOS-ui/app/app/layout.tsx` (new if needed for Web3Provider wrapper)
- `intentOS-ui/middleware.ts` — redirects (coordinates with WS-6B / WS-6C)
- `intentOS-ui/components/landing/{Hero,HowItWorks,AudienceTabs,DemoPreview,SafetySection,Faq,Footer}.tsx` (new)
- `intentOS-ui/public/og.png`, `public/favicon-*.png` (new)

## Acceptance criteria

- [ ] `/` renders landing; `/app` renders the existing chat UI.
- [ ] Two audience tabs each link to relevant deeper content (`/app`, `/docs`).
- [ ] Demo personas preview is interactive on landing.
- [ ] Lighthouse performance ≥ 90 on landing.
- [ ] OG card renders correctly when URL is pasted into Twitter/Discord.
- [ ] No dead internal links in the moved code.
- [ ] Smoke e2e updated to point at `/app`.
