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

# Run Foundry tests (requires: make generate-calldata first)
test-foundry:
	cd contracts && forge test -vvv

# Full test flow: generate calldata, then run Foundry tests
test-router: generate-calldata test-foundry

# Run Anvil fork tests (requires ETH_RPC_URL or uses public node)
test-anvil:
	cargo test -p evm-testing -- --nocapture

# Run all tests: compiler + foundry + anvil
test-all: test-compiler test-router test-anvil
