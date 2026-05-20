# IntentOS

> **North star.** This document is the source of truth for product scope. When an issue conflicts with this doc, the doc wins. When in doubt about whether something is in scope, check the MVP and Roadmap sections at the bottom.

## The thesis

Two adjacent markets are stuck on either side of a wall. On one side, traditional banking sells personalized financial advice as a roughly $50B/year product gated behind seven-figure minimums and human-only delivery — 95% of people can't access it. On the other side, DeFi has produced real, durable yield, but the UX is so brutal and the safety story so thin that 99% of people can't reach it. Five protocol UIs to click through, raw calldata to trust, and a single misrouted transfer can drain a position.

The contrarian truth is that crypto's biggest opportunity is not trading. It's not even AI agents. It's that **financial advice and execution become a programmable, scalable product the moment a sufficiently safe AI can operate on permissionless rails through plain human language.** Advice without execution is just talk. Execution without safety is impossible for real money. Safety requires a verifier — not a smarter LLM. Whoever ships the verifier-shaped advisor first wins the larger of two markets that crypto has spent a decade fighting over.

Most "AI DeFi" companies will lose this race because they have inverted the architecture. They ship a chat box around a vague execution layer and hope the model produces correct calldata. IntentOS inverts that: the compiler is the safety substrate, and the LLM is a frontend over it. Natural language goes in; a constrained, simulated, invariant-checked transaction comes out.

## What it is — the long-term product

IntentOS is an onchain financial advisor that lets anyone execute complex DeFi operations through natural language. A user types what they want — *"find the best yield for my idle USDC," "close my Aave position and bridge to Base," "rebalance my portfolio to lower risk"* — and the product translates the request into a constraint-checked, simulated, signable transaction. Multi-step operations across swaps, lending, LPs, staking, leverage, and bridging collapse into a single signature. The relationship is recurring: the advisor watches positions and surfaces opportunities or risks proactively over time.

At full scope, the product has four layers, each carrying part of the moat.

A **safety compiler**, written in Rust. Every recommendation or user request is lowered into `intent-script`, a JSON DSL that describes multi-step DeFi operations declaratively, then run through a fixed pipeline — parse, normalize, validate, preview, enrich, lower, plan, build — to produce a constrained, simulated, invariant-checked transaction. Recipient pinning, slippage caps, and health-factor checks block the LLM from quietly producing a transaction that drains the user. This is the verifier that makes everything above it possible.

An **advisor surface**: a chat-style web app where the LLM reads the user's wallet, surfaces opportunities and risks unprompted, makes opinionated recommendations with reasoning, and turns each one into a single-signature transaction. After execution, the advisor maintains the relationship — it watches positions, nudges on rebalances, and flags risk as conditions change.

A **smart-account chassis**: a Safe-based smart account extended with custom modules for vault deposits, credit drawdowns, intent execution, and scoped agent delegation. Self-custody and permissionless by design — no KYC, no fiat rails, no jurisdictional gates. The same chassis serves a solo retail user, a 12-person DAO, and an autonomous agent.

An **owned financial primitive**: curated MetaMorpho vaults built on Morpho Blue, with a paired credit module that turns deposits into an instant working-capital line. This is the layer that turns advice into a financial product and accumulates sticky TVL.

The MVP ships only the first two layers on top of the user's existing wallet. The smart-account chassis and the owned vault are deferred — see "MVP scope" and "Roadmap" below.

## What it does

A typical session at full scope looks like this. A user connects a wallet. The advisor scans the portfolio and opens the conversation:

> *"You're holding $42k USDC earning 0% across two wallets. Given your existing wstETH and Aave positions, I'd put $25k into Morpho USDC at 5.1% (matches your Aave exposure so we don't double up), $12k into a curated leveraged stETH strategy at 7.8% (correlates with the stETH you already have), and keep $5k liquid. Here's the simulation."*

The user asks follow-ups in chat, signs once, and three positions deploy atomically. Every step is simulated against an RPC end-to-end before the user sees it.

The chat also accepts user-initiated requests in plain language — *"close my Aave position," "swap and bridge to Base," "stake my idle ETH on Lido and lever it 2x on Aave"* — for users who already have a plan. This is a chat affordance, not a separate product. The headline experience is the proactive advisor; reactive execution is what the same chat does when you tell it what to do.

After execution, the advisor watches. Yields shift, utilization spikes, oracle prices drift, opportunities open — each becomes a useful nudge with a one-signature rebalance attached.

Supported protocols today: Aave V3, Morpho Blue, Lido and wstETH, Uniswap V3 (swaps and the full LP lifecycle), Balancer flashloan-backed leverage, Across for bridging.

## Who uses it

**On the consumer surface, the advisor.** The customer is someone whose stablecoins or ETH are idle and who wants ongoing financial guidance they can trust on real money. At v1 this is crypto-native retail and prosumer users; the same surface scales up into a treasury experience for crypto-native businesses, DAOs, and onchain orgs over time — where the credit module is most valuable, because idle reserves earn yield and simultaneously back a working-capital line at a net cost dramatically below any traditional bank facility.

For users who don't want to share an LLM API key with the app, a manual flow copies a context-rich prompt to run in ChatGPT and paste the response back. Keys stay client-side; the server stores none.

**On the developer surface, the compiler API** (`/api/v1/compile`, `/simulate`, `/execute`). The customer is a team building autonomous DeFi agents, wallet features, or app integrations who needs hardened execution without writing their own calldata layer. Auth is Sign-in-with-Ethereum (EIP-4361) issuing bearer tokens. Same engine, programmatic surface, different audience. Over time, third-party agents built on this API become first-class citizens inside the consumer product — composable through scoped, constraint-checked permissions.

## Business model

The advisor is free at the user surface. The full long-term revenue model has four legs.

**Curated vault management fee.** Recommended deposits route through IntentOS-owned MetaMorpho vaults built on Morpho Blue, charging a small management fee (10–50 bps) baked into protocol mechanics. The user sees no extra fee on top of gas; revenue scales with TVL and recurs without billing infrastructure. The closest analog is Wealthfront, adapted onchain.

**Credit-line spread.** The credit module captures a small spread between the deposit yield and the borrow rate on collateralized working-capital positions.

**API tier for developers.** Usage-based pricing once volume justifies it. The consumer surface remains the live proof point that the engine works on real money.

**Premium tier for orgs and power users.** Optional paid features — autonomous agent delegation, advanced rebalancing, multi-account treasury management, accounting and audit exports.

The MVP is the BYOK consumer surface only. No revenue layer ships with the MVP. The first paid layer is the developer API; the vault management fee comes online once the owned vault ships in a later phase.

## Why we win

The moat is intentionally compound, not single-axis. None of the layers below is replicable in a quarter; each one protects the next while it matures.

**Operational track record for safety.** Every simulated transaction, every prevented mis-execution, every audit, every year without an incident becomes brand. This is slow to build and impossible to fake. At scale it is the only durable trust that exists in this category. *(Builds from MVP onward.)*

**Default trust.** Most users don't want to choose. They want the smart recommendation, accepted on faith, the way Wealthfront and Betterment users accept the default portfolio. If the advisor's default suggestion is the trusted default, people stay because they have no reason to leave. *(Builds from MVP onward.)*

**Owned vaults that accumulate sticky TVL.** Curation of Morpho Blue markets gets better with data and worse to leave. Credit positions create explicit switching costs because a borrow position can't be ported between platforms. *(Activates once the owned vault ships — see Roadmap.)*

**The compiler as a category standard.** `intent-script` is open and designed for other surfaces, agents, and protocols to target. If it becomes the format the ecosystem speaks — the way Stripe became the API verb every payment integration speaks — IntentOS holds a Stripe-shaped position in safe LLM-driven onchain execution.

**A developer ecosystem.** Third-party agents and apps built on the API become composable inside the consumer surface: two-sided lock-in that compounds with every new agent shipped on the platform.

The timing argument is independent of the moat argument. Structured-output LLMs got reliable in 2024–2025. DeFi protocols have stabilized into a small, stable set of dominant primitives. The cost of building this exact product was prohibitive eighteen months ago and will be table stakes eighteen months from now. The window is open now and it does not stay open forever.

---

## MVP scope (this sprint)

Goal: ship the smallest possible advisor experience on top of the existing compiler and chat UI, runnable in days. The MVP proves that a user can connect a wallet, receive a proactive opinionated recommendation, and execute it in one signature — with the existing reactive flow ("do X for me") preserved as a fallback.

In scope:

- **Wallet portfolio scan.** On wallet connect, build a structured `PortfolioSummary` object: balances by token, existing positions across supported protocols (Aave V3, Morpho Blue, Lido, Uni V3 LPs), and idle stable/ETH amounts. Reuse existing position-loading code where possible.
- **Proactive recommendation generation.** A new system prompt path that takes a `PortfolioSummary` and emits an opinionated recommendation as `intent-script` plus a short rationale string. The recommendation should be specific (concrete amounts, concrete protocols) and risk-aware (acknowledge correlation with existing positions where relevant).
- **Auto-trigger on connect.** When a user connects their wallet for the first time in a session, automatically run scan → recommendation → render. The user does not have to type anything to see the first recommendation.
- **Recommendation card UI.** Render the recommendation with: a one-line summary, the rationale ("why this allocation?"), the existing simulation preview (steps, gas, you-send/you-receive), and a single sign button. "Why?" should be expandable to show longer reasoning.
- **Reactive chat preserved.** The existing flow where a user types a specific intent ("close my Aave position," "swap 5 ETH for USDC and lend it") still works in the same chat. Treat user-initiated messages as overriding the proactive recommendation.
- **Session continuity (minimal).** A user returning later sees the chat history from their last session. No notifications, no monitoring service, no nudges yet — just persistence.
- **Copy and tone pass.** All UI copy reads as advisor-shaped, not tool-shaped. The first message the user sees should feel like advice, not like a form.

Out of scope for MVP (deferred):

- Owned MetaMorpho vault and curated market creation.
- Credit module and working-capital line.
- Safe-based smart-account chassis and custom modules.
- Active position monitoring and notifications ("USDC yield dropped, want to move?").
- Scoped agent delegation and autonomous execution.
- Treasury surface and multi-sig org accounts.
- Premium tier features (multi-account, accounting exports, audit reports).
- Paid API tier billing and metering.
- Multi-recommendation portfolio views (one recommendation at a time is enough for MVP).

## Roadmap

**v0 (MVP, this sprint).** Proactive advisor on top of the existing compiler. Free, BYOK, single-recommendation flow per session. Existing reactive chat preserved. Goal: prove the advisor experience works end-to-end on real money.

**v1 (next 1–3 months).** Position monitoring and proactive nudges (yield changed, utilization spiked, rebalance opportunity). Multi-recommendation portfolio view. Polished onboarding for new wallets with no positions yet. First paid customers on the developer API tier.

**v2 (3–9 months).** Owned MetaMorpho vault deployed on Morpho Blue. Recommended deposits route through it; vault management fee starts producing revenue. Safe-based smart-account chassis and the first custom modules (vault deposit, intent execution). Treasury surface for crypto-native orgs and DAOs.

**v3 (9–18 months).** Credit module turning vault deposits into an instant working-capital line. Scoped agent delegation. Premium tier features (multi-account, accounting exports). First third-party agents shipping on the platform via the API.

**Long-term.** `intent-script` adopted as a category standard for safe LLM-driven onchain execution. A developer ecosystem of agents composing inside the consumer smart account. Default trust earned over years of operational track record. The financial-advice market — bigger than the trading market — running on permissionless rails.
