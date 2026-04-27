pub mod build;
pub mod enrich;
pub mod leverage;
pub mod lower;
pub mod normalize;
pub mod plan;
pub mod preview;
pub mod uniswap_ticks;
pub mod validate;

use alloc::string::ToString;

use alloy_primitives::{Address, U256};
use hashbrown::HashMap;

use crate::error::Result;
use crate::output::{CompileOutput, CompileResult};
use crate::registry::RegistryContext;
use crate::schema::{AllowancesInput, IntentScript};

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
    compile_with_allowances(json_input, chains_json, assets_json, protocols_json, None)
}

/// Same as [`compile`], but additionally accepts the user's current on-chain
/// ERC-20 allowances for the router as a separate JSON blob.
///
/// The JSON shape is `{ "tokens": { "<symbol>": "<base-units>", ... } }`.
/// When `allowances_json` is `Some(_)`, the compiler compares each token's
/// current allowance against the aggregate amount that will be pulled from
/// the signer into the router during batched execution, and emits an
/// `approve(router, amount)` prerequisite tx in
/// `Eip712IntentOutput::prerequisite_approvals` for each under-allowanced
/// token. Missing symbols are treated as 0 (always emits an approve).
///
/// When `None`, behavior is identical to [`compile`] — no prerequisite
/// approvals are emitted, and the JSON output does not even carry a
/// `prerequisiteApprovals` field.
pub fn compile_with_allowances(
    json_input: &str,
    chains_json: &str,
    assets_json: &str,
    protocols_json: &str,
    allowances_json: Option<&str>,
) -> Result<CompileResult> {
    // Stage A: Parse JSON into public AST. `#[serde(deny_unknown_fields)]`
    // on IntentScript and every *Step rejects hallucinated fields like
    // `recipient_override` up front — a pure-parser defense.
    let script: IntentScript = serde_json::from_str(json_input)?;

    // B12: Schema version gate. Accept absent (backward-compat) or exactly
    // "1.0". Reject any other value so a compiler built against schema 1.0
    // cannot silently reinterpret a schema-2.0 intent that happens to
    // deserialize cleanly.
    if let Some(v) = script.schema_version.as_deref()
        && v != "1.0"
    {
        return Err(crate::error::CompileError::Validation(alloc::format!(
            "Unsupported schema_version '{}': this compiler accepts '1.0' (or omitted). \
                 Upgrade the compiler or emit an intent matching the supported schema.",
            v
        )));
    }

    // Load registry for the target network
    let registry =
        RegistryContext::load(chains_json, assets_json, protocols_json, &script.network)?;

    // Parse the optional allowances blob into a (token_address → allowance) map
    // plus an optional (token_address → max_spend) cap map. Unknown symbols are
    // dropped with a warning; malformed numeric values bubble up as a parse
    // error (bad UI data is a bug, not silently ignorable).
    let mut parse_warnings: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();
    let mut max_spend_map: HashMap<Address, U256> = HashMap::new();
    let current_allowances: Option<HashMap<Address, U256>> = match allowances_json {
        Some(raw) if !raw.is_empty() => {
            let input: AllowancesInput = serde_json::from_str(raw)?;
            let mut map: HashMap<Address, U256> = HashMap::new();
            for (alias, base_units) in input.tokens.iter() {
                match registry.assets.get(alias) {
                    Some(cfg) if cfg.address != "native" => {
                        let addr: Address = cfg.address.parse().map_err(|_| {
                            crate::error::CompileError::Adapter(alloc::format!(
                                "Invalid address in asset config for '{}'",
                                alias
                            ))
                        })?;
                        let amount = U256::from_str_radix(base_units, 10).map_err(|_| {
                            crate::error::CompileError::InvalidAmount(base_units.to_string())
                        })?;
                        map.insert(addr, amount);
                    }
                    _ => {
                        parse_warnings.push(alloc::format!(
                            "Allowance entry for unknown asset '{}' ignored",
                            alias
                        ));
                    }
                }
            }
            // B4: Parse optional max_spend caps. Same alias-resolution and
            // error handling as `tokens`. Unknown aliases are dropped with a
            // warning rather than erroring so a UI that ships a stale alias
            // list doesn't brick compile — the worst-case outcome is the
            // cap silently not applying, which only ever *lets* things
            // through, not the other way around.
            for (alias, base_units) in input.max_spend.iter() {
                match registry.assets.get(alias) {
                    Some(cfg) if cfg.address != "native" => {
                        let addr: Address = cfg.address.parse().map_err(|_| {
                            crate::error::CompileError::Adapter(alloc::format!(
                                "Invalid address in asset config for '{}'",
                                alias
                            ))
                        })?;
                        let amount = U256::from_str_radix(base_units, 10).map_err(|_| {
                            crate::error::CompileError::InvalidAmount(base_units.to_string())
                        })?;
                        max_spend_map.insert(addr, amount);
                    }
                    _ => {
                        parse_warnings.push(alloc::format!(
                            "max_spend entry for unknown asset '{}' ignored",
                            alias
                        ));
                    }
                }
            }
            Some(map)
        }
        _ => None,
    };

    // Stage B: Normalize — resolve aliases, parse amounts
    let norm_result = normalize::normalize(&script, &registry)?;
    let resolved = norm_result.intent;
    let mut all_warnings = norm_result.warnings;
    all_warnings.extend(parse_warnings);

    // Stage C: Validate — returns warnings for non-fatal issues
    let validation = validate::validate(&resolved, &registry)?;
    all_warnings.extend(validation.warnings);

    // Build the user-facing preview from the resolved (pre-enrich) steps so
    // auto-inserted approvals/transferFroms never appear in the summary.
    let preview = preview::build_preview(&resolved, &registry);

    // Stage D: Enrich — insert approvals, wraps, track sweep tokens
    let enriched = enrich::enrich(resolved, &registry)?;

    // B4: Per-token spend cap. required_pulls is the aggregate amount the
    // router will pull from the signer for each ERC-20 during this batch.
    // If the caller supplied `max_spend` for a token and the required pull
    // exceeds it, reject — the intent outruns what the user pre-authorized
    // for this session.
    for (token, required) in enriched.required_pulls.iter() {
        if let Some(cap) = max_spend_map.get(token)
            && required > cap
        {
            return Err(crate::error::CompileError::Validation(alloc::format!(
                "Token {} required-pull {} exceeds the caller's max_spend cap {}. \
                     The intent tries to spend more than was pre-authorized for this session.",
                token,
                required,
                cap
            )));
        }
    }

    // Stage E: Lower — convert resolved steps to concrete EVM calls
    let calls = lower::lower(&enriched, &registry)?;

    // B5: Call budget — cap per-call ETH value, total ETH value, and
    // post-enrichment call count. These are defensive bounds that catch
    // obvious overflows (`value: 10^30`) and dust-flood shapes without
    // constraining any legitimate intent.
    validate::validate_call_budget(&calls)?;

    // Stage F: Plan — decide execution strategy
    // Pass router address and sweep tokens for batching decision
    let router = registry.router_address();
    let plan = plan::plan(
        &calls,
        router,
        enriched.tokens_to_sweep,
        enriched.requires_router,
    );

    // Stage G: Build — produce final unsigned transactions
    let output = build::build(
        plan,
        enriched.chain_id,
        enriched.signer,
        enriched.nonce,
        enriched.deadline,
        current_allowances.as_ref(),
        &enriched.required_pulls,
    );

    // Only batched (Eip712Intent) outputs need a deadline — `executeSigned`
    // rejects `deadline == 0`. `executeDirect` (the single-tx path) does not
    // check the deadline, so silence is fine there.
    //
    // Previously this was a warning. A hallucinating LLM that omits the
    // deadline produces an intent the router will reject — cheaper and safer
    // to reject at compile time than to let the user sign a doomed
    // transaction. TxSequence outputs also skip the check because they are a
    // fallback path executed as a series of raw EOA calls, not through the
    // signed-intent flow.
    if matches!(output, CompileOutput::Eip712Intent(_)) {
        let has_deadline_source =
            script.deadline.unwrap_or(0) > 0 || script.current_timestamp.is_some();
        if !has_deadline_source {
            return Err(crate::error::CompileError::DeadlineMissing);
        }
        if let (Some(explicit), Some(current)) = (script.deadline, script.current_timestamp)
            && explicit > 0
            && explicit <= current
        {
            return Err(crate::error::CompileError::DeadlineInPast {
                deadline: explicit,
                current_timestamp: current,
            });
        }
    }

    Ok(CompileResult {
        output,
        warnings: all_warnings,
        preview: Some(preview),
    })
}
