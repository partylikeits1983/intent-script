pub mod build;
pub mod enrich;
pub mod lower;
pub mod normalize;
pub mod plan;
pub mod validate;

use std::path::Path;

use crate::error::Result;
use crate::output::CompileOutput;
use crate::registry::RegistryContext;
use crate::schema::IntentScript;

/// Compile an intent script JSON string into unsigned transactions.
///
/// This is the main entry point for the compiler pipeline:
/// Parse → Normalize → Validate → Enrich → Lower → Plan → Build
pub fn compile(json_input: &str, config_dir: &Path) -> Result<CompileOutput> {
    // Stage A: Parse JSON into public AST
    let script: IntentScript = serde_json::from_str(json_input)?;

    // Load registry for the target network
    let registry = RegistryContext::load(config_dir, &script.network)?;

    // Stage B: Normalize — resolve aliases, parse amounts
    let resolved = normalize::normalize(&script, &registry)?;

    // Stage C: Validate
    validate::validate(&resolved, &registry)?;

    // Stage D: Enrich — insert approvals, wraps, etc.
    let enriched = enrich::enrich(resolved, &registry)?;

    // Stage E: Lower — convert resolved steps to concrete EVM calls
    let calls = lower::lower(&enriched, &registry)?;

    // Stage F: Plan — decide execution strategy
    let plan = plan::plan(&calls);

    // Stage G: Build — produce final unsigned transactions
    let output = build::build(plan, enriched.chain_id, enriched.signer);

    Ok(output)
}
