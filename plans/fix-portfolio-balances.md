# Fix portfolio modal: ERC20 balances not displaying

> Note: per saved memory, durable plans should live in `intent-script/plans/`. After exit-plan-mode I'll move this file there. The plan-mode harness only allowed editing this single path during planning.

## Context

The "Balances" button (bottom-left sidebar) opens the portfolio overlay (`components/portfolio-overlay.tsx`). Right now it shows only ETH — none of the ERC20 balances (USDC/USDT/DAI/etc.) appear, even though the user is connected with a default anvil dev account (e.g. `0xf39Fd6e...`) and is running `intent-script/scripts/start-anvil.sh`, which mints 100k USDC + 100k USDT to those accounts via `anvil_setStorageAt`.

Confirmed from dev console: the per-second invalidate ticks fire (`[intentos.balance] interval_invalidate`, `[intentos.portfolio] interval_invalidate {keys: 7}`) — so the queries ARE executing. The balances they return are `0`, and `lib/portfolio-summary.ts:332` filters out `"0"`/`"0.0"` rows, which is why they vanish from the modal.

The wagmi config is correct on paper:
- `intentOS-ui/.env.local` has `NEXT_PUBLIC_USE_LOCAL_FORK=true`.
- `lib/wagmi-config.ts:22-37` reroutes `transports[mainnet.id]` to `http://127.0.0.1:8545` when that flag is on. `useBalance` and `useReadContracts` calls pinned to `chainId: mainnet.id` should hit anvil.

So the live failure modes are:
- **B (most likely now)** — `start-anvil.sh`'s storage-slot mint isn't actually landing on the addresses the UI is querying. USDC slot 9 / USDT slot 2 are correct for the current proxy implementations, but the script may have other issues (e.g. mint happens before fork is ready, or the AMOUNT_HEX encoding doesn't match what `cast index address` expects, or fork inheritance is overwriting the slot).
- **D** — `useReadContracts` is returning per-call `status: "failure"` (multicall partial failure), which the hook silently maps to `"0"` (`lib/token-balances.ts:142-150`).
- **C (unlikely)** — transport misroute. We'll confirm with the diagnostic.

## Phase 1 — Diagnose

### 1a. Add dev-only logging to `lib/token-balances.ts`
Faster than the standalone script, and tells us exactly which mode the failure is in.

After the `useReadContracts` call (around `lib/token-balances.ts:102`), in dev mode log:
- `address` (connected account)
- For each contract: `result.status`, `result.error?.shortMessage`, `result.result`

This will distinguish:
- **success + result=0n** → mint never landed → fix is in `start-anvil.sh`
- **failure + error** → multicall/RPC issue → fix is in transport or chain config
- **never fires** → query not enabled / address undefined

Gate on `process.env.NODE_ENV !== "production"` and a one-line `console.log("[intentos.balance] reads", { address, results: [...] })`.

### 1b. Write `intent-script/scripts/check-balances.mts`
Standalone TypeScript diagnostic that does NOT depend on the UI:

```ts
// runnable via: pnpm tsx intent-script/scripts/check-balances.mts [extraAddr...]
import { createPublicClient, http, formatUnits, erc20Abi } from "viem";
import { mainnet } from "viem/chains";

const RPC = process.env.RPC_URL ?? "http://127.0.0.1:8545";
const TOKENS = {
  WETH: { address: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", decimals: 18 },
  USDC: { address: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", decimals: 6  },
  USDT: { address: "0xdAC17F958D2ee523a2206206994597C13D831ec7", decimals: 6  },
  DAI : { address: "0x6B175474E89094C44Da98b954EedeAC495271d0F", decimals: 18 },
  WBTC: { address: "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599", decimals: 8  },
} as const;

const DEV_ACCOUNTS = [
  "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
  "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
  // ...10 total
];

// For each (account, token): fetch ETH balance + multicall balanceOf
// Print a table. Flag any dev account where USDC or USDT == 0 — that's the bug.
```

Key checks the script must do:
1. `client.getChainId()` — confirm we're on 31337.
2. `client.getBlockNumber()` — confirm fork is alive.
3. For each dev account: ETH balance + multicall'd `balanceOf` for every token.
4. Same readContracts call shape the UI uses (`functionName: "balanceOf"`, single multicall via the chain's multicall3).
5. Optional: read slot 9 of USDC and slot 2 of USDT directly via `client.getStorageAt` for `keccak256(abi.encode(account, slot))` — confirms whether the mint storage write actually landed.

This script is the source of truth: if it shows 0 USDC for a dev account, the mint script is broken; if it shows 100k USDC, the UI is broken.

## Phase 2 — Apply the minimal fix (per user: "minimal fix")

Branch on the diagnostic result:

### If mint didn't land (most likely)
Fix `intent-script/scripts/start-anvil.sh`. Two minimal options:
- **Verify storage write**: re-read the slot after writing; if zero, error out. Currently the script silently succeeds even if the RPC call no-ops.
- **Fall back to whale impersonation** (already proven in `scripts/run-local-anvil.sh:182-199`). Port the `fund_token` function. This avoids the storage-slot fragility entirely.

Recommendation: switch the mint to whale impersonation. It's a ~30-line addition, mirrors the other script, and is robust against any future USDC/USDT proxy upgrade. Also add DAI to the mint list (currently missing — only USDC + USDT).

### If multicall returns failures
Likely the chain entry's multicall3 isn't being used because we pinned `chainId: mainnet.id` but the request actually requires the chain object's `contracts.multicall3` to be present. Wagmi already configures this for `mainnet`, so this is unlikely. If it triggers: pass `multicallAddress: "0xcA11bde05977b3631167028862bE2a173976CA11"` explicitly via the transport, or read each balance with separate `useReadContract` calls.

### If transport routing is wrong
Confirm `NEXT_PUBLIC_USE_LOCAL_FORK=true` is read at dev-server start (env vars baked in at `next dev` boot). Restart `pnpm dev` after any `.env.local` edit.

## Phase 3 — Verify end-to-end

1. `cd intent-script && ./scripts/start-anvil.sh` — leave running.
2. `pnpm tsx intent-script/scripts/check-balances.mts` — should print non-zero USDC/USDT for all 10 dev accounts.
3. `cd intentOS-ui && pnpm dev` — restart so env vars reload.
4. Open the UI, connect MetaMask with the dev account `0xf39Fd6e...` private key, switch wallet to chain id 31337.
5. Click the "Balances" sidebar button → portfolio modal should now show ETH + USDC + USDT (and DAI if added in Phase 2).
6. Smoke-test live updates: `cast send` a USDC transfer to a different anvil account; confirm the modal balance decreases within ~1s (the existing 1s invalidate tick).

## Files touched

- `intentOS-ui/lib/token-balances.ts` — add ~5-line dev-only diagnostic log around the `useReadContracts` result. Remove or gate behind a flag once root cause is fixed.
- `intent-script/scripts/check-balances.mts` — **new**, ~80 lines. Diagnostic only, never imported by the app.
- `intent-script/scripts/start-anvil.sh` — fix the mint (likely: switch to whale impersonation; add DAI). Largest change in the plan.
- (Possibly) `intentOS-ui/lib/portfolio-summary.ts:332` — leave the `"0"` filter as is; once balances are non-zero, rows render naturally.

Existing utilities reused:
- `intent-script/scripts/start-anvil.sh:36-48` — the dev-account list (don't redefine).
- `scripts/run-local-anvil.sh:182-199` — `fund_token()` whale-impersonation pattern (port shape, not wholesale copy).
- `intentOS-ui/lib/token-balances.ts:18-26` — `TOKEN_CONTRACTS` (the diagnostic script could even import it directly to guarantee parity with the UI).

## Out of scope

- Not changing the wagmi chain config or removing the `mainnet.id` pin in `lib/token-balances.ts` — both are working as designed for the local-fork setup.
- Not touching Aave / Morpho / UniV3 / Lido sections of the modal — those will start populating once the wallet has tokens deposited there. The reported bug is wallet-balance only.
- Not consolidating the two anvil scripts (per user: minimal fix).
