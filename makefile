# Load .env file if it exists (provides ETH_RPC_URL for fork tests)
-include .env
export

# Default mainnet RPC URL for Anvil-fork and Foundry-fork tests.
# `?=` only sets it when not already defined, so .env or a shell-exported
# value still takes precedence.
ETH_RPC_URL ?= https://ethereum-rpc.publicnode.com

.PHONY: format build test test-compiler generate-calldata generate-fixtures \
	generate-integration-fixtures test-foundry test-router e2e-test \
	test-anvil test-fork-e2e test-fork-integration test-fork-local \
	test-all compile-intent start-anvil

format:
	cargo fmt --all

build:
	cargo build --release --workspace

# Primary offline test target: everything that does NOT need an RPC/fork.
# Runs the Rust compiler tests, then (after fixture generation) the Foundry
# tests that don't hit the fork.
test: test-compiler generate-calldata test-foundry

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

# Run Foundry tests excluding fork E2E (requires: make generate-calldata first)
# Fork E2E tests need --fork-url and are run separately via make test-fork-e2e
test-foundry:
	cd contracts && forge test --no-match-contract IntentForkE2E -vvv

# Full test flow: generate calldata, then run Foundry tests
test-router: generate-calldata test-foundry

# Primary end-to-end target: everything that requires an Anvil fork.
# Uses ETH_RPC_URL (defaulted above to the public node).
e2e-test: generate-fixtures test-anvil test-fork-e2e

# Run Anvil fork tests (defaults to the public node via ETH_RPC_URL)
test-anvil:
	cargo test -p evm-testing -- --nocapture

# Run Foundry fork E2E tests against mainnet
# These deploy IntentRouter on a fork and execute against real protocols
test-fork-e2e: generate-fixtures
	cd contracts && forge test --mc IntentForkE2E --fork-url $(ETH_RPC_URL) -vvv

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

# Start a local anvil node forking Ethereum L1 with chain id 31337
start-anvil:
	./scripts/start-anvil.sh
