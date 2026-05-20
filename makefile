# Load .env file if it exists (provides ETH_RPC_URL for fork tests)
-include .env
export

# Default mainnet RPC URL for Anvil-fork and Foundry-fork tests.
# `?=` only sets it when not already defined, so .env or a shell-exported
# value still takes precedence.
ETH_RPC_URL ?= https://ethereum-rpc.publicnode.com

.PHONY: ci fmt-check clippy format build test test-compiler generate-calldata \
	generate-fixtures generate-integration-fixtures test-foundry test-router \
	e2e-test test-anvil test-fork-e2e test-fork-integration test-fork-local \
	test-all test-e2e-advisor compile-intent start start-anvil start-anvil-base \
	start-l1 wasm-build server-compat advisor sync-advisor-prompt

format:
	cargo fmt --all

build:
	cargo build --release --workspace

# Primary test target: Rust compiler tests, offline Foundry tests, and the
# fork-mode E2E suites (which need ETH_RPC_URL — defaulted above to the
# public node). test-fork-e2e matches `^IntentFork.*E2E$$`, picking up
# IntentForkScenariosE2E and its `test_fork_stakeWrapDepositBorrow` case.
test: test-compiler generate-calldata test-foundry test-fork-e2e

# Rust compiler tests only — no network, no fork, no Foundry.
test-compiler:
	cargo test -p intent-script

# Generate calldata fixture files for Foundry tests
generate-calldata:
	cargo test -p intent-script --test generate_calldata -- --nocapture

# Generate EIP-712 batch fixture files for fork tests
generate-fixtures: generate-calldata generate-integration-fixtures
	cargo test -p intent-script --test generate_eip712_fixtures -- --nocapture

# Generate the *_batch.bin / *_single.bin fixtures consumed by
# IntentForkIntegration.t.sol's eight DSL-driven scenarios.
generate-integration-fixtures:
	cargo test -p intent-script --test generate_integration_fixtures -- --nocapture

# Run Foundry tests excluding fork suites (requires: make generate-calldata first).
# Every IntentFork* contract needs --fork-url and is run separately via
# make test-fork-e2e / make test-fork-integration. Use a prefix-anchored
# regex so any new IntentFork* suite is auto-excluded — without this, a
# new contract whose name doesn't literally contain "IntentForkE2E" or
# "IntentForkIntegration" (e.g. IntentForkScenariosE2E) would silently
# run here without a fork and revert in setUp.
test-foundry:
	cd contracts && forge test --no-match-contract '^IntentFork' -vvv

# Full test flow: generate calldata, then run Foundry tests
test-router: generate-calldata test-foundry

# Primary end-to-end target: everything that requires an Anvil fork.
# Uses ETH_RPC_URL (defaulted above to the public node).
e2e-test: generate-fixtures test-anvil test-fork-e2e

# Run Anvil fork tests (defaults to the public node via ETH_RPC_URL)
test-anvil:
	cargo test -p evm-testing -- --nocapture

# Run Foundry fork E2E tests against mainnet
# These deploy IntentRouter on a fork and execute against real protocols.
# The regex matches any IntentFork* contract whose name ends in `E2E` —
# IntentForkE2E (the original primitive-by-primitive suite) plus
# IntentForkScenariosE2E (user-flow scenarios). Add new fork-mode
# *E2E suites with no Makefile change.
test-fork-e2e: generate-fixtures
	cd contracts && forge test --mc '^IntentFork.*E2E$$' --fork-url $(ETH_RPC_URL) -vvv

# Run the DSL → compile → sign → executeSigned integration suite on fork.
test-fork-integration: generate-integration-fixtures
	cd contracts && forge test --mc IntentForkIntegration --fork-url $(ETH_RPC_URL) -vvv

# Run legacy fork tests (local mock-based, misleadingly named)
test-fork-local:
	cd contracts && forge test --mc IntentLocalTests -vvv

# Run all tests: compiler + foundry + anvil (no fork needed)
test-all: test-compiler test-router test-anvil

# Compile a JSON intent file (default: examples/test.json)
compile-intent:
	cargo run -p intent-script --features clap -- crates/intent-script/examples/test.json --pretty

# Default fork target: Base mainnet at chain id 8453, port 8545.
# Delegates to the rich root-level script which (a) deploys IntentRouter via
# DeployIntentRouterBase.s.sol with constructor `(0x0, AAVE_POOL_BASE)`,
# (b) funds dev accounts with USDC + cbETH via whale impersonation, and
# (c) auto-updates `intent_router.contracts.router` in both Base config
# JSONs. Use `make start-l1` for the legacy Ethereum L1 fork.
start: start-anvil-base

start-anvil-base:
	../scripts/run-local-anvil-base.sh

# Forks Ethereum L1 with chain id 31337. Deploys IntentRouter with both
# Balancer and Aave wired so leverage `via: balancer` (default) and
# `via: aave` (with premium) both work. Mirrors run-local-anvil.sh.
start-anvil:
	./scripts/start-anvil.sh

# Alias for the L1 fork. Kept for muscle-memory parity with `make start`.
start-l1: start-anvil

# Natural-language advisor: plain English -> DSL -> compile -> validate.
# Needs OPENAI_API_KEY in the environment or .env. Add --simulate --rpc <url>
# for a fork simulation (see `cargo run ... --bin advisor -- --help`).
ADVISOR_PROMPT ?= deposit 5000 USDC into aave
advisor:
	cargo run -p intent-script --features advisor --bin advisor -- \
		"$(ADVISOR_PROMPT)" \
		--context crates/intent-script/examples/advisor-context.json \
		--config-dir ./config --pretty

# Cross-stack e2e for the advisor binary: spawns a local Anvil + IntentRouter
# via `scripts/start-anvil.sh`, then runs the compiled `advisor` with
# `--simulate --rpc <local>` and asserts the on-chain delta. Needs
# OPENAI_API_KEY (in env or .env) plus anvil, cast, forge, jq on PATH —
# skip-not-fail if any are missing. Use --test-threads=1 so multiple cases
# don't race on port allocation. See WS-3C-full-stack-e2e.md.
test-e2e-advisor:
	cargo test -p intent-script --features advisor --test advisor_e2e -- \
		--ignored --nocapture --test-threads=1

# Re-copy the canonical system prompt from the frontend into the advisor crate.
# Run this whenever intentOS-ui/lib/system-prompt.md changes.
sync-advisor-prompt:
	cp ../intentOS-ui/lib/system-prompt.md \
		crates/intent-script/src/bin/advisor/system-prompt.md
	@echo "advisor system prompt synced"

# Run the same gate the GitHub Actions `ci` workflow runs locally.
# Skips test-evm (needs ETH_RPC_URL) — run `make test-anvil` separately
# when you want fork coverage. Skips wasm-build if wasm-pack is missing.
fmt-check:
	cargo fmt --all -- --check

clippy:
	cargo clippy --all-targets -- -D warnings

# Verify intent-script compiles as a non-WASM library dep (the shape consumed
# by intentOS-server's WS-1C compile handler).
server-compat:
	cargo check -p intent-script --all-targets

# Build the browser WASM bundle the UI consumes via `pnpm build:wasm`.
# Soft-skip when wasm-pack isn't installed locally.
wasm-build:
	@if command -v wasm-pack >/dev/null 2>&1; then \
		wasm-pack build --target web --release crates/intent-script-wasm; \
	else \
		echo "wasm-pack not installed — skipping wasm-build (CI runs this job in GHA)"; \
	fi

ci: fmt-check clippy test-compiler generate-calldata generate-integration-fixtures test-foundry server-compat wasm-build
	@echo "ci ✓"
