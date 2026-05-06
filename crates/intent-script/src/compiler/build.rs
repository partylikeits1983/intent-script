//! Stage G: Build — produce final unsigned transactions from the execution plan.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::SolCall;
use hashbrown::HashMap;

use crate::compiler::plan::ExecutionPlan;
use crate::eip712;
use crate::ir::ConcreteCall;
use crate::output::{
    CallData, CompileOutput, Eip712Domain, Eip712IntentOutput, IntentBatchData, UnsignedTx,
};

// ABI definition for IntentRouter.executeDirect(), ERC-20 approve(),
// and Aave V3 vDebtToken.approveDelegation().
alloy_sol_types::sol! {
    struct RouterCall {
        address target;
        bytes callData;
        uint256 value;
    }

    function executeDirect(RouterCall[] calls, address[] tokensToSweep) external payable;

    function approve(address spender, uint256 value) external returns (bool);

    function approveDelegation(address delegatee, uint256 amount) external returns (bool);
}

/// Encode an ERC-20 `approve(spender, amount)` calldata.
fn encode_erc20_approve(spender: Address, amount: U256) -> Bytes {
    Bytes::from(
        approveCall {
            spender,
            value: amount,
        }
        .abi_encode(),
    )
}

/// Encode an Aave V3 vDebtToken `approveDelegation(delegatee, amount)` calldata.
fn encode_credit_delegation(delegatee: Address, amount: U256) -> Bytes {
    Bytes::from(approveDelegationCall { delegatee, amount }.abi_encode())
}

/// Format a short hex address like "0xC02a…6Cc2" for descriptions.
fn short_addr(addr: Address) -> String {
    let s = format!("{:?}", addr);
    if s.len() >= 42 {
        format!("{}…{}", &s[..6], &s[s.len() - 4..])
    } else {
        s
    }
}

/// Build the final compile output from an execution plan.
///
/// `current_allowances` / `required_pulls` and `current_delegations` /
/// `required_delegations` drive prerequisite-tx emission for the `Batched`
/// path:
/// - `required_pulls` is the aggregate amount the router will `transferFrom`
///   out of the signer for each token in this batch (populated by the
///   enricher). `current_allowances` is the caller-supplied snapshot of
///   on-chain `allowance(signer, router)` per token; an
///   `approve(router, amount)` UnsignedTx is prepended for every
///   under-allowanced token.
/// - `required_delegations` is the aggregate Aave V3 borrow amount that will
///   be drawn against the signer's credit-line through the router, keyed by
///   variable-debt-token address. `current_delegations` is the snapshot of
///   `borrowAllowance(signer, router)` per vDebt token; a
///   `vDebt.approveDelegation(router, amount)` UnsignedTx is prepended for
///   every under-delegated token (Aave V3 reverts with custom error
///   `0x1cb19ef3` / InsufficientBorrowAllowance otherwise).
///
/// When the corresponding snapshot is `None` (the legacy 4-arg `compile()`
/// entry point), no prerequisite txs of that kind are emitted — preserving
/// today's output byte-for-byte for callers that don't supply allowances.
#[allow(clippy::too_many_arguments)]
pub fn build(
    plan: ExecutionPlan,
    chain_id: u64,
    signer: Address,
    nonce: u64,
    deadline: u64,
    current_allowances: Option<&HashMap<Address, U256>>,
    required_pulls: &[(Address, U256)],
    current_delegations: Option<&HashMap<Address, U256>>,
    required_delegations: &[(Address, U256)],
) -> CompileOutput {
    match plan {
        ExecutionPlan::Single(call) => {
            CompileOutput::SingleTx(call_to_unsigned_tx(call, chain_id, signer))
        }
        ExecutionPlan::Sequence(calls) => {
            let txs = calls
                .into_iter()
                .map(|c| call_to_unsigned_tx(c, chain_id, signer))
                .collect();
            CompileOutput::TxSequence(txs)
        }
        ExecutionPlan::Batched {
            calls,
            router,
            tokens_to_sweep,
        } => {
            // Sum all ETH values across calls
            let total_value = calls.iter().fold(U256::ZERO, |acc, c| acc + c.value);

            // Build descriptions
            let descriptions: Vec<String> = calls.iter().map(|c| c.description.clone()).collect();
            let description = format!("Batched via router: [{}]", descriptions.join(", "));

            // Build CallData for EIP-712
            let call_data_vec: Vec<CallData> = calls
                .iter()
                .map(|c| CallData {
                    target: c.to,
                    call_data: c.calldata.clone(),
                    value: c.value,
                })
                .collect();

            // Build the direct tx (executeDirect calldata)
            let router_calls: Vec<RouterCall> = calls
                .iter()
                .map(|c| RouterCall {
                    target: c.to,
                    callData: c.calldata.to_vec().into(),
                    value: c.value,
                })
                .collect();

            let direct_calldata = executeDirectCall {
                calls: router_calls,
                tokensToSweep: tokens_to_sweep.clone(),
            }
            .abi_encode();

            let direct_tx = UnsignedTx {
                to: router,
                data: Bytes::from(direct_calldata),
                value: total_value,
                chain_id,
                from: signer,
                description: description.clone(),
            };

            // Build EIP-712 domain
            let domain = Eip712Domain {
                name: "IntentRouter".to_string(),
                version: "1".to_string(),
                chain_id,
                verifying_contract: router,
            };

            // Compute EIP-712 hashes
            let domain_separator = eip712::compute_domain_separator(
                &domain.name,
                &domain.version,
                domain.chain_id,
                domain.verifying_contract,
            );

            let eip712_calls: Vec<(Address, Bytes, U256)> = call_data_vec
                .iter()
                .map(|c| (c.target, c.call_data.clone(), c.value))
                .collect();

            // B9: sum of every inner call's ETH value becomes part of the
            // signed struct. The relayer must attach exactly this amount
            // when submitting executeSigned.
            let total_value: U256 = eip712_calls
                .iter()
                .map(|(_, _, v)| *v)
                .fold(U256::ZERO, |acc, v| acc.saturating_add(v));

            let struct_hash = eip712::hash_intent_batch(
                signer,
                &eip712_calls,
                &tokens_to_sweep,
                nonce,
                deadline,
                total_value,
            );

            let typed_data_hash = eip712::compute_typed_data_hash(&domain_separator, &struct_hash);

            let intent_batch = IntentBatchData {
                signer,
                calls: call_data_vec,
                tokens_to_sweep,
                nonce,
                deadline,
                total_value,
            };

            // Build prerequisite approvals. When `current_allowances` is None
            // (the legacy 4-arg `compile()` entry point), we emit nothing —
            // backwards-compatible with every existing test and caller.
            let mut prerequisite_approvals: Vec<UnsignedTx> = match current_allowances {
                Some(allowances) => required_pulls
                    .iter()
                    .filter(|(token, need)| {
                        allowances.get(token).copied().unwrap_or(U256::ZERO) < *need
                    })
                    .map(|(token, need)| UnsignedTx {
                        to: *token,
                        data: encode_erc20_approve(router, *need),
                        value: U256::ZERO,
                        chain_id,
                        from: signer,
                        description: format!("Approve {} for IntentRouter", short_addr(*token)),
                    })
                    .collect(),
                None => Vec::new(),
            };

            // Append credit-delegation prerequisites for Aave V3 borrows.
            // Filtered against `current_delegations` (the user's on-chain
            // `borrowAllowance(signer, router)`) so a max-approved vDebt
            // doesn't get re-prompted on every batch. Same `None == legacy`
            // contract as the ERC-20 path.
            if let Some(delegations) = current_delegations {
                for (vdebt, need) in required_delegations.iter() {
                    let current = delegations.get(vdebt).copied().unwrap_or(U256::ZERO);
                    if current >= *need {
                        continue;
                    }
                    prerequisite_approvals.push(UnsignedTx {
                        to: *vdebt,
                        data: encode_credit_delegation(router, *need),
                        value: U256::ZERO,
                        chain_id,
                        from: signer,
                        description: format!(
                            "Approve credit delegation: vDebt {} for IntentRouter",
                            short_addr(*vdebt)
                        ),
                    });
                }
            }

            CompileOutput::Eip712Intent(Eip712IntentOutput {
                domain,
                intent_batch,
                typed_data_hash,
                description,
                direct_tx,
                prerequisite_approvals,
            })
        }
    }
}

fn call_to_unsigned_tx(call: ConcreteCall, chain_id: u64, from: Address) -> UnsignedTx {
    UnsignedTx {
        to: call.to,
        data: call.calldata,
        value: call.value,
        chain_id,
        from,
        description: call.description,
    }
}
