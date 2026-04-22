# Corrections to `../expand-defi-coverage.md`

The parent plan is 95% correct, but there are a handful of places where its claims don't match the current code. These were verified by direct codebase inspection on 2026-04-22. Read this *before* trusting the parent plan, and consult it if you find the parent plan ambiguous.

---

## 1. ERC20 and ERC721 transfers are ALREADY supported — do not re-implement

**Claim in parent plan:** Not stated either way.

**Reality:** The compiler and adapters already handle user-to-user transfers for ERC20, native ETH, and ERC721. Every sub-task should assume these primitives exist.

**Evidence:**
- `crates/intent-script/src/schema/public_ast.rs` — `Step::Send(SendStep)` with `asset_type: "erc20" | "erc721"`, `contract`, `token_id`.
- `crates/intent-script/src/ir/canonical.rs` lines 145-158 — `SendErc20 { token, to, amount }`, `SendEth { to, amount }`, `SendErc721 { contract, from, to, token_id }`.
- `crates/intent-script/src/adapters/send.rs` — `lower_send_erc20` (uses `transfer()`), `lower_send_eth`, `lower_send_erc721` (uses `safeTransferFrom`).
- `crates/intent-script/src/adapters/mod.rs` lines 28-30 — dispatch wired.
- `crates/intent-script/src/compiler/normalize.rs` lines ~503-526 — normalizes both ERC20 and ERC721 send variants.
- `skills/json-dsl-spec.md` already documents `### send — Transfer Tokens/ETH/NFTs`.

**Action for every sub-task:** When you need a send primitive, just use it. When you update docs in sub-task 09, reinforce in the skills file that these are first-class step types.

**Caveat:** `SendErc20` uses `IERC20.transfer()` (tokens already held by router). To pull tokens from the user first, the enricher auto-inserts a separate `Erc20TransferFrom` step. Keep this invariant.

---

## 2. The `LidoStake` "bug" is a naming issue, not a wrong-token return

**Claim in parent plan (Phase 3.1):** `step_produces(LidoStake)` at `ir/canonical.rs:218` returns the Lido pool address instead of stETH. "Fix" by adding a `steth: Address` field to the variant.

**Reality:** The `LidoStake { lido, amount, referral }` variant's `lido` field already holds the **stETH contract address** — the Lido submit target is literally the stETH token contract (0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84). `normalize.rs` lines 474-488 look it up via `protocols.lido.contracts.steth`. Enrich.rs lines 158-162 treat it as stETH and add to sweep_tokens. Everything works — the field is just poorly named.

**Action for Phase 3 (sub-task 04):** Do NOT add a new `steth` field. Instead, **rename the field `lido` → `steth`** in `ResolvedStep::LidoStake` and update every reference (normalize.rs, enrich.rs, canonical.rs step_produces/step_consumes, adapters/lido.rs if any). Less code churn, same outcome.

---

## 3. `AaveV3Withdraw` not adding to `sweep_tokens` is intentional, not a bug

**Claim in parent plan (Context):** `enrich.rs` "AaveV3Withdraw doesn't add the withdrawn asset to `sweep_tokens` while AaveV3Borrow does — fix in Phase 3 (consistency; otherwise users see withdrawn tokens stuck in router)."

**Reality:** Aave's `withdraw(asset, amount, to)` sends tokens directly to the `to` address. The enricher sets `to = signer`, so tokens never touch the router. No sweep needed.

Aave's `borrow(asset, amount, rateMode, referral, onBehalfOf)` sends tokens to `msg.sender` (the router), which is why Borrow *does* require a sweep.

**Action for sub-task 04 (Phase 3):** Do not touch AaveV3Withdraw. Do not add to sweep_tokens. If you believe tokens are stuck, re-read `enrich.rs` and `adapters/aave_v3.rs` — the `to` field is what matters.

---

## 4. Solc version is already ≥0.8.24 — no pragma bump needed for flashloan sub-task

**Claim in parent plan (Phase 5 DoD):** "Solidity compiler pragma bumped to `^0.8.24` if necessary for `tstore`/`tload`."

**Reality:**
- `contracts/foundry.toml` has `solc = "0.8.28"`.
- `contracts/src/IntentRouter.sol` pragma is `^0.8.20`. That caret permits 0.8.28.

Both support EIP-1153 (`tstore`/`tload`). No bump required. If you want, tighten the pragma in IntentRouter.sol to `^0.8.24` for clarity, but it's cosmetic.

**Action for sub-task 06 (Phase 5):** Skip the pragma-bump bullet.

---

## 5. Phase 5 flashloan guard design — ignore the cookie variant

**Parent plan (Phase 5.1)** presents two designs in sequence: first a cookie-based guard (`keccak256(block.number, i, msg.sender)`), then abandons it in favor of a boolean sentinel. The cookie text is dead weight that will confuse an implementer.

**Action for sub-task 06 (Phase 5):** Use only the boolean sentinel:
- `tstore(FLASHLOAN_GUARD_SLOT, 1)` just before calling `vault.flashLoan`
- In `receiveFlashLoan`: `require(msg.sender == BALANCER_VAULT)`, `require(tload(FLASHLOAN_GUARD_SLOT) != 0)`, then `tstore(FLASHLOAN_GUARD_SLOT, 0)` to clear before executing inner calls.

Security argument: only the Vault can invoke `receiveFlashLoan` (msg.sender check). The sentinel defends against pathological cases like a compromised allowlisted target re-entering via `delegatecall`.

---

## 6. Flashloan IR should carry `Vec<ResolvedStep>`, not `Vec<ConcreteCall>`

**Claim in parent plan (Phase 5.4):**
```rust
pub enum ResolvedStep { …,
    BalancerFlashloan {
        …,
        inner_calls: Vec<ConcreteCall>,  // lowered inner pipeline
        …
    },
}
```

**Problem:** Enrich runs on `ResolvedStep`. If inner is already `ConcreteCall`, you've lowered before enriching — which means auto-insertion of approvals/transferFroms for inner steps has nowhere to happen.

**Fix for sub-task 06:**
```rust
BalancerFlashloan {
    vault: Address,
    tokens: Vec<Address>,
    amounts: Vec<U256>,
    inner_steps: Vec<ResolvedStep>,  // enriched separately, lowered last
    …
}
```

Enrich pass walks inner_steps with a fresh context; lower pass converts the whole tree bottom-up.

---

## 7. Confirmed-accurate parts of the parent plan

For the avoidance of doubt, these parent-plan statements are correct and should be followed:

- Tornado Cash / Privacy Pools out of scope.
- Fee: 10 bps at sweep time, 24h timelock, 100 bps cap.
- Bridging: Across V3 only, single-sided.
- Flashloan provider: Balancer V2 Vault (0% fee).
- Max 5 outer steps / 5 inner / depth 1.
- Fee-aware `step_produces` signature change: `step_produces(step, fee_bps) -> Option<(Address, U256)>`.
- Morpho market `id` stored in config so it isn't recomputed every compile.
- Uniswap V3 LP: `position_id` must be explicit (no `"last_minted"`); mint uses `recipient = signer` so router never holds NFT; decrease/collect requires user to pre-`approve(router, tokenId)` on NPM.
- Across: `relayer_fee_bps <= 50`; reject native ETH (require pre-wrapped WETH); `fill_deadline = quote_timestamp + 4h`; `output_token = input_token` for v1.
