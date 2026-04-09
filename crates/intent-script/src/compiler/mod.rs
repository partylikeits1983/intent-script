pub mod build;
pub mod enrich;
pub mod lower;
pub mod normalize;
pub mod plan;
pub mod validate;

use crate::error::Result;
use crate::output::CompileResult;
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

    Ok(CompileResult {
        output,
        warnings: all_warnings,
    })
}
