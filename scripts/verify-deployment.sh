#!/usr/bin/env bash
#
# Re-checks an existing deployment recorded in deployments/<network>.json.
# Runs the same four checks as scripts/deploy.sh, using read-only contract
# calls, so anyone can verify the wiring later. Exits non-zero on any mismatch.
#
# Requirements:
#   - the `stellar` CLI (https://developer.stellar.org/docs/tools/cli)
#   - `jq` to read the ids from the deployment record (https://jqlang.org)
#   - an identity configured for the network, used only to simulate the calls
#
# Usage:
#   scripts/verify-deployment.sh [network] [source-identity]
#
#   scripts/verify-deployment.sh testnet deployer
#   scripts/verify-deployment.sh -h | --help

set -euo pipefail

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

command -v stellar >/dev/null 2>&1 \
  || die "the 'stellar' CLI was not found on PATH. Install it with: cargo install stellar-cli"
command -v jq >/dev/null 2>&1 \
  || die "jq was not found on PATH. Install it from https://jqlang.org"

RECORD="deployments/$NETWORK.json"
[ -f "$RECORD" ] || die "no deployment record at $RECORD. Deploy first with scripts/deploy.sh"

ADMIN="$(jq -r '.admin' "$RECORD")"
VAULT_ID="$(jq -r '.contracts.vault.id' "$RECORD")"
STREAM_ID="$(jq -r '.contracts.stream.id' "$RECORD")"
REGISTRY_ID="$(jq -r '.contracts.registry.id' "$RECORD")"

# Read-only: without --send, `stellar contract invoke` only simulates the call.
invoke() {
  local id="$1"
  shift
  stellar contract invoke --id "$id" --source "$SOURCE" --network "$NETWORK" -- "$@"
}

# `stellar contract invoke` prints string results as JSON; strip the quotes.
unquote() {
  tr -d '"[:space:]'
}

FAILURES=0

check() {
  local what="$1" actual="$2" expected="$3"
  if [ "$actual" = "$expected" ]; then
    echo "ok: $what"
  else
    echo "MISMATCH: $what is '$actual', expected '$expected'" >&2
    FAILURES=$((FAILURES + 1))
  fi
}

echo "== Verifying the '$NETWORK' deployment in $RECORD (as '$SOURCE') =="

check "vault admin" "$(invoke "$VAULT_ID" get_admin | unquote)" "$ADMIN"
check "vault trusts the stream contract" "$(invoke "$VAULT_ID" get_stream_contract | unquote)" "$STREAM_ID"
check "registry admin" "$(invoke "$REGISTRY_ID" get_admin | unquote)" "$ADMIN"
check "stream contract is unpaused" "$(invoke "$STREAM_ID" is_paused | unquote)" "false"

if [ "$FAILURES" -gt 0 ]; then
  die "$FAILURES check(s) failed"
fi

echo "== Deployment verified =="
