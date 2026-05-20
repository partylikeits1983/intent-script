# IntentOS MVP UI and Compiler Test Plan

## Purpose

This document is a concrete handoff for what should happen next in IntentOS without changing application code yet.

It covers:

1. Product and UI priorities for a ship-ready MVP
2. How the current UI should evolve
3. What I observed in the Rust compiler and test layout
4. Which compiler tests should be improved or added next

The goal is to keep scope focused on a realistic MVP that can be shipped, trusted, and iterated on.

---

## What the product should be

IntentOS should not ship first as a generic "AI DeFi bot."

The current repo is strongest when framed as:

- an intent compiler
- a safe execution layer
- a simulation and preview layer
- a UI that helps a user decide what to do next

That means the MVP should be:

**"A DeFi recommendation and execution copilot for yield upgrades and position management on Ethereum."**

The first user jobs to win:

- "I want more yield on idle USDC / ETH / stables"
- "Should I close or keep this Aave / Morpho / Uni V3 position?"
- "Show me safer vs higher-yield options"
- "Execute the plan after showing me the risks"

The first non-goals:

- full DeBank replacement
- full multi-chain routing
- privacy integrations
- infinite protocol breadth
- open-ended autonomous trading

---

## Current repo reality

### What already exists and is stronger than the UI implies

The compiler side already supports more than the current UI framing suggests:

- Aave V3
- Morpho Blue
- Lido and wstETH flows
- Uniswap V3 swaps and LP lifecycle
- Balancer flashloan-backed leverage flows
- Across bridging
- router batching
- EIP-712 flows
- simulation and preview in the UI

The product gap is not mainly "more calldata adapters."

The bigger gap is that the current UI still behaves mostly like:

- user asks for a transaction
- LLM produces JSON
- compiler validates it
- UI previews it

But the intended product needs to become:

- user states a goal
- system evaluates options
- UI compares strategies
- user selects one
- compiler/executor handles the execution plan

---

## UI direction

## Core principle

The UI should stop feeling like a generic assistant shell with DeFi attached and start feeling like an **operator console for strategy selection and execution**.

The UI should make these three things obvious:

1. What the user has now
2. What IntentOS recommends instead
3. What exactly will happen if they execute

### Current UI strengths

The existing UI already has a solid base:

- wallet connection
- thread model
- compile + simulation flow
- approval handling
- preview card
- balances hook
- model selector

### Current UI weaknesses

The current UI is still weak on:

- portfolio context
- recommendation comparison
- risk communication
- product-specific navigation
- making the app feel like a DeFi tool rather than a chat scaffold

---

## Concrete UI plan

### Phase 1: make the current shell feel like IntentOS

This phase should happen before any large recommendation system work.

#### Goals

- tighten identity
- surface context
- reduce generic chat feel
- improve trust around execution states

#### Changes

1. Upgrade the empty state
   - Replace the generic welcome with an IntentOS-specific landing state.
   - Show 4-6 opinionated prompts:
     - Earn more on idle stables
     - Improve ETH yield
     - Review my Aave risk
     - Close my Uni V3 position
     - Show safe vs aggressive options

2. Promote balances into a real sidebar module
   - Keep the current token balance panel, but present it as a portfolio summary.
   - Show:
     - wallet status
     - active network
     - top balances
     - a small "view all" or expanded state later

3. Improve header clarity
   - The header should clearly show:
     - network
     - selected model/mode
     - wallet state
   - Avoid generic branding or unnecessary scaffold feel.

4. Improve execution-state language
   - Current phases are technically correct, but the UI should read more clearly:
     - Compiling plan
     - Approval required
     - Simulating outcome
     - Ready to execute
     - Executing
     - Confirmed
     - Failed
   - Use clear color semantics for safe / caution / blocked.

5. Keep advanced info, but hide it by default
   - Keep reasoning collapsed by default.
   - Keep raw JSON DSL in an Advanced disclosure.
   - Keep simulation raw errors accessible but not dominant.

#### Definition of done

Someone landing on the app should immediately understand:

- this is a DeFi strategy/execution product
- the wallet context matters
- the app is opinionated about risk and execution safety

### Phase 2: add recommendation cards

This is the most important product-facing UI upgrade.

#### Goals

- move from "single chat answer" to "compare options"
- make tradeoffs visible
- keep the execution pipeline intact underneath

#### Changes

Add a recommendation-card system with at most 3 options:

- Safe
- Balanced
- Aggressive

Each card should show:

- strategy title
- target protocols
- estimated net yield
- risk tier
- management burden
- liquidity / lockup note
- one-sentence rationale
- CTA: `Review Plan`

#### Example cards

- Deposit USDC to Morpho Blue
- Stay in Aave but rotate collateral
- Close Uni V3 LP and move to lending
- Stake ETH to Lido and optionally use wstETH as collateral

#### Definition of done

A user should be able to compare options without reading long assistant prose first.

### Phase 3: upgrade the preview into a review panel

The current preview card is directionally right but should become more decision-grade.

#### Goals

- make execution impact obvious
- show why the action matters
- keep advanced details accessible

#### Changes

The review surface should have explicit sections:

- `You Have Now`
- `You’ll End Up With`
- `Actions`
- `Approvals`
- `Risks`
- `Simulation`
- `Advanced`

For recommendation-driven plans, also show:

- `Why this is better`
- `Why not the other options`

#### Definition of done

A user should be able to decide whether to execute without mentally reconstructing the transaction flow.

### Phase 4: add position-aware context

This is the point where the app starts answering "what should I do with what I already have?"

#### Goals

- move beyond balances-only context
- let the UI reason about positions, not just assets

#### Position types to surface first

- Aave collateral and debt
- Morpho positions
- Uniswap V3 LP positions

#### UI modules

- `Current Positions`
- `Suggested Opportunities`
- `Selected Plan`
- `Post-Trade State`

#### Definition of done

The app can suggest closing, migrating, deleveraging, or re-allocating existing positions instead of only creating fresh actions.

---

## UI architecture implications

The current UI can support this direction, but the product contract will need to expand beyond plain markdown replies.

Eventually the UI should consume structured objects for:

- recommendations
- risk level
- estimated net yield
- current position summary
- post-trade summary
- strategy rationale

Without that, the interface will stay too chat-heavy and weak at comparison.

The UI should remain conversational, but structured outputs need to become first-class.

---

## Compiler observations

I reviewed the current compiler entrypoint and test layout:

- `crates/intent-script/src/compiler/mod.rs`
- `crates/intent-script/tests/integration.rs`
- `crates/intent-script/tests/enricher_tests.rs`
- `contracts/test/IntentRouterCalldata.t.sol`
- `contracts/test/IntentForkE2E.t.sol`

### Good current properties

The compiler pipeline is clean and easy to reason about:

- parse
- normalize
- validate
- preview
- enrich
- lower
- plan
- build

That structure is strong and worth preserving.

The test suite already does a good job covering:

- basic end-to-end compile success cases
- common validation failures
- enricher token-routing invariants
- calldata fixture round-tripping
- fork E2E against mainnet contracts

### Where the test suite still feels thin

The main test gap is not raw volume. It is **coverage shape**.

A lot of tests confirm happy-path compilation, but fewer tests pin down:

- edge-case warnings
- preview correctness
- allowance parsing behavior
- exact router batching decisions
- negative-path protocol-specific validation
- invariants across many supported step combinations

There is also still some coupling to known limitations or historical bugs, especially in the fork tests.

---

## Recommended compiler test improvements

## Priority 1: preview-output tests

The preview is now user-facing product surface, not just compiler garnish.

Add focused tests around `compiler/preview.rs` for:

- single-step wrap / stake / swap
- multi-step swap -> deposit
- deposit -> borrow
- LP open / decrease / collect
- close-position flows where relevant

What to assert:

- "you send" assets are correct
- "you receive" minimums are correct
- step labels are correct
- auto-inserted approvals / transferFrom do not leak into the preview

Why this matters:

The preview is becoming the trust layer in the UI. It deserves its own dedicated correctness tests.

## Priority 2: allowance parsing and prerequisite-approval tests

The compile path with allowances is important and should be pinned harder.

Add tests for `compile_with_allowances` covering:

- allowance exactly equal to required pull
- allowance smaller than required pull
- allowance larger than required pull
- missing token symbol in allowances blob
- malformed numeric allowance value
- unknown token alias in allowances blob producing warning
- native asset allowance entries being ignored

What to assert:

- prerequisite approvals appear only when needed
- warnings are deterministic
- unknown symbols do not silently corrupt behavior

Why this matters:

This is directly tied to the UI approval flow and is easy to regress.

## Priority 3: deadline and warning behavior tests

The compiler explicitly appends a warning for batched intents that have no deadline source.

Add tests for:

- batched intent with neither `deadline` nor `current_timestamp`
- batched intent with `current_timestamp`
- single-tx intent with no deadline source

What to assert:

- warning appears only for the batched case without a deadline source
- warning text stays intentional

Why this matters:

Warnings are user-facing behavior now. They should be treated as contract, not incidental strings.

## Priority 4: planner-mode tests

The plan/build split is strong, but the tests should lock it down more explicitly.

Add tests that assert execution mode selection for:

- single call -> `SingleTx`
- multi-call with router -> `Eip712Intent`
- multi-call without router -> `TxSequence`
- flows that require router even if routing seems optional

Why this matters:

Execution mode is a major product behavior. It affects UX, signing, and approvals.

## Priority 5: protocol-specific negative tests

Add more tests that intentionally fail for the right reason.

Suggested cases:

- deposit native `ETH` into Aave
- swap token to itself
- zero `min_amount_out`
- invalid Uni V3 LP tick spacing
- invalid LP token ordering normalization behavior
- borrow with clearly unsafe health factor inputs
- Morpho market mismatch or missing market
- Lido claim without required hint data

Why this matters:

The compiler is a constraint system. Negative-path quality is part of the product.

## Priority 6: cross-step amount-flow stress tests

There are already amount-flow checks, but they deserve more varied cases.

Add tests for:

- `"all"` referencing the most recent producer only
- downstream consume greater than guaranteed prior production
- mixed-token chains where `"all"` could be ambiguous
- fee-aware downstream amount reduction when router fee is active
- sweep-sensitive output chaining

Why this matters:

This is where LLM-generated plans are most likely to get subtle things wrong.

## Priority 7: preview vs enriched-call invariants

The compile pipeline intentionally builds preview from resolved pre-enriched steps.

Add tests that assert:

- preview omits transferFrom
- preview omits ERC20 approve
- preview still reflects the same economic result as the enriched batch

Why this matters:

This is a core UX invariant and easy to accidentally break during compiler refactors.

## Priority 8: fixture and fork-test coverage improvements

The Foundry tests are useful, but a few improvements would make them more robust.

### For `IntentRouterCalldata.t.sol`

Add stronger decode assertions:

- number of calls in the batch
- first call target
- selectors of embedded calls
- sweep token list content

Right now some tests mainly check selector presence and non-empty calldata. They can go further.

### For `IntentForkE2E.t.sol`

Add or improve tests for:

- prerequisite approval expectations
- router fee behavior when sweeping
- explicit assertions on no residual dust in the router
- LP lifecycle round-trips
- Morpho supply/borrow/withdraw flows
- withdrawal / close-position flows where they are already supported

Why this matters:

Fork tests are the highest-confidence end-to-end signal in the suite.

---

## Specific test files I would add

If I were extending the suite next, I would add:

- `crates/intent-script/tests/preview_tests.rs`
- `crates/intent-script/tests/allowance_tests.rs`
- `crates/intent-script/tests/deadline_warning_tests.rs`
- `crates/intent-script/tests/planner_mode_tests.rs`
- `crates/intent-script/tests/protocol_negative_tests.rs`

These should stay small and targeted rather than bloating `integration.rs`.

That separation would make failures easier to understand and would reduce "giant file" drift.

---

## Test-style improvements

Some structural improvements would make the test suite easier to maintain.

### 1. Reduce giant-file accumulation

`integration.rs` already carries many unrelated concerns.

It would be better to split tests by behavior:

- integration happy paths
- warnings and planner behavior
- preview behavior
- allowances
- protocol-specific negatives

### 2. Add helper assertions for output shapes

Common assertions should become helpers:

- assert single-tx target and selector
- assert eip712 router target
- assert sweep tokens contain X
- assert prerequisite approvals length and token

That will make future adapter additions less repetitive.

### 3. Treat warnings as part of the contract

If warnings drive UI and user trust, they should be asserted more deliberately.

Tests should pin both:

- when warnings appear
- when warnings do not appear

### 4. Prefer behavior-level assertions over giant JSON string checks

Some current tests serialize output and assert broad substrings.

That is useful as smoke coverage, but the more durable tests are the ones that assert:

- typed output variant
- field values
- counts
- addresses
- selectors

The JSON serialization checks should remain as a thin layer, not the main layer.

---

## Suggested immediate next implementation order

If I were implementing from this document, I would do it in this order:

1. UI Phase 1 shell improvements
2. compiler preview tests
3. allowance / deadline warning tests
4. UI recommendation-card surface
5. planner and protocol-negative tests
6. position-aware UI context
7. stronger calldata and fork assertions

That order improves:

- product clarity first
- trust layer second
- deeper strategy UX after the compiler surface is better pinned down

---

## Final recommendation

The most important discipline for the next phase is:

**do not confuse "more protocols" with "more product."**

The compiler is already fairly capable. The next leap in product quality will come from:

- clearer recommendation UX
- stronger preview/review UX
- better position awareness
- more explicit compiler invariants in tests

That is the path to a real MVP that can be trusted by users and later sold as infrastructure.
