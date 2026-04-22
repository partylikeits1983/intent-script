#!/usr/bin/env bash
# Start a local anvil node forking Ethereum L1 (mainnet) with chain id 31337,
# then mint 100k USDC and 100k USDT to each of the default anvil dev accounts.
#
# Env vars:
#   ETH_RPC_URL       Upstream RPC to fork (default: https://ethereum-rpc.publicnode.com)
#   ANVIL_PORT        Port to bind (default: 8545)
#   ANVIL_BLOCK_TIME  Seconds between blocks (default: instant mining)
#
# A sibling .env file (intent-script/.env) is auto-loaded if present; existing
# shell-exported values take precedence, matching the convention in the makefile.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [ -f "$REPO_ROOT/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "$REPO_ROOT/.env"
  set +a
fi

: "${ETH_RPC_URL:=https://ethereum-rpc.publicnode.com}"
: "${ANVIL_PORT:=8545}"
RPC="http://127.0.0.1:$ANVIL_PORT"

# Mainnet token addresses (mirrors config/assets/anvil.json).
USDC=0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48
USDT=0xdAC17F958D2ee523a2206206994597C13D831ec7
# balanceOf mapping storage slots in each token's implementation.
USDC_BAL_SLOT=9
USDT_BAL_SLOT=2

# Default anvil dev accounts (mnemonic: "test test test test test test test test test test test junk").
ACCOUNTS=(
  0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
  0x70997970C51812dc3A010C7d01b50e0d17dc79C8
  0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC
  0x90F79bf6EB2c4f870365E785982E1f101E93b906
  0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65
  0x9965507D1a55bcC2695C58ba16FB37d819B0A4dc
  0x976EA74026E726554dB657fA54763abd0C3a0aa9
  0x14dC79964da2C08b23698B3D3cc7Ca32193d9955
  0x23618e81E3f5cdF7f54C3d65f7FBc0aBf5B21E8f
  0xa0Ee7A142d267C1f36714E4a8F75612F20a79720
)

# 100,000 tokens at 6 decimals = 100_000 * 10^6.
AMOUNT_HEX=$(cast to-uint256 100000000000)

echo "Starting anvil: fork=$ETH_RPC_URL chain_id=31337 port=$ANVIL_PORT"

anvil \
  --fork-url "$ETH_RPC_URL" \
  --chain-id 31337 \
  --port "$ANVIL_PORT" \
  ${ANVIL_BLOCK_TIME:+--block-time "$ANVIL_BLOCK_TIME"} &
ANVIL_PID=$!

cleanup() { kill "$ANVIL_PID" 2>/dev/null || true; }
trap cleanup INT TERM EXIT

# Wait up to ~60s for the RPC to come up.
for _ in $(seq 1 120); do
  if cast chain-id --rpc-url "$RPC" >/dev/null 2>&1; then break; fi
  sleep 0.5
done
cast chain-id --rpc-url "$RPC" >/dev/null

echo "Minting 100k USDC + 100k USDT to ${#ACCOUNTS[@]} dev accounts..."
for acct in "${ACCOUNTS[@]}"; do
  usdc_slot=$(cast index address "$acct" "$USDC_BAL_SLOT")
  usdt_slot=$(cast index address "$acct" "$USDT_BAL_SLOT")
  cast rpc --rpc-url "$RPC" anvil_setStorageAt "$USDC" "$usdc_slot" "$AMOUNT_HEX" >/dev/null
  cast rpc --rpc-url "$RPC" anvil_setStorageAt "$USDT" "$usdt_slot" "$AMOUNT_HEX" >/dev/null
done
echo "Minting complete. Anvil is running on $RPC (Ctrl-C to stop)."

wait "$ANVIL_PID"
