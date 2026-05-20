# [WS-5D] Developer quickstart + cookbook

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/docs`, `size/S`
**Depends on:** WS-1C, WS-1D, WS-5B, WS-8A

## Context

The agent-developer audience needs a path from zero to a working integration in under five minutes. With the advisor surface (WS-8A) live, the cookbook should reflect the high-leverage patterns: compile a recommendation, simulate it, and either execute through the user's wallet or submit it back to `/execute`.

## Scope

1. Quickstart (`content/docs/quickstart.mdx`):
   - SIWE + API key issuance.
   - `curl` example: `/api/v1/compile` for a USDC → Aave V3 supply intent.
   - `curl` example: `/api/v1/simulate` for the compiled tx.
   - `curl` example: `/api/v1/advisor/scan` for a sample wallet.
   - JS snippet using `fetch` that does scan → compile → simulate → render.
2. Cookbook (`content/docs/cookbook/`):
   - **`compile-only-integration.mdx`** — "use intentOS as a calldata layer" pattern for teams that just want hardened calldata.
   - **`advisor-integration.mdx`** — call `/advisor/scan` for a wallet, render the recommendation, hand the compiled tx to the user's wallet for signing.
   - **`yield-screener.mdx`** — read `/api/v1/yields`, present opportunities to a user; on choice, compile + simulate a deposit.
3. Each cookbook recipe links to runnable scripts in a small companion repo (`intentos-js-sdk`), tracked separately.

## Files

- `intentOS-ui/content/docs/quickstart.mdx`
- `intentOS-ui/content/docs/cookbook/*.mdx` (new)

## Acceptance criteria

- [ ] Quickstart curl commands work as documented against staging.
- [ ] Each cookbook recipe is end-to-end runnable (Node or TS).
- [ ] All external links (DSL spec, protocol docs) resolve.
- [ ] Examples are tested locally against anvil before merging (note in PR body).
