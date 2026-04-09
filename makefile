# Load .env file if it exists (provides ETH_RPC_URL for fork tests)
-include .env
export

format:
	cargo fmt --all

build:
	cargo build --release --workspace

test:
	cargo test --workspace

# Run only the Rust compiler tests (no Anvil fork needed)
test-compiler:
	cargo test -p intent-script

# Generate calldata fixture files for Foundry tests
generate-calldata:
	cargo test -p intent-script --test generate_calldata -- --nocapture

# Generate EIP-712 batch fixture files for fork tests
generate-fixtures: generate-calldata
	cargo test -p intent-script --test generate_eip712_fixtures -- --nocapture

# Run Foundry tests (requires: make generate-calldata first)
test-foundry:
	cd contracts && forge test -vvv

# Full test flow: generate calldata, then run Foundry tests
test-router: generate-calldata test-foundry

# Run Foundry fork E2E tests against mainnet (requires ETH_RPC_URL in .env)
# These deploy IntentRouter on a fork and execute against real protocols
test-fork-e2e: generate-fixtures
	cd contracts && forge test --mc IntentForkE2E --fork-url $(ETH_RPC_URL) -vvv

# Run legacy fork tests (local mock-based, misleadingly named)
test-fork-local:
	cd contracts && forge test --mc IntentLocalTests -vvv

# Run Anvil fork tests (requires ETH_RPC_URL or uses public node)
test-anvil:
	cargo test -p evm-testing -- --nocapture

# Run all tests: compiler + foundry + anvil (no fork needed)
test-all: test-compiler test-router test-anvil

# Run everything including fork E2E (requires ETH_RPC_URL in .env)
test-e2e: test-all test-fork-e2e
