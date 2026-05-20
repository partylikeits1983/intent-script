//! Phase 3 — fork simulation. The Rust equivalent of the frontend's
//! `simulateCompiledOutput()` (`intentOS-ui/lib/simulate-transaction.ts`):
//! fork the chain, replay the compiled transactions, and report per-asset
//! balance deltas plus any revert.
//!
//! Requires the `anvil` binary (from Foundry) on `PATH`.

use std::collections::{BTreeMap, BTreeSet};

use alloy::network::TransactionBuilder;
use alloy::node_bindings::Anvil;
use alloy::providers::ext::AnvilApi;
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::TransactionRequest;
use alloy_primitives::{Address, U256};
use eyre::{Result, eyre};

use intent_script::output::{CompileOutput, UnsignedTx};

use crate::chain::{fmt_units, read_balances};
use crate::config::AssetMap;

/// Per-transaction outcome.
pub struct TxReport {
    pub description: String,
    pub success: bool,
    pub gas_used: u64,
    pub revert_reason: Option<String>,
}

/// Signed balance change for one asset over the whole simulated batch.
pub struct AssetDelta {
    pub symbol: String,
    pub before: String,
    pub after: String,
    /// Signed human-readable change, e.g. `"+2.0"` / `"-1000"`.
    pub change: String,
}

/// The full simulation result.
pub struct SimReport {
    pub txs: Vec<TxReport>,
    pub deltas: Vec<AssetDelta>,
    pub warnings: Vec<String>,
    /// `true` only if every transaction succeeded.
    pub all_ok: bool,
}

/// Flatten a `CompileOutput` into the ordered list of txs to replay.
fn extract_txs(output: &CompileOutput) -> Vec<&UnsignedTx> {
    match output {
        CompileOutput::SingleTx(tx) => vec![tx],
        CompileOutput::Eip712Intent(intent) => vec![&intent.direct_tx],
        CompileOutput::TxSequence(txs) => txs.iter().collect(),
        CompileOutput::RequiresExecutor { .. } => vec![],
    }
}

/// Convert an `intent_script::UnsignedTx` to an Alloy `TransactionRequest`.
fn to_alloy_tx(tx: &UnsignedTx) -> TransactionRequest {
    let mut req = TransactionRequest::default();
    req.set_from(tx.from);
    req.set_to(tx.to);
    req.set_value(tx.value);
    req.set_input(tx.data.clone());
    req.set_chain_id(tx.chain_id);
    req
}

/// Simulate `output` against an Anvil fork of `rpc`.
///
/// `from` is the signer; it is impersonated and gas-funded on the fork so the
/// transactions can be replayed without a private key.
pub async fn simulate(
    output: &CompileOutput,
    rpc: &str,
    chain_id: u64,
    from: Address,
    assets: &AssetMap,
) -> Result<SimReport> {
    let txs = extract_txs(output);
    if txs.is_empty() {
        return Err(eyre!(
            "compiled output is not directly executable (RequiresExecutor) — nothing to simulate"
        ));
    }

    // Fork the chain. `chain_id` is pinned to what `chains.json` declares so
    // the compiled txs (which carry that id) are not rejected by the node.
    let anvil = Anvil::new()
        .fork(rpc)
        .chain_id(chain_id)
        .try_spawn()
        .map_err(|e| eyre!("failed to spawn Anvil fork (is `anvil` on PATH?): {e}"))?;
    let provider = ProviderBuilder::new().connect_http(anvil.endpoint_url());

    // Impersonate the signer and give it gas money.
    provider.anvil_impersonate_account(from).await?;
    provider
        .anvil_set_balance(from, U256::from(100u64) * U256::from(10u64).pow(U256::from(18u64)))
        .await?;

    let mut warnings = Vec::new();

    // A batched intent calls the IntentRouter; on a bare mainnet fork the
    // router is usually not deployed, which would revert with no useful
    // reason. Warn early so the failure is understood.
    if let CompileOutput::Eip712Intent(intent) = output {
        let code = provider.get_code_at(intent.direct_tx.to).await?;
        if code.is_empty() {
            warnings.push(format!(
                "IntentRouter is not deployed at {} on this fork — batched-intent \
                 simulation will revert. Point --rpc at an RPC where the router is \
                 deployed (e.g. the intentOS local Anvil), or deploy it with \
                 contracts/script/DeployIntentRouter.s.sol.",
                intent.direct_tx.to
            ));
        }
    }

    let pre = read_balances(&provider, from, assets).await?;

    let mut reports = Vec::new();
    let mut all_ok = true;
    for tx in &txs {
        match provider.send_transaction(to_alloy_tx(tx)).await {
            Ok(pending) => {
                let receipt = pending.get_receipt().await?;
                let ok = receipt.status();
                all_ok &= ok;
                reports.push(TxReport {
                    description: tx.description.clone(),
                    success: ok,
                    gas_used: receipt.gas_used,
                    revert_reason: (!ok).then(|| "transaction reverted on-chain".to_string()),
                });
                if !ok {
                    break;
                }
            }
            Err(e) => {
                all_ok = false;
                reports.push(TxReport {
                    description: tx.description.clone(),
                    success: false,
                    gas_used: 0,
                    revert_reason: Some(e.to_string()),
                });
                break;
            }
        }
    }

    let post = read_balances(&provider, from, assets).await?;
    let deltas = diff_balances(&pre, &post, assets);

    Ok(SimReport {
        txs: reports,
        deltas,
        warnings,
        all_ok,
    })
}

/// Diff pre/post balance maps into signed, human-readable deltas. Assets that
/// did not change are omitted.
fn diff_balances(
    pre: &BTreeMap<String, U256>,
    post: &BTreeMap<String, U256>,
    assets: &AssetMap,
) -> Vec<AssetDelta> {
    let symbols: BTreeSet<&String> = pre.keys().chain(post.keys()).collect();
    let mut deltas = Vec::new();

    for symbol in symbols {
        let before = pre.get(symbol).copied().unwrap_or(U256::ZERO);
        let after = post.get(symbol).copied().unwrap_or(U256::ZERO);
        if before == after {
            continue;
        }
        let decimals = assets.get(symbol).map(|a| a.decimals).unwrap_or(18);
        let (sign, magnitude) = if after >= before {
            ("+", after - before)
        } else {
            ("-", before - after)
        };
        deltas.push(AssetDelta {
            symbol: symbol.clone(),
            before: fmt_units(before, decimals),
            after: fmt_units(after, decimals),
            change: format!("{sign}{}", fmt_units(magnitude, decimals)),
        });
    }
    deltas
}
