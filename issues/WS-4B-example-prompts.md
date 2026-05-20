# [WS-4B] Demo portfolio scenarios for visitors without a wallet

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/ux`, `area/ui`, `area/marketing`, `size/M`
**Depends on:** WS-8A

## Context

The advisor's value is in opening with a proactive recommendation tied to the user's portfolio. A visitor without a wallet sees nothing. Slash commands and "type these prompts" chips don't translate well — the surface is now advisor-driven, not prompt-driven. Replace the example-prompts UX with **demo portfolio scenarios**: fixed personas the visitor can step into to see what the advisor would say.

## Scope

1. Static demo-persona catalog in `lib/demo-personas.ts`:
   - **Idle USDC holder** — $42k USDC, no DeFi positions.
   - **Mixed DeFi user** — wstETH + Aave supply + small Uni LP.
   - **Leverage holder** — Aave borrow at moderate utilization, looking for risk advice.
   - **Bridging stables** — USDC on mainnet looking for cheaper deployment elsewhere.
2. Each persona freezes balances, positions, and price assumptions (no live data needed) and pre-renders the WS-8A scan output as a static fixture.
3. `components/demo-personas.tsx`:
   - Visible on landing page (`/`) and on `/app` when no wallet is connected.
   - Picking a persona renders the canned advisor recommendation set as a read-only thread (no execute affordances; "Connect wallet to run this for real").
4. Data integrity: demo recommendations use only data baked into the persona — no API calls, no fabrication.
5. Slash commands and small prompt chips are removed; reactive chat still accepts free-form input as before.

## Files

- `intentOS-ui/lib/demo-personas.ts` (new)
- `intentOS-ui/components/demo-personas.tsx` (new)
- `intentOS-ui/components/demo-thread.tsx` (new — read-only thread)
- `intentOS-ui/app/page.tsx` — landing renders demo personas (coordinates with WS-5A)
- `intentOS-ui/app/app/page.tsx` — render demo personas before wallet connect

## Acceptance criteria

- [ ] Visitor without a wallet sees the demo personas on landing and on `/app`.
- [ ] Picking a persona renders a deterministic, read-only thread of advisor recommendations.
- [ ] No execute affordances are shown in demo mode.
- [ ] Demo content has no live network calls — fully static.
- [ ] Reactive-chat composer still works for connected users; slash-command UX is removed without regression.
