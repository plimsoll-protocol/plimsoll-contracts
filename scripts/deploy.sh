#!/usr/bin/env bash
# Deploy the Plimsoll contracts to a Stellar network in dependency order.
#
#   ./scripts/deploy.sh <network>
#
# The network must be one the Stellar CLI knows about (`stellar network ls`):
# testnet, futurenet and local are built in, and a custom network can be added
# with `stellar network add`. Set STELLAR_RPC_URL and/or
# STELLAR_NETWORK_PASSPHRASE to override the configured endpoint.
#
# This script only deploys. It creates no identities, funds nothing and seeds no
# demo data; see seed-demo.sh for the PUSD demo. It needs a funded identity,
# named by ADMIN_IDENTITY (default plimsoll-admin), and builds the wasm first.
#
# If ASSET_SAC is set, the reference covered-vault is deployed for that asset
# with MIN_BPS / MAX_AGE / MIN_TIER (defaults 10000, 604800 and 2).
set -euo pipefail

usage() {
  echo "usage: $0 <network>" >&2
  exit 2
}

[ $# -eq 1 ] || usage
NETWORK=$1
ADMIN_IDENTITY=${ADMIN_IDENTITY:-plimsoll-admin}
WASM_DIR=${WASM_DIR:-target/wasm32v1-none/release}
OUT=deployments/$NETWORK.json
ASSET_SAC=${ASSET_SAC:-}
MIN_BPS=${MIN_BPS:-10000}
MAX_AGE=${MAX_AGE:-604800}
MIN_TIER=${MIN_TIER:-2}

cd "$(dirname "$0")/.."

command -v stellar >/dev/null 2>&1 || {
  echo "error: the Stellar CLI is not on PATH. Install stellar-cli 28 or later." >&2
  exit 1
}

# Read a field from `stellar network ls --long` for the named network.
network_detail() {
  local label=$1
  stellar network ls --long | awk -v want="$NETWORK" -v label="$label" '
    /^Name: / { name = substr($0, 7) }
    index($0, label ": ") == 1 && name == want {
      print substr($0, length(label) + 3)
      exit
    }
  '
}

if ! stellar keys address "$ADMIN_IDENTITY" >/dev/null 2>&1; then
  echo "error: identity '$ADMIN_IDENTITY' not found in the Stellar CLI keystore." >&2
  echo "Create and fund it first, for example:" >&2
  echo "  stellar keys generate '$ADMIN_IDENTITY' --network '$NETWORK' --fund" >&2
  exit 1
fi
ADMIN=$(stellar keys address "$ADMIN_IDENTITY" | tr -d '\r')

CONFIGURED_PASSPHRASE=$(network_detail "Network passphrase")
RPC_URL=${STELLAR_RPC_URL:-$(network_detail "RPC url")}
NETWORK_PASSPHRASE=${STELLAR_NETWORK_PASSPHRASE:-$CONFIGURED_PASSPHRASE}

if [ -z "$NETWORK_PASSPHRASE" ]; then
  echo "error: network '$NETWORK' is not configured in the Stellar CLI." >&2
  echo "Add it with 'stellar network add', or set STELLAR_NETWORK_PASSPHRASE." >&2
  exit 1
fi
if [ -z "$RPC_URL" ] || [[ "$RPC_URL" == Bring\ Your\ Own* ]]; then
  echo "error: network '$NETWORK' has no usable RPC URL." >&2
  echo "Set STELLAR_RPC_URL to the network's Soroban RPC endpoint." >&2
  exit 1
fi

NETWORK_ARGS=(--network "$NETWORK")
if [ -n "${STELLAR_RPC_URL:-}" ]; then
  NETWORK_ARGS+=(--rpc-url "$STELLAR_RPC_URL")
fi
if [ -n "${STELLAR_NETWORK_PASSPHRASE:-}" ]; then
  NETWORK_ARGS+=(--network-passphrase "$STELLAR_NETWORK_PASSPHRASE")
fi

if ! HEALTH=$(stellar network health "${NETWORK_ARGS[@]}" --output json 2>/dev/null); then
  echo "error: cannot reach $NETWORK at $RPC_URL" >&2
  exit 1
fi
LATEST=$(printf '%s\n' "$HEALTH" | sed -nE 's/.*"latestLedger":([0-9]+).*/\1/p')
if [ -z "$LATEST" ]; then
  echo "error: $NETWORK did not report a latest ledger" >&2
  exit 1
fi

echo "==> build"
stellar contract build >/dev/null
for wasm in reporter_registry coverage_ledger covered_vault; do
  [ -f "$WASM_DIR/$wasm.wasm" ] || {
    echo "error: $WASM_DIR/$wasm.wasm was not built" >&2
    exit 1
  }
done

echo "==> deploy reporter-registry"
REGISTRY=$(stellar contract deploy --wasm "$WASM_DIR/reporter_registry.wasm" \
  --source "$ADMIN_IDENTITY" "${NETWORK_ARGS[@]}" -- --admin "$ADMIN")
echo "registry $REGISTRY"

echo "==> deploy coverage-ledger"
LEDGER=$(stellar contract deploy --wasm "$WASM_DIR/coverage_ledger.wasm" \
  --source "$ADMIN_IDENTITY" "${NETWORK_ARGS[@]}" -- --admin "$ADMIN" --registry "$REGISTRY")
echo "ledger   $LEDGER"

VAULT=""
if [ -n "$ASSET_SAC" ]; then
  echo "==> deploy covered-vault ($ASSET_SAC, $MIN_BPS bps, $MAX_AGE s, tier $MIN_TIER)"
  VAULT=$(stellar contract deploy --wasm "$WASM_DIR/covered_vault.wasm" \
    --source "$ADMIN_IDENTITY" "${NETWORK_ARGS[@]}" -- \
    --admin "$ADMIN" --ledger "$LEDGER" --asset "$ASSET_SAC" --min_bps "$MIN_BPS" \
    --max_age "$MAX_AGE" --min_tier "$MIN_TIER")
  echo "vault    $VAULT"
fi

CONTRACTS_JSON="    \"reporterRegistry\": \"$REGISTRY\",
    \"coverageLedger\": \"$LEDGER\""
if [ -n "$VAULT" ]; then
  CONTRACTS_JSON="$CONTRACTS_JSON,
    \"coveredVault\": \"$VAULT\""
fi

mkdir -p deployments
{
  printf '{\n'
  printf '  "network": "%s",\n' "$NETWORK"
  printf '  "rpcUrl": "%s",\n' "$RPC_URL"
  printf '  "networkPassphrase": "%s",\n' "$NETWORK_PASSPHRASE"
  printf '  "deployedAtLedger": %s,\n' "$LATEST"
  printf '  "contracts": {\n%s\n  },\n' "$CONTRACTS_JSON"
  printf '  "accounts": {\n'
  printf '    "admin": "%s"\n' "$ADMIN"
  printf '  }\n'
  printf '}\n'
} >"$OUT"

cat <<EOF

================ Plimsoll $NETWORK deployment ================
REPORTER_REGISTRY_ID=$REGISTRY
COVERAGE_LEDGER_ID=$LEDGER
EOF
if [ -n "$VAULT" ]; then
  echo "COVERED_VAULT_ID=$VAULT"
fi
cat <<EOF
DEPLOYED_AT_LEDGER=$LATEST
=============================================================
Written to $OUT

Next: seed the demo with ./scripts/seed-demo.sh $NETWORK
EOF
