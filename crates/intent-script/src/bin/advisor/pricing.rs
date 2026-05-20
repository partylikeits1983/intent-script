//! Cost estimation for OpenAI calls.
//!
//! Prices below are hard-coded USD per 1M tokens. **They change** — verify
//! against <https://openai.com/api/pricing/> and update the table when
//! OpenAI's pricing shifts or new models ship.
//!
//! Last reviewed: 2026-05.
//!
//! For models not in the table (or to override the table for one run), set
//! these in `.env` or the shell — both in USD per 1M tokens:
//!
//!   ADVISOR_PRICE_INPUT=2.50
//!   ADVISOR_PRICE_OUTPUT=10.00
//!   ADVISOR_PRICE_CACHED_INPUT=1.25   # optional; defaults to input × 0.5

use rig::completion::Usage;

#[derive(Clone, Copy)]
pub struct ModelPrice {
    /// USD per 1M input tokens.
    pub input: f64,
    /// USD per 1M output tokens.
    pub output: f64,
    /// USD per 1M cached-input tokens. `None` ⇒ defaults to `input × 0.5`,
    /// OpenAI's standard cached-prompt discount.
    pub cached_input: Option<f64>,
}

/// Hard-coded price table. Ordered longest-prefix-first so the lookup picks
/// `gpt-4o-mini` before `gpt-4o` for a request like `gpt-4o-mini-2024-07-18`.
// IMPORTANT: lookup uses `starts_with`, so longer/more-specific prefixes
// MUST come before shorter ones in the same family — e.g. `gpt-5.2-pro` must
// precede `gpt-5.2`, and `gpt-5-nano` / `gpt-5-mini` / `gpt-5-pro` must all
// precede `gpt-5`.
const TABLE: &[(&str, ModelPrice)] = &[
    // ── GPT-5 family (verified 2026-05 against openai.com/api/pricing) ──
    ("gpt-5.2-pro",   ModelPrice { input: 21.00, output: 168.00, cached_input: None }),
    ("gpt-5.2",       ModelPrice { input: 1.75,  output: 14.00,  cached_input: Some(0.175) }),
    ("gpt-5.1",       ModelPrice { input: 1.25,  output: 10.00,  cached_input: Some(0.125) }),
    ("gpt-5-nano",    ModelPrice { input: 0.05,  output: 0.40,   cached_input: Some(0.005) }),
    ("gpt-5-mini",    ModelPrice { input: 0.25,  output: 2.00,   cached_input: Some(0.025) }),
    ("gpt-5-pro",     ModelPrice { input: 15.00, output: 120.00, cached_input: None }),
    ("gpt-5",         ModelPrice { input: 1.25,  output: 10.00,  cached_input: Some(0.125) }),

    // ── GPT-4.1 family ──
    ("gpt-4.1-nano",  ModelPrice { input: 0.10,  output: 0.40,   cached_input: Some(0.025) }),
    ("gpt-4.1-mini",  ModelPrice { input: 0.40,  output: 1.60,   cached_input: Some(0.10) }),
    ("gpt-4.1",       ModelPrice { input: 2.00,  output: 8.00,   cached_input: Some(0.50) }),

    // ── GPT-4o family ──
    ("gpt-4o-mini",   ModelPrice { input: 0.15,  output: 0.60,   cached_input: Some(0.075) }),
    ("gpt-4o",        ModelPrice { input: 2.50,  output: 10.00,  cached_input: Some(1.25) }),

    // ── Older GPT-4 / 3.5 ──
    ("gpt-4-turbo",   ModelPrice { input: 10.00, output: 30.00,  cached_input: None }),
    ("gpt-4",         ModelPrice { input: 30.00, output: 60.00,  cached_input: None }),
    ("gpt-3.5-turbo", ModelPrice { input: 0.50,  output: 1.50,   cached_input: None }),

    // ── o-series reasoning models ──
    ("o4-mini",       ModelPrice { input: 1.10,  output: 4.40,   cached_input: Some(0.275) }),
    ("o3-mini",       ModelPrice { input: 1.10,  output: 4.40,   cached_input: Some(0.55) }),
    ("o3",            ModelPrice { input: 2.00,  output: 8.00,   cached_input: Some(0.50) }),
    ("o1-mini",       ModelPrice { input: 1.10,  output: 4.40,   cached_input: Some(0.55) }),
    ("o1",            ModelPrice { input: 15.00, output: 60.00,  cached_input: Some(7.50) }),
];

/// Resolve a model id to a price. Env overrides win over the table.
pub fn lookup(model: &str) -> Option<ModelPrice> {
    let env_input = std::env::var("ADVISOR_PRICE_INPUT").ok().and_then(|s| s.parse::<f64>().ok());
    let env_output = std::env::var("ADVISOR_PRICE_OUTPUT").ok().and_then(|s| s.parse::<f64>().ok());
    if let (Some(input), Some(output)) = (env_input, env_output) {
        let cached_input = std::env::var("ADVISOR_PRICE_CACHED_INPUT")
            .ok()
            .and_then(|s| s.parse::<f64>().ok());
        return Some(ModelPrice { input, output, cached_input });
    }

    TABLE
        .iter()
        .find(|(prefix, _)| model.starts_with(prefix))
        .map(|(_, price)| *price)
}

/// USD cost for a single LLM call. `None` ⇒ the model is unknown and no env
/// override is set — print tokens but not a dollar figure.
pub fn estimate_cost(model: &str, usage: &Usage) -> Option<f64> {
    let price = lookup(model)?;
    let cached_rate = price.cached_input.unwrap_or(price.input * 0.5);

    let cached = usage.cached_input_tokens as f64;
    let regular_input = (usage.input_tokens as f64 - cached).max(0.0);
    let output = usage.output_tokens as f64;

    let cost = regular_input * price.input + cached * cached_rate + output * price.output;
    Some(cost / 1_000_000.0)
}
