#!/usr/bin/env bash
#
# Deploys the Stellar Stream Hive contracts (vault, stream, registry) to a
# Soroban network, in dependency order, wires them together, verifies the
# wiring, and records the result.
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
#   CONFIRM_MAINNET=yes scripts/deploy.sh mainnet deployer
#
# Outputs:
#   deployments/<network>.json   contract ids, wasm hashes, admin, timestamp
#   stdout                       a DEPLOYMENTS.md table and the env lines the
#                                frontend and API need
#
# Re-running deploys fresh contract instances (Soroban has no "redeploy in
# place" for code plus storage); each run gets a new set of contract ids and
# overwrites deployments/<network>.json.

set -euo pipefail

# Prints the usage block from the header comment above.
usage() {
  awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
}

case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
esac

NETWORK="${1:-testnet}"
SOURCE="${2:-deployer}"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

die() {
  echo "error: $*" >&2
  exit 1
}

case "$NETWORK" in
  testnet | futurenet | local | mainnet) ;;
  *) die "unknown network '$NETWORK' (expected testnet, futurenet, local or mainnet)" ;;
esac

# Mainnet moves real funds' worth of trust; make it impossible to hit by accident.
if [ "$NETWORK" = "mainnet" ] && [ "${CONFIRM_MAINNET:-}" != "yes" ]; then
  die "refusing to deploy to mainnet without CONFIRM_MAINNET=yes. The contracts are unaudited; see SECURITY.md."
fi

command -v stellar >/dev/null 2>&1 \
  || die "the 'stellar' CLI was not found on PATH. Install it with: cargo install stellar-cli"

ADMIN_ADDRESS="$(stellar keys address "$SOURCE" 2>/dev/null)" \
  || die "identity '$SOURCE' does not exist. Create it with: stellar keys generate $SOURCE --network $NETWORK --fund"

echo "== Stellar Stream Hive: deploying to '$NETWORK' as '$SOURCE' ($ADMIN_ADDRESS) =="

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# `stellar contract invoke` prints return values as JSON; strip the quotes
# from a string result so it can be compared to a plain address.
unquote() {
  tr -d '"[:space:]'
}

invoke() {
  local id="$1"
  shift
  stellar contract invoke --id "$id" --source "$SOURCE" --network "$NETWORK" -- "$@"
}

expect_eq() {
  local what="$1" actual="$2" expected="$3"
  if [ "$actual" != "$expected" ]; then
    die "verification failed: $what is '$actual', expected '$expected'"
  fi
  echo "ok: $what"
}

if command -v rustup >/dev/null 2>&1 \
  && ! rustup target list --installed | grep -qx "wasm32v1-none"; then
  die "the wasm32v1-none target is not installed. Install it with: rustup target add wasm32v1-none"
fi

echo
echo "-- Building all contracts to wasm32v1-none (release) --"
cargo build --target wasm32v1-none --release \
  -p stellar-stream-hive-vault \
  -p stellar-stream-hive-stream \
  -p stellar-stream-hive-registry

WASM_DIR="target/wasm32v1-none/release"
VAULT_WASM="$WASM_DIR/stellar_stream_hive_vault.wasm"
STREAM_WASM="$WASM_DIR/stellar_stream_hive_stream.wasm"
REGISTRY_WASM="$WASM_DIR/stellar_stream_hive_registry.wasm"

for f in "$VAULT_WASM" "$STREAM_WASM" "$REGISTRY_WASM"; do
  [ -f "$f" ] || die "expected build artifact not found: $f"
done

echo
echo "-- Optimizing wasm binaries --"
stellar contract optimize --wasm "$VAULT_WASM"
stellar contract optimize --wasm "$STREAM_WASM"
stellar contract optimize --wasm "$REGISTRY_WASM"

VAULT_WASM_OPT="${VAULT_WASM%.wasm}.optimized.wasm"
STREAM_WASM_OPT="${STREAM_WASM%.wasm}.optimized.wasm"
REGISTRY_WASM_OPT="${REGISTRY_WASM%.wasm}.optimized.wasm"

VAULT_HASH="$(sha256_of "$VAULT_WASM_OPT")"
STREAM_HASH="$(sha256_of "$STREAM_WASM_OPT")"
REGISTRY_HASH="$(sha256_of "$REGISTRY_WASM_OPT")"

deploy() {
  stellar contract deploy --wasm "$1" --source "$SOURCE" --network "$NETWORK"
}

echo
echo "-- Deploying vault contract --"
VAULT_ID="$(deploy "$VAULT_WASM_OPT")"
echo "vault contract id: $VAULT_ID"

echo
echo "-- Deploying stream contract --"
STREAM_ID="$(deploy "$STREAM_WASM_OPT")"
echo "stream contract id: $STREAM_ID"

echo
echo "-- Deploying registry contract --"
REGISTRY_ID="$(deploy "$REGISTRY_WASM_OPT")"
echo "registry contract id: $REGISTRY_ID"

echo
echo "-- Initializing vault (admin=$ADMIN_ADDRESS, stream_contract=$STREAM_ID) --"
invoke "$VAULT_ID" initialize --admin "$ADMIN_ADDRESS" --stream_contract "$STREAM_ID"

echo
echo "-- Initializing stream contract (admin=$ADMIN_ADDRESS, vault=$VAULT_ID) --"
invoke "$STREAM_ID" initialize --admin "$ADMIN_ADDRESS" --vault "$VAULT_ID"

echo
echo "-- Initializing registry contract (admin=$ADMIN_ADDRESS) --"
invoke "$REGISTRY_ID" initialize --admin "$ADMIN_ADDRESS"

echo
echo "-- Verifying the deployment --"
expect_eq "vault admin" "$(invoke "$VAULT_ID" get_admin | unquote)" "$ADMIN_ADDRESS"
expect_eq "vault trusts the stream contract" "$(invoke "$VAULT_ID" get_stream_contract | unquote)" "$STREAM_ID"
expect_eq "registry admin" "$(invoke "$REGISTRY_ID" get_admin | unquote)" "$ADMIN_ADDRESS"
expect_eq "stream contract starts unpaused" "$(invoke "$STREAM_ID" is_paused | unquote)" "false"

DEPLOYED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
mkdir -p deployments

# Keep the previous ids: funds may still sit in the old contracts.
if [ -f "deployments/$NETWORK.json" ]; then
  BACKUP="deployments/$NETWORK.$(date -u +%Y%m%dT%H%M%SZ).json.bak"
  cp "deployments/$NETWORK.json" "$BACKUP"
  echo "kept the previous deployment record as $BACKUP"
fi

cat > "deployments/$NETWORK.json" <<EOF
{
  "network": "$NETWORK",
  "deployed_at": "$DEPLOYED_AT",
  "admin": "$ADMIN_ADDRESS",
  "contracts": {
    "vault":    { "id": "$VAULT_ID",    "wasm_sha256": "$VAULT_HASH" },
    "stream":   { "id": "$STREAM_ID",   "wasm_sha256": "$STREAM_HASH" },
    "registry": { "id": "$REGISTRY_ID", "wasm_sha256": "$REGISTRY_HASH" }
  }
}
EOF

echo
echo "== Deployment complete: wrote deployments/$NETWORK.json =="
echo
echo "DEPLOYMENTS.md ($NETWORK):"
echo
echo "| Contract | Contract ID | WASM sha256 | Deployed at |"
echo "|---|---|---|---|"
echo "| Vault | $VAULT_ID | $VAULT_HASH | $DEPLOYED_AT |"
echo "| Stream | $STREAM_ID | $STREAM_HASH | $DEPLOYED_AT |"
echo "| Registry | $REGISTRY_ID | $REGISTRY_HASH | $DEPLOYED_AT |"
echo
echo "Frontend (.env.local):"
echo "  NEXT_PUBLIC_STREAM_CONTRACT_ID=$STREAM_ID"
echo "  NEXT_PUBLIC_VAULT_CONTRACT_ID=$VAULT_ID"
echo "  NEXT_PUBLIC_REGISTRY_CONTRACT_ID=$REGISTRY_ID"
echo
echo "API (.env):"
echo "  STREAM_CONTRACT_ID=$STREAM_ID"
