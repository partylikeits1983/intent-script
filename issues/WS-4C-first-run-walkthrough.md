# [WS-4C] First-run walkthrough — connect → scan → first recommendation

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/ux`, `area/onboarding`, `size/S`
**Depends on:** WS-8A, WS-11B

## Context

A new user lands on `/app`, connects, and the advisor produces a first recommendation. The walkthrough orients them to that flow rather than tutorializing a chat box. Three steps: (1) connect, (2) advisor scans, (3) review and confirm a recommendation. The fallback for users without an LLM key is a hand-off to the manual ChatGPT flow (WS-11B).

## Scope

1. `components/onboarding-tour.tsx` (rewrite):
   - Step 1 — highlights Connect button: "Connect to let the advisor scan your portfolio."
   - Step 2 — appears mid-scan, highlights the streaming advisor message: "Reading your wallet, looking for opportunities..."
   - Step 3 — highlights the first recommendation card: "Nothing executes until you confirm the preview."
   - For users with no API key + no server LLM: Step 2 instead points at the manual ChatGPT flow card.
2. Persisted via `localStorage["intentos-onboarded"] = "1"`. Re-openable from settings.
3. Accessibility: keyboard-navigable, ESC dismisses, focus traps respected.
4. Dovetails with WS-4B demo personas: a visitor who clicked through a persona then connected continues into the same tour.

## Files

- `intentOS-ui/components/onboarding-tour.tsx`
- `intentOS-ui/app/app/page.tsx`
- `intentOS-ui/components/settings/general-panel.tsx` — "Show tour again"

## Acceptance criteria

- [ ] Tour appears once on a fresh browser; dismissal persists.
- [ ] Tour adapts step 2 for manual-ChatGPT users.
- [ ] "Show tour again" reopens the tour.
- [ ] Keyboard-navigable; ESC dismisses.
- [ ] No regression to existing smoke e2e.
