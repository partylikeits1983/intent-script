# Compiler: emit prerequisite ERC-20 approval txs

## Status

Implemented. The compiler now supports an optional allowances JSON input; when
provided, it emits a `prerequisiteApprovals` list of `ERC20.approve(router, amount)`
UnsignedTxs for any token whose current allowance is below the aggregate
pulled-from-user amount for this batch. When absent, behavior is identical to
before (no approvals emitted, `prerequisiteApprovals` omitted from JSON).

## Shape

The compiler gets a second, separate JSON blob (not merged into `IntentScript`):

```json
{
  "tokens": {
    "USDC": "0",
    "USDT": "115792089237316195423570985008687907853269984665640564039457584007913129639935",
    "WETH": "0"
  }
}
```

- Keyed by symbol; values are base-units decimal strings.
- Spender is implicit — the router configured for this chain in the registry.
- The LLM never produces this; the UI assembles it under the hood from a
  multicall of `allowance(user, router)` across `lib/build-allowances-json.ts`.

## Entry points

Rust:

```rust
// Legacy — unchanged signature, no approvals ever emitted. All existing
// tests exercise this path and still pass byte-for-byte.
pub fn compile(a, b, c, d) -> Result<CompileResult>;

// New — optional 5th arg. When `Some("<json>")`, the compiler filters
// `required_pulls` by the provided allowances and emits approve txs.
pub fn compile_with_allowances(a, b, c, d, allowances_json: Option<&str>) -> Result<CompileResult>;
```

WASM (`crates/intent-script-wasm`): mirrors both. The UI calls
`compile_with_allowances(…, "")` on renders where it doesn't yet have an
allowance snapshot (wallet unconnected, or first render before multicall
resolves) — empty-string behaves as `None`.

## Output

`Eip712IntentOutput::prerequisite_approvals: Vec<UnsignedTx>` — deterministic
order (sorted by token address in the enrich stage). JSON side:
`CompileOutputJson::prerequisiteApprovals?: UnsignedTxJson[]` with
`skip_serializing_if = "Vec::is_empty"`, so legacy output stays byte-stable.

Per-token `required` amounts are computed in `compiler/enrich.rs` — the same
spot that already emits `Erc20TransferFrom { from: signer, … }` for each
user-held ERC-20 pulled into the router. The `build` stage compares those
aggregates against `current_allowances` and emits one approve per
under-allowanced token.

## Backwards compatibility

- `compile()` (4-arg, `compile()` in WASM) is untouched. All existing Rust
  tests and all existing WASM consumers work without any change.
- Opt-in only. Callers that don't pass an allowances blob see today's output.
- Integration tests added in `crates/intent-script/tests/integration.rs`:
  - `test_allowances_zero_emits_approve_for_usdc_deposit`
  - `test_allowances_sufficient_emits_no_approve`
  - `test_no_allowances_arg_backcompat`
  - `test_multi_token_only_user_pulls_counted`
  - `test_multi_step_emits_one_approve_for_pulled_token`

## UI integration

- `intentOS-ui/lib/build-allowances-json.ts` — new hook `useAllowancesJson(network)`;
  multicalls `allowance(user, router)` for top tokens, returns the JSON string.
- `intentOS-ui/lib/router-address.ts` — new helper reading the protocol config
  for the router spender address (mirrors `RegistryContext::router_address()`).
- `intentOS-ui/lib/intent-compiler.ts` — loads `compile_with_allowances`;
  `compileIntent` now takes `{ network, allowancesJson? }`.
- `intentOS-ui/hooks/use-intent-compile.ts` — threads `allowancesJson` through.
- `intentOS-ui/components/finalize-intent-tool.tsx` and
  `components/chatgpt-flow.tsx` — call `useAllowancesJson(network)` and pass
  the result into the compile call.
- `intentOS-ui/lib/required-approvals.ts` — rewritten to map
  `output.prerequisiteApprovals` into `MissingApproval[]`. No more calldata
  regex, no more `extractRequiredApprovals`.
- `intentOS-ui/lib/execute-transaction.ts` — in the `eip712_intent` branch,
  sends each `prerequisiteApprovals` tx (awaiting receipt) before signing the
  EIP-712 message and broadcasting `directTx`.
- `intentOS-ui/lib/system-prompt.md` (§ "two-transaction pattern") — updated
  to describe the now-automatic approval behavior ("up to 2 signatures").

## Why separate JSON, not a field on the intent

- The LLM doesn't need to know on-chain allowance state.
- `IntentScript` stays pure DSL; existing hand-crafted test intents don't grow
  a field.
- `balances` lives on the intent because the LLM uses it for feasibility /
  borrow-warning reasoning. Allowances are a pure UI-layer concern.

## Out of scope

- EIP-2612 permit (USDC/DAI) to collapse approve + main intent into one signature.
- Aave variable-debt `approveDelegation` for borrow flows — tracked in
  `plans/fix-aave-borrow-credit-delegation.md`.
- `SingleTx` / `TxSequence` prerequisite approvals — no current adapter
  produces a `SingleTx` that pulls user ERC-20s.
