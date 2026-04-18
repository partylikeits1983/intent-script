//! Structured preview builder.
//!
//! Walks the resolved IR to produce a user-facing summary of what the intent
//! consumes, what it produces, and the ordered sequence of meaningful steps.
//! Auto-inserted approvals and router transfers are excluded from `steps`.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use alloy_primitives::{Address, U256};
use hashbrown::HashMap;

use crate::ir::{ResolvedIntent, ResolvedStep, step_consumes, step_produces};
use crate::output::{Preview, PreviewStepInfo, PreviewToken};
use crate::registry::RegistryContext;

/// Build a user-facing preview from a resolved intent.
pub fn build_preview(intent: &ResolvedIntent, registry: &RegistryContext) -> Preview {
    let mut inputs: HashMap<Address, U256> = HashMap::new();
    let mut outputs: HashMap<Address, U256> = HashMap::new();

    for step in &intent.steps {
        if let Some((token, amount)) = step_consumes(step) {
            *inputs.entry(token).or_insert(U256::ZERO) += amount;
        }
        if let Some((token, amount)) = step_produces(step) {
            *outputs.entry(token).or_insert(U256::ZERO) += amount;
        }
    }

    // Net out: tokens that appear on both sides are intermediate — drop the
    // overlap so the preview only shows the user's actual send/receive.
    let overlap: Vec<Address> = inputs
        .keys()
        .filter(|k| outputs.contains_key(*k))
        .copied()
        .collect();
    for token in overlap {
        let consumed = *inputs.get(&token).unwrap_or(&U256::ZERO);
        let produced = *outputs.get(&token).unwrap_or(&U256::ZERO);
        if consumed > produced {
            inputs.insert(token, consumed - produced);
            outputs.remove(&token);
        } else if produced > consumed {
            outputs.insert(token, produced - consumed);
            inputs.remove(&token);
        } else {
            // Exactly cancel out — pure intermediate.
            inputs.remove(&token);
            outputs.remove(&token);
        }
    }

    let mut preview = Preview::default();
    preview.inputs = to_preview_tokens(&inputs, registry);
    preview.outputs = to_preview_tokens(&outputs, registry);
    preview.steps = intent
        .steps
        .iter()
        .filter_map(|s| describe_step(s, registry))
        .collect();

    preview
}

fn to_preview_tokens(
    map: &HashMap<Address, U256>,
    registry: &RegistryContext,
) -> Vec<PreviewToken> {
    let mut out: Vec<PreviewToken> = map
        .iter()
        .map(|(addr, amount)| {
            let symbol = registry.symbol_for_address(addr);
            let decimals = registry.decimals_for_address(addr);
            PreviewToken {
                symbol: symbol.clone(),
                amount: format_amount(*amount, decimals),
                amount_raw: amount.to_string(),
                address: if *addr == Address::ZERO {
                    "native".to_string()
                } else {
                    format!("{:?}", addr)
                },
            }
        })
        .collect();
    // Stable ordering for deterministic output.
    out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    out
}

/// Format a raw amount with the given decimals, trimming trailing zeros but
/// keeping at least one decimal digit when fractional.
fn format_amount(amount: U256, decimals: u8) -> String {
    if decimals == 0 {
        return amount.to_string();
    }
    let s = amount.to_string();
    let d = decimals as usize;
    if s.len() <= d {
        let pad = d - s.len();
        let mut frac = "0".repeat(pad);
        frac.push_str(&s);
        trim_zeros(&format!("0.{}", frac))
    } else {
        let split = s.len() - d;
        let (whole, frac) = s.split_at(split);
        trim_zeros(&format!("{}.{}", whole, frac))
    }
}

fn trim_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Produce a preview step entry for user-meaningful steps. Returns None for
/// auto-inserted approvals, transferFroms, and permits.
fn describe_step(step: &ResolvedStep, registry: &RegistryContext) -> Option<PreviewStepInfo> {
    match step {
        ResolvedStep::Erc20Approve { .. }
        | ResolvedStep::Erc20TransferFrom { .. }
        | ResolvedStep::Erc20Permit { .. } => None,

        ResolvedStep::Wrap {
            wrapped_token,
            amount,
        } => {
            let sym = registry.symbol_for_address(wrapped_token);
            let dec = registry.decimals_for_address(wrapped_token);
            Some(PreviewStepInfo {
                action: "wrap".into(),
                protocol: "weth".into(),
                description: format!(
                    "Wrap {} {} to {}",
                    format_amount(*amount, dec),
                    registry.chain.native_asset,
                    sym
                ),
            })
        }
        ResolvedStep::Unwrap {
            wrapped_token,
            amount,
        } => {
            let sym = registry.symbol_for_address(wrapped_token);
            let dec = registry.decimals_for_address(wrapped_token);
            Some(PreviewStepInfo {
                action: "unwrap".into(),
                protocol: "weth".into(),
                description: format!(
                    "Unwrap {} {} to {}",
                    format_amount(*amount, dec),
                    sym,
                    registry.chain.native_asset
                ),
            })
        }
        ResolvedStep::UniswapV3Swap {
            token_in,
            token_out,
            amount_in,
            amount_out_minimum,
            ..
        } => {
            let in_sym = registry.symbol_for_address(token_in);
            let out_sym = registry.symbol_for_address(token_out);
            let in_dec = registry.decimals_for_address(token_in);
            let out_dec = registry.decimals_for_address(token_out);
            Some(PreviewStepInfo {
                action: "swap".into(),
                protocol: "uniswap_v3".into(),
                description: format!(
                    "Swap {} {} for at least {} {}",
                    format_amount(*amount_in, in_dec),
                    in_sym,
                    format_amount(*amount_out_minimum, out_dec),
                    out_sym
                ),
            })
        }
        ResolvedStep::OneInchSwap {
            token_in,
            token_out,
            amount_in,
            ..
        } => {
            let in_sym = registry.symbol_for_address(token_in);
            let out_sym = registry.symbol_for_address(token_out);
            let in_dec = registry.decimals_for_address(token_in);
            Some(PreviewStepInfo {
                action: "swap".into(),
                protocol: "1inch".into(),
                description: format!(
                    "Swap {} {} for {}",
                    format_amount(*amount_in, in_dec),
                    in_sym,
                    out_sym
                ),
            })
        }
        ResolvedStep::AaveV3Supply { asset, amount, .. } => {
            let sym = registry.symbol_for_address(asset);
            let dec = registry.decimals_for_address(asset);
            Some(PreviewStepInfo {
                action: "deposit".into(),
                protocol: "aave_v3".into(),
                description: format!("Deposit {} {} to Aave V3", format_amount(*amount, dec), sym),
            })
        }
        ResolvedStep::AaveV3Borrow { asset, amount, .. } => {
            let sym = registry.symbol_for_address(asset);
            let dec = registry.decimals_for_address(asset);
            Some(PreviewStepInfo {
                action: "borrow".into(),
                protocol: "aave_v3".into(),
                description: format!(
                    "Borrow {} {} from Aave V3",
                    format_amount(*amount, dec),
                    sym
                ),
            })
        }
        ResolvedStep::AaveV3Withdraw { asset, amount, .. } => {
            let sym = registry.symbol_for_address(asset);
            let dec = registry.decimals_for_address(asset);
            Some(PreviewStepInfo {
                action: "withdraw".into(),
                protocol: "aave_v3".into(),
                description: format!(
                    "Withdraw {} {} from Aave V3",
                    format_amount(*amount, dec),
                    sym
                ),
            })
        }
        ResolvedStep::LidoStake { amount, .. } => Some(PreviewStepInfo {
            action: "stake".into(),
            protocol: "lido".into(),
            description: format!(
                "Stake {} {} with Lido",
                format_amount(*amount, 18),
                registry.chain.native_asset
            ),
        }),
        ResolvedStep::WstETHWrap { amount, .. } => Some(PreviewStepInfo {
            action: "wrap".into(),
            protocol: "lido".into(),
            description: format!("Wrap {} stETH to wstETH", format_amount(*amount, 18)),
        }),
        ResolvedStep::SendErc20 { token, amount, to } => {
            let sym = registry.symbol_for_address(token);
            let dec = registry.decimals_for_address(token);
            Some(PreviewStepInfo {
                action: "send".into(),
                protocol: "erc20".into(),
                description: format!("Send {} {} to {:?}", format_amount(*amount, dec), sym, to),
            })
        }
        ResolvedStep::SendEth { amount, to } => Some(PreviewStepInfo {
            action: "send".into(),
            protocol: "native".into(),
            description: format!(
                "Send {} {} to {:?}",
                format_amount(*amount, 18),
                registry.chain.native_asset,
                to
            ),
        }),
        ResolvedStep::SendErc721 { token_id, to, .. } => Some(PreviewStepInfo {
            action: "send".into(),
            protocol: "erc721".into(),
            description: format!("Send NFT #{} to {:?}", token_id, to),
        }),
    }
}
