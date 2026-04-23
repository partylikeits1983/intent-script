# Sub-Task 10 — Phase 9: Leverage Trading (Longs & Shorts on Aave)

## Context

Sub-task 06 ships the generic `flashloan` primitive (Balancer V2 Vault, 0% fee) with recursive enrichment. This sub-task builds **high-level leverage sugar** on top of it so an LLM can emit a one-line intent like:

```json
{ "long":  { "collateral": "ETH",  "amount": "1.0",   "leverage": "5", "slippage": "50" } }
{ "short": { "collateral": "USDC", "amount": "3000",  "leverage": "4", "slippage": "50" } }
```

The compiler expands that into the same `flashloan → deposit → borrow → swap → (implicit repay)` pipeline that a power-user could write by hand via the sub-task 06 block.

### Math in one paragraph

An Aave V3 asset with loan-to-value `L` lets you borrow `L × collateral_value`. Recursive looping gives a geometric series `1 + L + L² + … = 1/(1 - L)` as the theoretical max exposure. At 80% LTV (e.g. ETH as collateral), the max is `5×`; at 75% (most stables), `4×`. Flashloan-assisted leverage reaches that max in **one** transaction instead of N loops:

```
flashloan F        = (leverage − 1) × collateral_in
supply_total       = leverage × collateral_in
borrow_amount      = F × price(borrow/collateral) × (1 + slippage + swap_fee)
swap borrow→collateral
repay flashloan    = F  (Balancer: 0 fee; Aave fallback: +5 bps)
```

Defaults: **3× conservative** (safe for both assets), max **5× on long / 4× on short**, hard-capped at `1/(1 − LTV) − safety_margin`. Liquidation price is surfaced in the preview so the user sees the risk before signing.

### Why Balancer first, Aave fallback

| Provider  | Fee    | Liquidity           | Notes                                    |
|-----------|--------|---------------------|------------------------------------------|
| Balancer V2 | 0 bps | Vault-balance-bound | Primary. Free money for the user.        |
| Aave V3     | 5 bps | Deep                | Fallback when Balancer can't fill.       |

v1 ships with Balancer only. Aave fallback is a follow-up (§ Hand-off below). The DSL surfaces the `via` knob today so future opt-in is a zero-DSL-breakage change.

## Prerequisites

- **Sub-task 06 complete** (Balancer flashloan primitive, recursive enrich, reentrancy guard). This sub-task is a DSL+normalize layer on top.
- Sub-task 03 complete (`step_produces` fee-aware — important so the inner pipeline's repayability check is correct when the router fee is non-zero).
- Sub-task 02 complete (router reentrancy guard, `onERC721Received`).

## Files to read first

- `plans/expand-defi-coverage/06-phase5-balancer-flashloan-aave-loop.md` — the foundation. Don't re-implement any of that here.
- `plans/expand-defi-coverage/00-corrections.md` §5, §6 — use boolean-sentinel flashloan guard and `Vec<ResolvedStep>` (not lowered calls) for inner pipeline.
- `crates/intent-script/src/compiler/normalize.rs` — for the expansion pattern.
- `crates/intent-script/src/registry/loader.rs` — for LTV-table loading.
- `crates/intent-script/src/adapters/aave_v3.rs` — supply/borrow/repay/withdraw primitives already exist; we compose them.

## Global rules (inherited)

- Slippage protection is mandatory on every swap inside the pipeline — enforced by existing validate pass.
- Max 5 outer steps, max 5 inner steps, max flashloan depth 1 (inherited from sub-task 06).
- Deterministic compile: no RPC reads, no `SystemTime::now()`.
- Allowlist-gated: Balancer Vault + Aave Pool + Uniswap SwapRouter must all be in the deploy allowlist.

## Design summary

Two new DSL steps, **zero** new IR variants. The normalize pass desugars `long`/`short` into the existing `ResolvedStep::BalancerFlashloan { inner_steps: vec![supply, borrow, swap] }` tree that sub-task 06 ships. Close-position similarly desugars into `BalancerFlashloan { inner_steps: vec![repay, withdraw, swap] }`.

This keeps the IR flat, reuses all the validate/enrich/lower plumbing from sub-task 06, and makes the sugar trivial to audit (`cargo test` can diff the expanded IR against a hand-written equivalent).

## Implementation

### 10.1 Config — per-asset LTV table

Add to `config/protocols/ethereum.json` and `anvil.json`:

```json
"aave": {
  "type": "lending", "version": "v3",
  "contracts": { "pool": "0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2" },
  "ltv_bps": {
    "WETH":   8000,
    "WBTC":   7300,
    "USDC":   7700,
    "USDT":   7500,
    "DAI":    7700,
    "wstETH": 7500,
    "stETH":  7500
  },
  "max_leverage_safety_margin_bps": 300
}
```

`ltv_bps` values are Aave V3 mainnet as of **2026-04-23** — frozen in config because (a) the compiler has no RPC and (b) Aave governance can change them. The deploy pipeline is responsible for keeping this current; stale values only cause the compiler to be *more* conservative, never less.

`max_leverage_safety_margin_bps` (default 300 = 3%) is subtracted from the theoretical max so a user at 5× isn't one block from liquidation. Max effective leverage:

```
max_leverage = 1 / (1 − (ltv_bps − safety_margin) / 10_000)
             = 10_000 / (10_000 − ltv_bps + safety_margin)
```

At `ltv_bps = 8000`, `safety_margin = 300` → `max_leverage = 10_000 / 2_300 ≈ 4.35×`. Hmm — that's below the 5× target. Two options, decide in review:

- **A (implemented below):** shrink `safety_margin` to `0` by default; require user to pass `safety_margin_bps` explicitly if they want slack. The raw LTV floor (80% → 5×) is honored.
- **B:** keep the 3% margin. Document "max 5× long" as a *nominal* ceiling, real cap ~4.35×.

Recommendation: **A**. The slippage param already encodes the user's safety appetite at the swap level; adding a second hidden margin causes confusion. Config ships `max_leverage_safety_margin_bps: 0` with an explanatory comment.

### 10.2 DSL — two new steps

```json
{ "long": {
    "collateral": "ETH",      // asset deposited + gained exposure to
    "borrow":     "USDC",     // borrowed asset to short against (optional; default: "USDC")
    "amount":     "1.0",      // user's initial contribution, in `collateral` units
    "leverage":   "5",        // final exposure multiplier: 1.0 = no leverage, 5.0 = max for 80% LTV
    "slippage":   "50",       // max swap slippage in bps (default: 50 = 0.5%)
    "via":        "balancer"  // flashloan provider (default: "balancer")
} }

{ "short": {
    "collateral": "USDC",     // stable deposited as margin
    "borrow":     "WETH",     // asset shorted (borrowed then sold)
    "amount":     "3000",     // user's initial contribution, in `collateral` units
    "leverage":   "4",        // exposure multiplier
    "slippage":   "50",
    "via":        "balancer"
} }

{ "close_position": {
    "collateral":         "ETH",        // matches the open
    "borrow":             "USDC",
    "current_debt":       "4180.0",     // frontend-supplied (read from Aave V3 off-chain)
    "current_collateral": "5.0",        // frontend-supplied
    "slippage":           "50"
} }
```

**Notes:**
- `ETH` aliases are accepted — normalize wraps to WETH for the Aave leg (same behavior as existing `deposit`).
- `borrow` default: when collateral is a volatile asset → `USDC`; when collateral is a stable → `WETH`. Surfaces in DSL as optional for simple cases.
- The semantic difference between `long` and `short` is which side sits as collateral. `long ETH` = ETH collateral + USDC debt (exposure increases when ETH goes up). `short ETH` = USDC collateral + ETH debt (exposure increases when ETH goes down).
- `close_position` requires `current_debt` and `current_collateral` because the compiler can't read Aave state. Frontend calls `aave.getUserAccountData(signer)` and threads the numbers in.

### 10.3 Schema (`crates/intent-script/src/schema/public_ast.rs`)

```rust
pub enum Step { …,
    Long(LongStep),
    Short(ShortStep),
    ClosePosition(ClosePositionStep),
}

pub struct LongStep {
    pub collateral: String,
    #[serde(default)]
    pub borrow: Option<String>,          // default inferred from collateral
    pub amount: String,
    pub leverage: String,
    #[serde(default)]
    pub slippage: Option<String>,        // default "50" (0.5%)
    #[serde(default)]
    pub via: Option<String>,             // default "balancer"
}

// ShortStep has the same fields as LongStep (the side-swap is done in normalize).

pub struct ClosePositionStep {
    pub collateral: String,
    pub borrow: String,
    pub current_debt: String,            // explicit decimal string in `borrow` units
    pub current_collateral: String,      // explicit decimal string in `collateral` units
    #[serde(default)]
    pub slippage: Option<String>,
}
```

### 10.4 IR — nothing new

Desugaring in normalize produces `ResolvedStep::BalancerFlashloan { … }` with inner steps composed of:

| Action   | Inner steps                                                        |
|----------|--------------------------------------------------------------------|
| Long     | `AaveV3Supply(collateral, leverage×amount) → AaveV3Borrow(borrow, F×price×(1+s)) → UniswapV3Swap(borrow → collateral, amount_out_min=F)` |
| Short    | `AaveV3Supply(collateral, amount+F_in_collateral) → AaveV3Borrow(borrow, F) → UniswapV3Swap(borrow → collateral, amount_out_min=F_in_collateral)` — but the flashloan is of `collateral`, not `borrow`, see §10.5 |
| Close-long | `AaveV3Repay(borrow, flashloan_amount) → AaveV3Withdraw(collateral, current_collateral) → UniswapV3Swap(collateral → borrow, amount_out_min=flashloan_amount)` |
| Close-short | symmetric |

The only new compiler capability we need is an `AaveV3Repay` resolved step — currently missing (only Supply/Borrow/Withdraw exist). That's an isolated addition:

```rust
pub enum ResolvedStep { …,
    AaveV3Repay {
        pool: Address,
        asset: Address,
        amount: U256,
        rate_mode: u8,          // 2 = variable (match our borrow default)
        on_behalf_of: Address,  // = signer
    },
}
```

Add to `adapters/aave_v3.rs`, wire in dispatch, `step_consumes` entry (consumes `borrow`), `validate_amount` entry, `preview` entry. Approve of `borrow` token to pool is auto-inserted by enrich (same pattern as supply).

### 10.5 Normalize — desugaring

Add a module `crates/intent-script/src/compiler/leverage.rs` with the expansion functions:

```rust
pub fn expand_long(
    step: &LongStep, signer: Address, script: &IntentScript,
    registry: &RegistryContext,
) -> Result<ResolvedStep /* BalancerFlashloan */> { … }

pub fn expand_short(…) -> Result<ResolvedStep> { … }
pub fn expand_close(step: &ClosePositionStep, …) -> Result<ResolvedStep> { … }
```

Each expander:
1. Resolves asset addresses and decimals via the existing helpers in `normalize.rs` (make them `pub(crate)` if not already).
2. Parses `leverage` as a fixed-point decimal with 4 fractional digits (stored as `u32 ppm-ish` or `U256` scaled by 10_000). `"5"` → `50_000`, `"3.5"` → `35_000`.
3. Looks up `ltv_bps[collateral]` in the registry. Computes:
   ```
   theoretical_max = 10_000² / (10_000 − ltv_bps)   // as scaled integer
   effective_max   = theoretical_max − safety_margin_bps
   ```
   Rejects with `CompileError::InvalidChain("leverage X exceeds max Y for collateral Z")` if `leverage > effective_max`.
4. **For `long`:**
   - `F = (leverage − 1) × amount` denominated in `collateral`.
   - Flashloan token = `collateral` (user receives F of collateral; supplies 1+F; borrows; swaps borrow → collateral; repays F).
   - `borrow_amount = F × price × (1 + slippage_bps/10_000)`. Price is **user-supplied** via optional `price` field OR derived from an on-chain quote step that precedes this (future enhancement). For v1, require `price` as an explicit field on the step when `leverage > 1`.
   - `min_amount_out` on the swap = `F` (we must get back at least what we flashloaned, or repayment fails).
5. **For `short`:**
   - Symmetric, but the flashloan is of `collateral` (to have enough collateral to deposit as margin) AND we borrow `borrow` then swap borrow → collateral.
   - Actually — clearer construction: flashloan `borrow` directly, deposit `collateral` (already held by user), borrow MORE `borrow` using new collateral as margin, repay flashloan with the borrowed amount. Shorting a volatile asset with a stable margin is the common case (USDC margin, short WETH): flashloan WETH, swap to USDC, deposit USDC, borrow WETH, repay. The intermediate swap is flashloan → collateral.
   - Pick one construction and stick with it. Recommended: flashloan `collateral` amount = `(leverage − 1) × amount` and let the inner pipeline mirror `long` with the side-swap done at the swap step. Keeps symmetry.

Actually, to keep this tight: **model `short` as `long` with the collateral/borrow pair swapped**. User-facing fields read naturally (`short ETH with USDC margin` = collateral USDC, borrow ETH). Expansion uses the same template as long. The leverage-cap LTV is read against `collateral` in both cases.

6. **For `close_position`:** flashloan = `current_debt` of `borrow` (plus slippage buffer). Inner: `repay(borrow, current_debt) → withdraw(collateral, current_collateral) → swap(collateral → borrow, min_out = current_debt × (1 + buffer))`. Surface a warning (not an error) when `current_debt × price_of_collateral / current_collateral < 0.5` — the user is probably closing at a loss, which is fine but should be visible.

### 10.6 Validate

Enforce in order:
- `collateral != borrow` (else the flashloan is a no-op).
- `amount > 0`, `leverage >= 1`, `leverage <= max_leverage(collateral)`.
- `slippage ∈ [0, 500]` bps — reject wider windows (≥5% sends the user straight into MEV territory).
- For `close_position`: `current_debt > 0 && current_collateral > 0`.
- For `long` with `leverage > 1`: `price` field required until the pre-quote primitive lands (follow-up).

Sub-task 06's validate pass already rejects non-repayable inner pipelines; our expansion leans on it.

### 10.7 Enrich — no changes

The existing sub-task 06 enricher handles approvals/transferFroms for the inner supply/borrow/swap. Our desugared steps are a subset of what 06 already enriches. If 06 is correct for the hand-written composition, it's correct for ours.

### 10.8 Preview — liquidation price

The preview entry for `long`/`short`/`close_position` includes:
- Action summary (`"Open 5× long ETH with 1.0 ETH margin"`).
- Flashloan amount, swap route, Aave collateral+debt deltas (already derivable from inner steps).
- **Liquidation price**: computed from LTV, leverage, slippage: `liq_price = entry_price × (1 − (1 − leverage × (1 − ltv)) / leverage)` — exposed in preview so the UI can show it without the frontend re-implementing the math. Tests pin-check the formula.

### 10.9 Adapter layer

- Add `adapters/aave_v3.rs::lower_repay` (straightforward, mirrors `lower_borrow`).
- No other adapter work — long/short/close are compiler-only sugar.

### 10.10 Tests

Rust (`tests/leverage_tests.rs` NEW):
- `test_long_3x_expansion_matches_hand_written` — compile `{ long: … }` and a hand-written flashloan pipeline with the same economics; assert the resolved IR trees are identical (modulo the outer-step labels).
- `test_long_5x_eth_at_80bps_ltv_accepts` — boundary case.
- `test_long_6x_eth_rejects` — above max.
- `test_short_4x_usdc_collateral_weth_borrow_accepts`.
- `test_close_position_requires_current_debt_and_collateral`.
- `test_close_position_balances_flashloan_to_debt_plus_buffer`.
- `test_leverage_rejects_wide_slippage`.

Foundry (`contracts/test/LeverageFork.t.sol` NEW, behind `ETH_RPC_URL`):
- `test_fork_open5xLongETH_healthFactorSafe` — fork mainnet, deposit 1 ETH, 5× long → assert user's post-tx HF > 1.03 (immediately post-open; drifts with interest).
- `test_fork_close5xLongETH_returnsCollateralAndProfit` — same, then immediately close at unchanged price → assert net loss ≤ 1.5% (swap slippage + Aave flashloan fee if we switched providers in the test).

### 10.11 Skills file updates (deferred to sub-task 09)

- Document `long`, `short`, `close_position` with examples.
- Document the LTV cap table (`ltv_bps` in config).
- Warn that `close_position` requires the frontend to supply `current_debt` / `current_collateral` read from Aave, and that stale values can cause slippage-revert or leave residual debt.

## Definition of done

- [ ] `long` / `short` / `close_position` DSL steps compile end-to-end.
- [ ] Resolved IR for these steps is a single `BalancerFlashloan` outer step with 3-step inner pipelines.
- [ ] `AaveV3Repay` variant + adapter + enrich branch shipped.
- [ ] LTV table loads from config; leverage cap enforced per-asset.
- [ ] Close-position requires and validates `current_debt` / `current_collateral`.
- [ ] Liquidation price appears in preview output.
- [ ] `make test && make test-foundry` green.
- [ ] `ETH_RPC_URL=… make test-fork-e2e` passes the new leverage fork tests.

## Verification

```bash
cd /Users/fermat/Desktop/intentOS/intent-script
make test && make test-foundry
ETH_RPC_URL=… make test-fork-e2e
```

## Hand-off (follow-ups beyond v1)

1. **Aave flashloan fallback** — add `"via": "aave"` that emits `AaveV3Flashloan` instead of `BalancerFlashloan`. Requires `receiveFlashLoan`-equivalent callback on IntentRouter (`executeOperation`). Token selection can be automatic (Balancer first, fall back to Aave if the frontend reports Balancer vault balance < required).
2. **On-chain price quote step** — a `quote` step that calls Uniswap V3 `quoter.quoteExactInputSingle` and threads the answer into the leverage expansion, removing the need for user-supplied `price` on longs.
3. **Position registry helper contract** — for users managing multiple concurrent positions on the same Aave account, a thin Solidity "position ledger" that assigns IDs and stores original collateral/borrow pairs. The old-code `closePosition(ID)` pattern. Opt-in; stateless flow stays the default.
4. **Close by delta** — `{ "close_position": { …, "fraction": "0.5" } }` to partially close. Needs the Aave state read too (maps to half the recorded debt & collateral).
