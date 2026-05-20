# [WS-3A-UI] CI pipeline for intentOS-ui

**Repo:** `partylikeits1983/intentOS-ui`
**Labels:** `area/ci`, `size/M`
**Depends on:** none

## Context

No `.github/workflows/` exists. We need CI that runs on every PR: lint, typecheck, WASM build, Next build, and the Playwright smoke suite. Ideally also the full-stack e2e once WS-3C lands.

## Scope

1. Add `.github/workflows/ci.yml` with jobs:
   - `lint` — `pnpm lint`
   - `typecheck` — `pnpm tsc --noEmit`
   - `build-wasm` — `pnpm build:wasm` (sets up Rust, wasm-pack, uses `intent-script` as a git submodule or cloned sibling — see below)
   - `build` — `pnpm build`
   - `test-e2e-smoke` — `pnpm exec playwright install chromium && pnpm test:e2e e2e/smoke.spec.ts`
   - (Optional post-3C) `test-e2e-full` — spins up anvil, runs full-stack spec
2. WASM builds need the `intent-script` repo checked out as a sibling. Two options:
   - Submodule (cleaner): add `intent-script` as a submodule, CI runs `git submodule update --init`.
   - Clone-by-tag (simpler): CI step checks out `partylikeits1983/intent-script@<tag>` into `../intent-script`. Pin the tag in `package.json` or a `.intent-script-ref` file.
3. Caching: `actions/cache` keyed on `pnpm-lock.yaml`, `Cargo.lock`, and the intent-script ref.
4. Branch protection: require these checks to pass before merging to `main`.
5. Status badge in the README.

## Files

- `intentOS-ui/.github/workflows/ci.yml` (new)
- `intentOS-ui/.gitmodules` (if submodule chosen)
- `intentOS-ui/README.md` (badge)
- `intentOS-ui/.intent-script-ref` (if clone-by-tag chosen)

## Acceptance criteria

- [ ] Every PR triggers the workflow.
- [ ] All jobs pass on `main` as of this PR's merge.
- [ ] Green CI badge visible in README.
- [ ] Branch protection configured (document the setting in the PR body even if we can't set it via code).
- [ ] Failed workflow leaves inline annotations on the PR.
