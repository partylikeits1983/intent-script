# [WS-4F] Transaction review and risk panel

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/ux`, `area/security`, `area/defi`, `size/M`
**Depends on:** WS-7B, WS-7C

## Context

The preview card shows that a transaction is executable, but production users also need decision-grade risk context before signing complex DeFi transactions. "Simulation passed" is not enough for leverage, LP ranges, bridging, approvals, or position-closing flows.

## Scope

1. Upgrade the review surface with explicit sections:
   - `You have now`
   - `You will end up with`
   - `Actions`
   - `Approvals`
   - `Risks`
   - `Costs`
2. Add protocol-specific risk rows:
   - Aave/Morpho: health factor before/after, liquidation threshold, debt change.
   - Uni V3 LP: price range, current price relative to range, out-of-range behavior.
   - Lido: withdrawal queue delay and claim requirement.
   - Across: destination chain, recipient, relayer fee, fill deadline.
   - Swaps: route, expected output, minimum output, slippage percent.
3. Add hard confirmation guards for high-risk actions:
   - leverage above configured threshold;
   - health factor below configured threshold;
   - bridge recipient not equal to connected wallet;
   - unusually high slippage;
   - approval amount materially above spend amount.
4. Preserve advanced details in collapsible panels:
   - raw intent JSON;
   - calldata targets/selectors;
   - simulation trace or fallback reason.

## Files

- `intentOS-ui/components/intent-preview-card.tsx`
- `intentOS-ui/components/transaction-card.tsx`
- `intentOS-ui/components/tx-sequence-card.tsx`
- `intentOS-ui/components/risk-panel.tsx` (new)
- `intentOS-ui/lib/risk-policy.ts` (new)
- `intentOS-ui/lib/simulate-transaction.ts`

## Acceptance criteria

- [ ] Every preview shows asset deltas, approvals, gas estimate, and risk rows before the Confirm button.
- [ ] Aave/Morpho borrow and leverage flows show before/after health-factor data when available.
- [ ] Bridge flows show destination chain and recipient prominently and require extra confirmation when the recipient differs from the wallet.
- [ ] High-risk policy violations block one-click execution and require an explicit acknowledgement.
- [ ] Raw JSON/calldata remains available but is not the primary review experience.
