//! System-prompt assembly — the Rust port of `buildSystemPrompt()` from
//! `intentOS-ui/lib/system-prompt.ts`.
//!
//! `system-prompt.md` in this directory is a **verbatim copy** of
//! `intentOS-ui/lib/system-prompt.md` (the canonical source of truth). It is
//! copied in so the `intent-script` crate stays self-contained. Re-sync it
//! with `make sync-advisor-prompt` whenever the frontend prompt changes.

use crate::context::RuntimeContext;

/// The prompt body, embedded at compile time.
const TEMPLATE: &str = include_str!("system-prompt.md");

/// Text-mode output contract — the LLM emits a `TITLE / SUMMARY / --- / JSON`
/// block that `parse.rs` then parses. This is the verbatim `TEXT_OUTPUT_FORMAT`
/// constant from `intentOS-ui/lib/system-prompt.ts`. We use text mode (not the
/// `finalize_intent` tool-call mode) because a CLI has no need for tool-calling
/// round-trips — one completion in, one DSL block out.
const TEXT_OUTPUT_FORMAT: &str = r#"In intent mode, you MUST emit your response in EXACTLY this form and nothing else:

```
TITLE: <3–5 word unique title for this chat, e.g. "Swap stablecoins", "Leverage ETH in Aave", "Borrow DAI against USDC">
SUMMARY: <one-line human-readable description of the transaction>
---
<the intent JSON object, NOT inside code fences>
```

Rules:
- The TITLE must be short and describe the intent category (not specific amounts). It is used to name the chat thread and is hidden from the user's chat view.
- The SUMMARY is shown to the user — plain English, one sentence.
- After the `---` separator, emit ONLY the raw JSON object. No markdown code fences, no commentary.
- No text before TITLE, no text after the JSON.

In Q&A mode, just answer in plain text. Do not emit a TITLE/SUMMARY/JSON block."#;

/// Assemble the full system prompt, substituting every `{{TOKEN}}` the
/// template uses with the resolved runtime context.
pub fn build(ctx: &RuntimeContext) -> String {
    let wallet = ctx
        .wallet
        .clone()
        .unwrap_or_else(|| "(none — ask the user to connect)".to_string());

    let balances_line = ctx
        .balances_summary
        .as_ref()
        .map(|s| format!("- Current balances: {s}"))
        .unwrap_or_default();

    let prices_line = ctx
        .prices_summary
        .as_ref()
        .map(|s| format!("- Current prices (USD, spot): {s}"))
        .unwrap_or_default();

    let timestamp_line = format!("- Current Unix timestamp (seconds): {}", ctx.timestamp);
    let positions = ctx.positions.clone().unwrap_or_default();

    TEMPLATE
        .replace("{{OUTPUT_FORMAT_INSTRUCTIONS}}", TEXT_OUTPUT_FORMAT)
        .replace("{{WALLET_ADDRESS}}", &wallet)
        .replace("{{NETWORK}}", &ctx.network)
        .replace("{{BALANCES_LINE}}", &balances_line)
        .replace("{{PRICES_LINE}}", &prices_line)
        .replace("{{TIMESTAMP_LINE}}", &timestamp_line)
        .replace("{{POSITIONS_BLOCK}}", &positions)
}
