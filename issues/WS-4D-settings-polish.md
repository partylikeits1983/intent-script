# [WS-4D] Settings — advisor preferences + execution controls

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/ux`, `area/ui`, `size/M`
**Depends on:** none

## Context

Current settings (`components/model-selector.tsx`) are model-focused. The advisor needs explicit preference inputs (risk tolerance, asset preferences) so its recommendations stay aligned with the user. Execution controls (slippage, gas, network) also belong here for the reactive chat path that the user keeps using.

## Scope

1. Reorganize the settings panel into sections: **Advisor**, **Model**, **Execution**, **Network**, **Advanced**.
2. **Advisor** section (new):
   - Risk tolerance band: conservative / balanced / aggressive (numeric slider for max LTV, max single-position concentration, allowed protocols).
   - Asset whitelist / blacklist (free-form chip input, validated against the registry).
   - Persisted in `hooks/use-advisor-prefs.ts` → `localStorage` *and* (when SIWE'd) round-tripped to a server key/value store so prefs follow the wallet.
3. **Execution** section:
   - Slippage presets: 0.1% / 0.5% / 1% / custom.
   - Default gas behavior: Auto / Manual.
   - Persisted in `hooks/use-execution-settings.ts`.
4. **Network** section:
   - Dropdown listing configured chains; calls `useSwitchChain`.
   - Shows current chain + chainId prominently (ties into WS-3B).
5. Header chrome: surface active network + active LLM model + advisor risk band for at-a-glance visibility.
6. Wire prefs into the call paths:
   - Slippage threads through `lib/intent-compiler.ts` as `user_slippage_bps`.
   - Risk band threads into the advisor scan request (WS-8A).

## Files

- `intentOS-ui/components/settings/*.tsx` (split per section)
- `intentOS-ui/hooks/use-advisor-prefs.ts` (new)
- `intentOS-ui/hooks/use-execution-settings.ts` (new)
- `intentOS-ui/components/header.tsx` (new — promoted chrome)
- `intentOS-ui/lib/intent-compiler.ts` — slippage passthrough
- `intentOS-ui/lib/advisor/scan-client.ts` — risk-band passthrough

## Acceptance criteria

- [ ] Advisor risk band changes the next scan output (observable in recommendation sizing/protocols).
- [ ] Slippage preset persists across reloads and affects compile output.
- [ ] Network switch from header invokes `wallet_switchEthereumChain`.
- [ ] Settings UI fits within a panel; no dedicated page required.
