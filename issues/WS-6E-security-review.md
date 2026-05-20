# [WS-6E] Security review pass

**Repo:** `partylikeits1983/intentOS-ui` + `partylikeits1983/intentOS-server` + `partylikeits1983/intent-script`
**Labels:** `area/security`, `type/review`, `size/M`
**Depends on:** all of WS-1, WS-2, WS-6, WS-8

## Context

After the Rust API/executor, UI auth/key handling, rate limiting, CORS/CSP, audit logging, and advisor surface work all land, run a structured security review and close out findings before tagging v1.0.

## Scope

1. Run the `security-review` skill against the diff on `main`.
2. Findings report in `docs/security-review-2026-XX.md`:
   - Executive summary.
   - Each finding: severity, description, location, remediation, status.
3. For each HIGH or CRITICAL finding, open a follow-up issue tagged `priority/p0` and block the `v1.0` tag until resolved.
4. Re-run after fixes; attach the clean report.
5. Manual review checklist (on top of the skill):
   - [ ] No secrets in git history (run `gitleaks`).
   - [ ] Dependency audit: `pnpm audit --audit-level=high` clean.
   - [ ] Rust dependency audit: `cargo audit` clean or tracked.
   - [ ] Foundry slither on `IntentRouter` and any other shipped Solidity.
   - [ ] Verify BYOK keys never appear in server logs.
   - [ ] Verify agent API keys hashed at rest, never logged raw.
   - [ ] Verify executor cannot broadcast mutated calldata, expired signatures, wrong-chain payloads, replayed payloads, or payloads whose fee quote changed after signing.
   - [ ] Verify executor private key handling / signer custody story.
   - [ ] Verify `AUTH_SECRET` rotation story documented.
   - [ ] CSP does not break any feature after full run-through.
   - [ ] **Advisor surface**: scan + recommendations never leak balances of other users; advisor LLM responses cannot exfiltrate cookies/tokens via tool-call abuse.

## Files

- `intentOS-ui/docs/security-review-*.md`
- `intentOS-server/docs/security-review-*.md`
- `intent-script/docs/security-review-*.md`
- Follow-up issues linked from the report.

## Acceptance criteria

- [ ] Security review report committed.
- [ ] Zero HIGH/CRITICAL findings open at merge time.
- [ ] `pnpm audit --audit-level=high` passes in CI.
- [ ] `cargo audit` passes in server CI.
- [ ] All MEDIUM findings either fixed or tracked with a plan + ETA.
- [ ] Sign-off note in the PR description: which checks were done, by whom, when.
- [ ] Slither output attached for each shipped Solidity contract.
