pub mod build;
pub mod enrich;
pub mod lower;
pub mod normalize;
pub mod plan;
pub mod preview;
pub mod validate;

use crate::error::Result;
use crate::output::{CompileOutput, CompileResult};
use crate::registry::RegistryContext;
use crate::schema::IntentScript;

/// Compile an intent script JSON string into unsigned transactions.
///
/// This is the main entry point for the compiler pipeline:
/// Parse → Normalize → Validate → Enrich → Lower → Plan → Build
///
/// The caller provides pre-loaded config JSON strings (no file I/O in the library).
///
/// Returns a `CompileResult` containing the output and any warnings
/// from validation (e.g., borrow without prior deposit when no balance info).
///
/// When a router address is configured in the registry and the intent
/// produces multiple calls, they are automatically batched into a single
/// `router.execute()` transaction.
pub fn compile(
    json_input: &str,
    chains_json: &str,
    assets_json: &str,
    protocols_json: &str,
) -> Result<CompileResult> {
    // Stage A: Parse JSON into public AST
    let script: IntentScript = serde_json::from_str(json_input)?;

    // Load registry for the target network
    let registry =
        RegistryContext::load(chains_json, assets_json, protocols_json, &script.network)?;

    // Stage B: Normalize — resolve aliases, parse amounts
    let norm_result = normalize::normalize(&script, &registry)?;
    let resolved = norm_result.intent;
    let mut all_warnings = norm_result.warnings;

    // Stage C: Validate — returns warnings for non-fatal issues
    let validation = validate::validate(&resolved, &registry)?;
    all_warnings.extend(validation.warnings);

    // Build the user-facing preview from the resolved (pre-enrich) steps so
    // auto-inserted approvals/transferFroms never appear in the summary.
    let preview = preview::build_preview(&resolved, &registry);

    // Stage D: Enrich — insert approvals, wraps, track sweep tokens
    let enriched = enrich::enrich(resolved, &registry)?;

    // Stage E: Lower — convert resolved steps to concrete EVM calls
    let calls = lower::lower(&enriched, &registry)?;

    // Stage F: Plan — decide execution strategy
    // Pass router address and sweep tokens for batching decision
    let router = registry.router_address();
    let plan = plan::plan(&calls, router, enriched.tokens_to_sweep);

    // Stage G: Build — produce final unsigned transactions
    let output = build::build(
        plan,
        enriched.chain_id,
        enriched.signer,
        enriched.nonce,
        enriched.deadline,
    );

    // Only batched (Eip712Intent) outputs need a deadline — `executeSigned`
    // rejects `deadline == 0`. `executeDirect` (the single-tx path) does not
    // check the deadline, so warning there would just be noise.
    let has_deadline_source =
        script.deadline.unwrap_or(0) > 0 || script.current_timestamp.is_some();
    if matches!(output, CompileOutput::Eip712Intent(_)) && !has_deadline_source {
        all_warnings.push(
            "Intent has no deadline: neither 'deadline' nor 'current_timestamp' was provided. \
             Batched intents will be rejected by the router (deadline > 0 required). \
             Set 'current_timestamp' to the current Unix timestamp to auto-compute a 30-minute deadline."
                .into(),
        );
    }

    Ok(CompileResult {
        output,
        warnings: all_warnings,
        preview: Some(preview),
    })
}
