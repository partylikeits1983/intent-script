# [WS-8B] Watch / alerts / nudges

**Repo:** `partylikeits1983/intentOS-server` + `partylikeits1983/intentOS-ui`
**Labels:** `area/data`, `area/llm`, `area/observability`, `size/L`
**Depends on:** WS-7B, WS-8A

> **Phase: v1 (post-MVP).** Per `PRODUCT.md`, position monitoring + nudges are v1, not MVP. Track but do not start until the MVP advisor surface ships.

## Context

The advisor maintains an ongoing relationship — "yields shift, utilization spikes, oracle prices drift, opportunities emerge — each becomes a useful nudge with a one-signature rebalance attached". The scan in WS-8A is a snapshot; this issue is the watcher that keeps producing useful nudges between sessions. Without it, the advisor is a one-shot tool.

## Scope

1. Server-side watch loop (`intentOS-server`):
   - Background tokio task per opted-in wallet/chain: poll WS-7B data feeds + WS-7C portfolio context every N minutes (config-driven; default 15m).
   - Detect threshold-crossing events: yield-drift > X bps, utilization > Y%, health-factor < Z, oracle-deviation > W%, expiring claim windows (Lido), new vault opportunities materially better than current allocation.
   - Each event materializes as a typed `Nudge { kind, reason, evidence, recommendation: AdvisorScan-compatible }`.
2. Storage:
   - Persist nudges per wallet in Postgres/Redis (decided in WS-0A; reuse).
   - Idempotent: same event within N hours collapses, doesn't re-emit.
3. Delivery channels:
   - In-app: badge on `/app` header, panel listing pending nudges with one-sig "Apply rebalance" affordance.
   - Email (optional, off by default): one digest per day.
   - Webhook (developer tier): POST to a registered URL.
4. Endpoints:
   - `GET /api/v1/advisor/nudges` — list pending nudges for the authed wallet.
   - `POST /api/v1/advisor/nudges/{id}/dismiss` — user dismissal.
   - `POST /api/v1/advisor/watch` — opt in / out per wallet+chain.
5. UI surfaces:
   - Header badge with count.
   - `components/advisor/nudges-panel.tsx` listing nudges with reasoning, evidence, and "Apply" or "Dismiss".
   - Apply path reuses the existing recommendation-card → preview → sign flow.

## Files

- `intentOS-server/src/watch/loop.rs` (new)
- `intentOS-server/src/watch/triggers.rs` (new)
- `intentOS-server/src/routes/nudges.rs` (new)
- `intentOS-server/src/store/nudges.rs` (new)
- `intentOS-ui/components/advisor/nudges-panel.tsx` (new)
- `intentOS-ui/components/advisor/nudge-card.tsx` (new)
- `intentOS-ui/lib/advisor/nudges-client.ts` (new)
- `intentOS-ui/components/header.tsx` — badge

## Acceptance criteria

- [ ] A watched wallet whose Aave health factor crosses 1.3 produces exactly one nudge within 2× poll interval.
- [ ] A watched wallet sees no spam — same trigger collapses for at least 6h after first emission.
- [ ] Apply path executes the rebalance through the existing compile→sign flow with the *same* safety checks (no shortcut).
- [ ] Dismiss persists across reloads.
- [ ] Email and webhook channels off by default; opt-in flows documented and tested.
- [ ] Watch loop survives server restart (state recoverable from store).
