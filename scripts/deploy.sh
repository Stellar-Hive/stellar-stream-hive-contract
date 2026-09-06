#!/usr/bin/env bash
#
# Deploys the Stellar Stream Hive contracts (vault, stream, registry) to a
# Soroban network, in dependency order, and wires them together.
#
# Requirements:
#   - the `stellar` CLI (https://developer.stellar.org/docs/tools/cli)
#   - a funded identity configured for the target network
#     (e.g. `stellar keys generate deployer --network testnet --fund`)
#
# Usage:
#   scripts/deploy.sh [network] [source-identity]
#
#   scripts/deploy.sh testnet deployer
#   scripts/deploy.sh mainnet deployer
#
# The script is idempotent-ish: re-running it deploys fresh contract
# instances (Soroban has no "redeploy in place" for contract *code plus
# storage* — each deploy is a new contract id). It does not modify
# DEPLOYMENTS.md automatically; copy the printed contract ids there.

set -euo pipefail

NETWORK="${1:-testnet}"
SOURCE="${2:-deployer}"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "== Stellar Stream Hive: deploying to '$NETWORK' as '$SOURCE' =="

if ! command -v stellar >/dev/null 2>&1; then
  echo "error: the 'stellar' CLI was not found on PATH." >&2
  echo "Install it with: cargo install stellar-cli" >&2
  exit 1
fi

echo
echo "-- Building all contracts to wasm32-unknown-unknown (release) --"
cargo build --target wasm32-unknown-unknown --release \
  -p stellar-stream-hive-vault \
  -p stellar-stream-hive-stream \
  -p stellar-stream-hive-registry

WASM_DIR="target/wasm32-unknown-unknown/release"
VAULT_WASM="$WASM_DIR/stellar_stream_hive_vault.wasm"
STREAM_WASM="$WASM_DIR/stellar_stream_hive_stream.wasm"
REGISTRY_WASM="$WASM_DIR/stellar_stream_hive_registry.wasm"

for f in "$VAULT_WASM" "$STREAM_WASM" "$REGISTRY_WASM"; do
  if [ ! -f "$f" ]; then
    echo "error: expected build artifact not found: $f" >&2
    exit 1
  fi
done

echo
echo "-- Optimizing wasm binaries --"
stellar contract optimize --wasm "$VAULT_WASM"
stellar contract optimize --wasm "$STREAM_WASM"
stellar contract optimize --wasm "$REGISTRY_WASM"

VAULT_WASM_OPT="${VAULT_WASM%.wasm}.optimized.wasm"
STREAM_WASM_OPT="${STREAM_WASM%.wasm}.optimized.wasm"
REGISTRY_WASM_OPT="${REGISTRY_WASM%.wasm}.optimized.wasm"

echo
echo "-- Deploying vault contract --"
VAULT_ID=$(stellar contract deploy \
  --wasm "$VAULT_WASM_OPT" \
  --source "$SOURCE" \
  --network "$NETWORK")
echo "vault contract id: $VAULT_ID"

echo
echo "-- Deploying stream contract --"
STREAM_ID=$(stellar contract deploy \
  --wasm "$STREAM_WASM_OPT" \
  --source "$SOURCE" \
  --network "$NETWORK")
echo "stream contract id: $STREAM_ID"

echo
echo "-- Deploying registry contract --"
REGISTRY_ID=$(stellar contract deploy \
  --wasm "$REGISTRY_WASM_OPT" \
  --source "$SOURCE" \
  --network "$NETWORK")
echo "registry contract id: $REGISTRY_ID"

ADMIN_ADDRESS=$(stellar keys address "$SOURCE")

echo
echo "-- Initializing vault (admin=$ADMIN_ADDRESS, stream_contract=$STREAM_ID) --"
stellar contract invoke \
  --id "$VAULT_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  -- initialize \
  --admin "$ADMIN_ADDRESS" \
  --stream_contract "$STREAM_ID"

echo
echo "-- Initializing stream contract (admin=$ADMIN_ADDRESS, vault=$VAULT_ID) --"
stellar contract invoke \
  --id "$STREAM_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  -- initialize \
  --admin "$ADMIN_ADDRESS" \
  --vault "$VAULT_ID"

echo
echo "-- Initializing registry contract (admin=$ADMIN_ADDRESS) --"
stellar contract invoke \
  --id "$REGISTRY_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  -- initialize \
  --admin "$ADMIN_ADDRESS"

echo
echo "== Deployment complete =="
echo "vault:    $VAULT_ID"
echo "stream:   $STREAM_ID"
echo "registry: $REGISTRY_ID"
echo
echo "Record these ids in DEPLOYMENTS.md."
