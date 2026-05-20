//! The OpenAI call, via the [Rig](https://github.com/0xPlaygrounds/rig) crate.
//!
//! Equivalent to the frontend's `streamText({ model, system, messages })` in
//! `app/api/v1/chat/route.ts` — one system prompt (preamble), one user turn,
//! one completion back. The `OPENAI_API_KEY` env var is the local-CLI
//! equivalent of the frontend's BYOK key.
//!
//! We deliberately use the lower-level `Completion::completion(...).send()`
//! path rather than `.prompt(...)` — `.prompt(...)` returns a bare `String`
//! and discards the token-usage metadata we need to price the call.

use eyre::{Result, eyre};
use rig::client::{CompletionClient, ProviderClient};
use rig::completion::{AssistantContent, Completion, Message, Usage};
use rig::providers::openai;

/// What a single LLM call gave us back.
pub struct LlmResponse {
    pub text: String,
    pub usage: Usage,
}

/// Send `instruction` to `model` with `system_prompt` as the preamble.
pub async fn ask(model: &str, system_prompt: &str, instruction: &str) -> Result<LlmResponse> {
    let client = openai::Client::from_env()
        .map_err(|e| eyre!("OpenAI client init failed (is OPENAI_API_KEY set?): {e}"))?;

    // Newer reasoning / efficient models (o-series, gpt-5-nano, gpt-5-mini)
    // reject `temperature` outright. We auto-skip it for those families so
    // swapping `ADVISOR_MODEL` in `.env` "just works"; for older models we
    // pin temperature to 0.0 to keep DSL output deterministic.
    let mut builder = client.agent(model).preamble(system_prompt);
    if supports_temperature(model) {
        builder = builder.temperature(0.0);
    }
    let agent = builder.build();

    let request = agent
        .completion(instruction, Vec::<Message>::new())
        .await
        .map_err(|e| eyre!("failed to build completion request: {e}"))?;
    let response = request
        .send()
        .await
        .map_err(|e| eyre!("OpenAI completion failed: {e}"))?;

    let text = response
        .choice
        .iter()
        .find_map(|c| match c {
            AssistantContent::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .ok_or_else(|| eyre!("model returned no text content (only tool calls / reasoning)"))?;

    Ok(LlmResponse {
        text,
        usage: response.usage,
    })
}

/// `true` if the model accepts the `temperature` sampling parameter.
///
/// Deny-listed: every o-series reasoning model and the lighter gpt-5
/// variants. The full `gpt-5` and gpt-4*/gpt-3.5 families still take it.
/// Extend `NO_TEMP` when OpenAI ships another model that rejects it.
fn supports_temperature(model: &str) -> bool {
    // gpt-5 family is the awkward one: `gpt-5.1` / `gpt-5.2` (dotted)
    // *accept* temperature, but plain `gpt-5` and every `gpt-5-…` variant
    // (nano, mini, pro) *reject* it. Prefix-match alone can't separate
    // those, so check the dot first.
    if model.starts_with("gpt-5.") {
        return true;
    }
    if model.starts_with("gpt-5") {
        return false;
    }

    // Everything else: deny the known reasoning-tuned families.
    const NO_TEMP: &[&str] = &[
        "o1", "o3", "o4", "o5",
        "gpt-4.1-nano", "gpt-4.1-mini",
    ];
    !NO_TEMP.iter().any(|prefix| model.starts_with(prefix))
}
