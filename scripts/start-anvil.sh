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

# Deploy the IntentRouter at a deterministic address. Account #1 (nonce 0) →
# CREATE address 0x8464135c8F25Da09e49BC8782676a84730C318bC. This MUST match
# the `intent_router.contracts.router` field in
# intentOS-ui/lib/config/protocols-anvil.json and
# intent-script/config/protocols/anvil.json. If those addresses ever change,
# update both files in lockstep.
DEPLOYER_PK=0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d
DEPLOYER_ADDR=0x70997970C51812dc3A010C7d01b50e0d17dc79C8
ROUTER_ADDR=0x8464135c8F25Da09e49BC8782676a84730C318bC

# Reset the deployer's nonce so the CREATE address is deterministic regardless
# of any prior activity since fork. anvil_setNonce expects a hex string.
cast rpc --rpc-url "$RPC" anvil_setNonce "$DEPLOYER_ADDR" 0x0 >/dev/null

# Build the contracts (skipped if forge cache is up to date).
ROUTER_ARTIFACT="$REPO_ROOT/contracts/out/IntentRouter.sol/IntentRouter.json"
if [ ! -f "$ROUTER_ARTIFACT" ] || [ "$REPO_ROOT/contracts/src/IntentRouter.sol" -nt "$ROUTER_ARTIFACT" ]; then
  echo "Building IntentRouter..."
  ( cd "$REPO_ROOT/contracts" && forge build --silent )
fi

ROUTER_BYTECODE=$(jq -r '.bytecode.object' "$ROUTER_ARTIFACT")
echo "Deploying IntentRouter to $ROUTER_ADDR..."
DEPLOY_TX=$(cast send --rpc-url "$RPC" --private-key "$DEPLOYER_PK" --gas-limit 6000000 --create "$ROUTER_BYTECODE" --json | jq -r '.contractAddress')
# Case-insensitive compare. `${VAR,,}` (bash 4+) doesn't exist on the system
# bash 3.2 that ships with macOS, so use `tr` — portable across every shell
# anyone is realistically running.
DEPLOY_TX_LC=$(printf '%s' "$DEPLOY_TX" | tr '[:upper:]' '[:lower:]')
ROUTER_ADDR_LC=$(printf '%s' "$ROUTER_ADDR" | tr '[:upper:]' '[:lower:]')
if [ "$DEPLOY_TX_LC" != "$ROUTER_ADDR_LC" ]; then
  echo "ERROR: router deployed to $DEPLOY_TX, expected $ROUTER_ADDR." >&2
  echo "       Likely cause: deployer nonce wasn't 0 before deploy." >&2
  exit 1
fi

# Seed the allowlist with every protocol/token the compiler can produce calls
# to. Without this, _executeCalls reverts with "Target not allowed" and the
# whole batched intent fails. Mirror of DeployIntentRouter.s.sol.
echo "Seeding router allowlist (14 targets)..."
TARGETS='[0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2,0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48,0xdAC17F958D2ee523a2206206994597C13D831ec7,0x6B175474E89094C44Da98b954EedeAC495271d0F,0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599,0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84,0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0,0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2,0xE592427A0AEce92De3Edee1F18E0157C05861564,0xC36442b4a4522E871399CD717aBDD847Ab11FE88,0x889edC2eDab5f40e902b864aD4d7AdE8E412F9B1,0xBA12222222228d8Ba445958a75a0704d566BF2C8,0xBBBBBbbBBb9cC5e90e3b3Af64bdAF62C37EEFFCb,0x5c7BCd6E7De5423a257D81B442095A1a6ced35C5]'
cast send --rpc-url "$RPC" --private-key "$DEPLOYER_PK" --gas-limit 1000000 \
  "$ROUTER_ADDR" 'setAllowedTargets(address[],bool)' "$TARGETS" true >/dev/null
echo "Router ready at $ROUTER_ADDR (owner: $DEPLOYER_ADDR)."

echo "Minting complete. Anvil is running on $RPC (Ctrl-C to stop)."

wait "$ANVIL_PID"
