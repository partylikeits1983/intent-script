# Sub-Task 09 — Phase 8: Final Integration

## Context

The previous sub-tasks each touched a slice of the system. This one ties everything together: documentation, deploy allowlist, end-to-end LLM smoke test, and fixture regeneration.

## Prerequisites

- Sub-tasks 02-08 complete.
- All `make test`, `make test-foundry`, and `make test-fork-e2e` targets green per their respective sub-task's verification.

## Implementation

### 9.1 Update skills documentation

The `intent-script/skills/` directory is agent-facing context — the LLM that writes intents reads it. Update:

**`skills/codebase-status.md`:**
- Update supported-protocols table with the new entries: Morpho Blue, Balancer V2 (flashloan), Uniswap V3 LP, Lido withdrawal queue, Across V3.
- Update test counts.
- Note the router fee mechanism (10 bps default, 24h timelock, 100 bps cap).

**`skills/json-dsl-spec.md`:**
- Add new step type sections in alphabetic order:
  - `### bridge — Across Cross-Chain Transfer`
  - `### claim_withdrawal — Lido Withdrawal Claim`
  - `### flashloan — Balancer V2 Flashloan with Inner Pipeline`
  - `### lp_collect — Uniswap V3 LP Collect Fees`
  - `### lp_decrease — Uniswap V3 LP Decrease Liquidity`
  - `### lp_increase — Uniswap V3 LP Increase Liquidity`
  - `### lp_mint — Uniswap V3 LP Mint`
  - `### request_withdrawal — Lido Withdrawal Request`
- Extend `### deposit`, `### borrow`, `### withdraw` to document the optional `market` field for Morpho and the optional `as: "collateral"` discriminator.
- Extend `### unwrap` to document `wstETH` as a valid asset.
- **Add a top-level callout near the existing `### send` section reaffirming that ERC20 (`asset_type: "erc20"`), native ETH, and ERC721 (`asset_type: "erc721"`) transfers are first-class supported step types.** This is the explicit statement the user asked for in the original brief.
- Update `## Validation Rules` with the new bounds:
  - `relayer_fee_bps ≤ 50` for bridges
  - flashloan max 5 inner steps, depth 1
  - LP fees restricted to {500, 3000, 10000}
  - LP `min_amount0 > 0 || min_amount1 > 0`

**`skills/llm-intent-generation.md`:**
- New action recipes:
  - "Open an LP position in Uniswap V3"
  - "Leveraged loop on Aave via Balancer flashloan"
  - "Bridge stablecoins via Across"
  - "Supply collateral and borrow on Morpho"
  - "Request Lido withdrawal"
  - "Send ERC20 / Send NFT" (these are reaffirmations, not new — the LLM should know about them)
- Each recipe: 1-2 prose sentences + a minimal example JSON.

**`skills/adding-new-adapters.md`:**
- Add appendix sections:
  - "Pattern: NFT custody and the `recipient=signer` shortcut"
  - "Pattern: Recursive enrich for flashloan-style steps"
  - "Pattern: Config-keyed markets (Morpho)"
  - "Pattern: User-prerequisite NFT approval (Uniswap V3 LP decrease/collect)"

### 9.2 Update router allowlist deployment

Find the deploy script under `contracts/script/`. If the allowlist is constructor-seeded or set in the script, add the new targets:

- Balancer Vault: `0xBA12222222228d8Ba445958a75a0704d566BF2C8`
- Morpho Blue: `0xBBBBBbbBBb9cC5e90e3b3Af64bdAF62C37EEFFCb`
- Uniswap V3 NonfungiblePositionManager: `0xC36442b4a4522E871399CD717aBDD847Ab11FE88`
- Lido Withdrawal Queue: `0x889edC2eDab5f40e902b864aD4d7AdE8E412F9B1`
- Across SpokePool: `0x5c7BCd6E7De5423a257D81B442095A1a6ced35C5`

If the script reads addresses from a JSON file, update the JSON.

### 9.3 End-to-end LLM round-trip smoke

Manually prompt a Claude agent (Sonnet or Opus) using the updated `skills/llm-intent-generation.md` as the system prompt. Feed each of these test prompts:

1. "Open a USDC/ETH LP position at 0.3% fee between $1800 and $2200 USDC/ETH"
2. "3× leveraged ETH on Aave using a Balancer flashloan"
3. "Supply 5000 USDC to Morpho USDC/WETH market"
4. "Request withdrawal of 1 stETH from Lido"
5. "Bridge 1000 USDC to Arbitrum via Across"
6. "Send 100 USDC to 0xabc…"
7. "Send NFT #12345 from Uniswap V3 NPM to 0xdef…"

Each must produce JSON that compiles cleanly via `cargo run -p intent-script -- <file>`. Capture failures and adjust prompts/recipes/spec until all 7 pass.

### 9.4 Regenerate fixtures

```bash
cd /Users/riemann/Desktop/intentOS/intent-script
make generate-fixtures
```

Commit the new fixture files.

### 9.5 Final verification

```bash
cd /Users/riemann/Desktop/intentOS/intent-script
make test && make test-foundry && ETH_RPC_URL=… make test-fork-e2e
```

All green = done.

## Definition of done

- [ ] All four skills files updated.
- [ ] Skills explicitly document that ERC20 + ERC721 transfers are supported.
- [ ] Deploy allowlist includes all new protocol addresses.
- [ ] LLM smoke test passes for 7/7 prompts.
- [ ] Fixtures regenerated and committed.
- [ ] `make test && make test-foundry && make test-fork-e2e` green end-to-end.
- [ ] Update `README.md` in this directory: change all sub-task statuses to `✅ DONE (<date>)`.

## Verification

See 9.5.

## Hand-off

Mission complete. The intent-script compiler now covers: ERC20/ETH/NFT sends (already supported), wrap/unwrap (incl. wstETH), Aave V3 supply/borrow/withdraw, Lido stake + wstETH wrap + withdrawal queue, Uniswap V3 swap + LP, 1inch swap, Morpho Blue lending, Balancer V2 flashloans for leverage, and Across V3 bridging — with router-level fees behind a 24h timelock.
