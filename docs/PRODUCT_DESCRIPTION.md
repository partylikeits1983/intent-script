# IntentOS

## What it is

IntentOS is a DeFi recommendation and execution copilot for yield upgrades and position management on Ethereum. It has three pieces that work together:

- **intent-script**, a JSON DSL for describing multi-step DeFi operations (swaps, lending, LPs, staking, leverage, bridging) declaratively rather than as raw contract calls.
- A **Rust compiler** that takes an intent and runs it through a fixed pipeline — parse, normalize, validate, preview, enrich, lower, plan, build — to produce a simulated, signable Ethereum transaction (or batch).
- A **chat-style web UI** where a user states a goal in natural language; an LLM emits intent-script JSON, the compiler validates and simulates it, and the UI shows a transaction preview the user can sign.

## What it's used for

The product is aimed at a small set of concrete user jobs: *I want more yield on my idle USDC, ETH, or stables. Should I close or keep this Aave / Morpho / Uni V3 position? Show me a safer option versus a higher-yield one. Execute the plan once you've shown me the risks.*

End-to-end, a session looks like this: the user connects a wallet; their balances and existing DeFi positions are loaded into context; they type what they want; the LLM generates intent-script; the compiler validates it against a registry of supported protocols and a set of safety invariants, simulates it against an RPC, and renders a preview card showing the steps, gas, and a "you send / you receive" summary; the user signs and broadcasts. Supported protocols today include Aave V3, Morpho Blue, Lido and wstETH, Uniswap V3 (swaps and the full LP lifecycle), Balancer flashloan-backed leverage, and Across for bridging.

## Who uses it and why

There are two distinct customer segments, served by two surfaces of the same compiler.

**DeFi end users** use the browser UI. These are wallet-connected retail and prosumer DeFi participants who want a safer and more legible alternative to clicking through five different protocol UIs and trusting that the calldata is right. The pitch to them is correctness and trust: every transaction is simulated before it's signed, and a recipient-pinning invariant in the compiler blocks the LLM from rerouting funds (Aave borrows, withdrawals, and ERC-20 transfers must resolve to the signer's own address). For users who don't want to share an OpenAI key with the app at all, there's a manual flow that copies a context-rich prompt for the user to run in ChatGPT and paste the response back.

**Agent and app developers** use the Rust HTTP service (`/api/v1/compile`, `/simulate`, `/execute`). These are teams building autonomous DeFi agents or product features who need a hardened intent compiler instead of writing their own calldata layer. Auth is Sign-in-with-Ethereum (EIP-4361) issuing bearer tokens.

The shared reason both segments pick IntentOS is the same: a constraint-checked compiler with simulation and preview is dramatically safer than letting an LLM hand-roll calldata.

## Business model

IntentOS is structured as a two-tier business. The consumer UI is free and BYOK — the user supplies their own LLM API key, which stays client-side; the server stores no user keys. This UI acts as the funnel and the proof point. The Rust API is positioned as paid infrastructure for agent and app developers who want the same compiler under their own product.

Pricing, tiering, and revenue specifics are not yet documented in the repo. The visible roadmap is still landing page, hosted docs, API reference, and a quickstart cookbook, with the commercial layer to come after.

## Why this exists

Most "AI DeFi" tools are a chat box wrapped around a vague execution layer. IntentOS inverts that: the compiler is the product, and the LLM is a frontend over it. Recipient pinning, slippage caps, health-factor checks, and a deterministic preview mean the model can't quietly produce a transaction that drains the user — which is the only way an AI execution layer for real money can work.
