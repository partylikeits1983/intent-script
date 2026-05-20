//! LLM-response parser — the Rust port of `parseLLMResponse()` from
//! `intentOS-ui/lib/parse-llm-response.ts`.
//!
//! Expected text-mode shape:
//!
//!   TITLE: <short chat title — optional>
//!   SUMMARY: <one-line description>
//!   ---
//!   { "network": "anvil", "from": "0x...", "steps": [...] }
//!
//! Resilient to: missing TITLE, markdown code fences, and JSON-only bodies.

use eyre::{Result, eyre};

/// A parsed text-mode response.
pub struct ParsedIntent {
    pub title: Option<String>,
    pub summary: String,
    /// The raw intent JSON string, ready to hand to `intent_script::compile`.
    pub intent_json: String,
}

const MAX_TITLE_LEN: usize = 60;

/// Parse a raw LLM response into title + summary + intent JSON.
///
/// Returns `Err` if no JSON object with the required `network` / `from` /
/// `steps` fields can be found — i.e. the model answered in Q&A mode rather
/// than emitting an intent.
pub fn parse_llm_response(raw: &str) -> Result<ParsedIntent> {
    let trimmed = raw.trim();

    let title = line_value(trimmed, "TITLE:").map(|t| sanitize_title(&t));
    let summary = line_value(trimmed, "SUMMARY:").unwrap_or_default();

    let intent_json = extract_json(trimmed)
        .ok_or_else(|| eyre!("no intent JSON found — the model did not emit an intent"))?;

    let parsed: serde_json::Value = serde_json::from_str(&intent_json)
        .map_err(|e| eyre!("the model emitted a JSON-shaped block that does not parse: {e}"))?;

    for field in ["network", "from", "steps"] {
        if parsed.get(field).is_none() {
            return Err(eyre!("intent JSON is missing the required field '{field}'"));
        }
    }

    let summary = if summary.is_empty() {
        "Execute transaction".to_string()
    } else {
        summary
    };

    Ok(ParsedIntent {
        title,
        summary,
        intent_json,
    })
}

/// First line that starts with `prefix`, with the prefix stripped and trimmed.
fn line_value(text: &str, prefix: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(prefix).map(|v| v.trim().to_string()))
        .filter(|v| !v.is_empty())
}

/// Strip wrapping quotes/backticks and clamp the title length.
fn sanitize_title(raw: &str) -> String {
    let mut t = raw.trim();
    for q in ['"', '\'', '`'] {
        if t.len() >= 2 && t.starts_with(q) && t.ends_with(q) {
            t = t[1..t.len() - 1].trim();
        }
    }
    if t.chars().count() > MAX_TITLE_LEN {
        let cut: String = t.chars().take(MAX_TITLE_LEN - 1).collect();
        format!("{cut}…")
    } else {
        t.to_string()
    }
}

/// Extract a JSON object: after a `---` separator, then a code fence, then
/// anywhere in the text. Mirrors `extractJson()` in the TS source.
fn extract_json(text: &str) -> Option<String> {
    // 1. After a `---` separator.
    if let Some(idx) = text.find("---")
        && let Some(json) = find_json_object(text[idx + 3..].trim())
    {
        return Some(json);
    }

    // 2. Inside a ``` / ```json fence.
    if let Some(fenced) = extract_code_fence(text)
        && let Some(json) = find_json_object(fenced.trim())
    {
        return Some(json);
    }

    // 3. Anywhere.
    find_json_object(text)
}

/// Pull the body of the first ```...``` fence, if any.
fn extract_code_fence(text: &str) -> Option<&str> {
    let open = text.find("```")?;
    let after = &text[open + 3..];
    // Skip an optional language tag up to the newline.
    let body_start = after.find('\n').map(|n| n + 1).unwrap_or(0);
    let body = &after[body_start..];
    let close = body.find("```")?;
    Some(&body[..close])
}

/// Find the first complete `{...}` object by brace-matching, string-aware.
/// Direct port of `findJsonObject()` from the TS source.
fn find_json_object(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;

    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;

    for (i, &ch) in bytes.iter().enumerate().skip(start) {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            b'\\' => escaped = true,
            b'"' => in_string = !in_string,
            _ if in_string => {}
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}
