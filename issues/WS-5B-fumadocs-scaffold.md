# [WS-5B] Fumadocs scaffold + core concepts

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/docs`, `size/M`
**Depends on:** none

## Context

No docs site exists. Fumadocs (Next.js-native, MDX) colocated in `intentOS-ui` at `/docs` — one deploy target, shared styling.

## Scope

1. Install `fumadocs-ui`, `fumadocs-core`, `fumadocs-mdx`.
2. Scaffold the docs route group:
   - `app/docs/[[...slug]]/page.tsx`
   - `app/docs/layout.tsx`
   - `content/docs/*.mdx` — MDX source
   - `source.config.ts` — Fumadocs config
3. Core pages (MDX):
   - `introduction.mdx` — what intentOS is, who it's for
   - `quickstart.mdx` — placeholder; real content lands in WS-5D
   - `concepts/intents.mdx` — what an intent is
   - `concepts/compiler.mdx` — pipeline: parse → normalize → validate → preview → enrich → lower → plan → build
   - `concepts/execution.mdx` — single-tx vs EIP-712 vs sequence modes
   - `concepts/safety.mdx` — recipient pinning, sweep, allowlist
   - `guides/local-development.mdx` — how to run anvil + the UI, with the fork-vs-mainnet explainer from WS-3B
   - `guides/supported-protocols.mdx` — generated from `lib/config/protocols-*.json` (list, capabilities, caveats)
   - `security.mdx` — threat model: BYOK browser-local, SIWE sessions, API-key handling
4. Sidebar nav generated from folder structure.
5. Link from the landing page footer (WS-5A) and from the in-app settings.

## Files

- `intentOS-ui/app/docs/[[...slug]]/page.tsx` (new)
- `intentOS-ui/app/docs/layout.tsx` (new)
- `intentOS-ui/content/docs/*.mdx` (new)
- `intentOS-ui/source.config.ts` (new)
- `intentOS-ui/next.config.ts` — MDX plugin if needed
- `intentOS-ui/package.json` — new deps

## Acceptance criteria

- [ ] `/docs` renders with Fumadocs chrome, navigable sidebar, search.
- [ ] All nine initial pages render without MDX errors.
- [ ] Protocol list in `supported-protocols.mdx` is generated (not hand-written) so it stays in sync.
- [ ] Dark/light mode respected.
- [ ] Search index builds in CI.
