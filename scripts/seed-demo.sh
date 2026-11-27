#!/usr/bin/env bash
# Seed the PUSD demo on a deployment made by deploy.sh.
#
#   ./scripts/seed-demo.sh [network]     # default: testnet
#
# This creates the demo identities, registers the demo reporters, issues PUSD
# and lists its Stellar Asset Contract. On testnet it also lists Circle's USDC.
# It deploys a reference covered-vault for PUSD if the deployment has none.
#
# Demo data is deliberately test-only: this refuses to run on mainnet, by name
# or by network passphrase. Requires deployments/<network>.json from deploy.sh
# and the wasm from `stellar contract build`.
set -euo pipefail

usage() {
  echo "usage: $0 [network]" >&2
  exit 2
}

[ $# -le 1 ] || usage
NETWORK=${1:-testnet}
ADMIN_IDENTITY=${ADMIN_IDENTITY:-plimsoll-admin}
WASM_DIR=${WASM_DIR:-target/wasm32v1-none/release}
DEMO_CODE=PUSD
DEMO_SUPPLY=10000000000000 # 1,000,000 PUSD in stroops
# Circle's testnet USDC issuer.
TESTNET_USDC_ISSUER=GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5
MAINNET_PASSPHRASE="Public Global Stellar Network ; September 2015"

cd "$(dirname "$0")/.."
DEP=deployments/$NETWORK.json

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

PASSPHRASE=${STELLAR_NETWORK_PASSPHRASE:-$(network_detail "Network passphrase")}
if [ "$NETWORK" = "mainnet" ] || [ "$PASSPHRASE" = "$MAINNET_PASSPHRASE" ]; then
  echo "refusing to seed demo identities and assets on mainnet" >&2
  exit 1
fi
if [ -z "$PASSPHRASE" ]; then
  echo "error: network '$NETWORK' is not configured in the Stellar CLI." >&2
  exit 1
fi
if [ ! -f "$DEP" ]; then
  echo "error: $DEP is missing. Run ./scripts/deploy.sh $NETWORK first." >&2
  exit 1
fi

PY=
for candidate in python3 python; do
  if "$candidate" -c "" >/dev/null 2>&1; then
    PY=$candidate
    break
  fi
done
[ -n "$PY" ] || {
  echo "error: no working python interpreter on PATH" >&2
  exit 1
}

json_get() {
  "$PY" -c '
import json, sys
value = json.load(open(sys.argv[1]))
for part in sys.argv[2].split("."):
    value = value[part]
print(value)
' "$DEP" "$1"
}

json_optional() {
  "$PY" -c '
import json, sys
value = json.load(open(sys.argv[1]))
try:
    for part in sys.argv[2].split("."):
        value = value[part]
except (KeyError, TypeError):
    value = ""
print(value)
' "$DEP" "$1"
}

key() {
  local name=$1
  if ! stellar keys address "$name" >/dev/null 2>&1; then
    stellar keys generate "$name" --network "$NETWORK" --fund >/dev/null
  else
    stellar keys fund "$name" --network "$NETWORK" >/dev/null 2>&1 || true
  fi
  stellar keys address "$name" | tr -d '\r'
}

read_call() {
  stellar contract invoke --network "$NETWORK" --source "$ADMIN_IDENTITY" \
    --send=no --id "$1" -- "${@:2}" 2>/dev/null | tail -1 | tr -d '\r'
}

ADMIN=$(stellar keys address "$ADMIN_IDENTITY" 2>/dev/null | tr -d '\r')
if [ -z "$ADMIN" ]; then
  echo "error: identity '$ADMIN_IDENTITY' not found. Run deploy.sh first." >&2
  exit 1
fi
DEPLOYED_ADMIN=$(json_get "accounts.admin")
if [ "$ADMIN" != "$DEPLOYED_ADMIN" ]; then
  echo "error: $DEP was deployed by $DEPLOYED_ADMIN, not $ADMIN." >&2
  echo "Set ADMIN_IDENTITY to the identity that deployed it." >&2
  exit 1
fi

LEDGER=$(json_get "contracts.coverageLedger")
REGISTRY=$(json_get "contracts.reporterRegistry")

echo "==> identities"
POSTER=$(key plimsoll-poster)
AUDITOR=$(key plimsoll-auditor)
ISSUER=$(key plimsoll-issuer)
HOLDER=$(key plimsoll-holder)
echo "poster  $POSTER"
echo "auditor $AUDITOR"
echo "issuer  $ISSUER"
echo "holder  $HOLDER"

echo "==> register reporters"
stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- \
  set_reporter --reporter "$POSTER" --role 3 --name "Plimsoll indexer" >/dev/null
stellar contract invoke --id "$REGISTRY" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- \
  set_reporter --reporter "$AUDITOR" --role 1 --name "Demo Auditor ($NETWORK)" >/dev/null

echo "==> issue demo asset $DEMO_CODE"
PUSD_SAC=$(stellar contract asset deploy --asset "$DEMO_CODE:$ISSUER" \
  --source "$ADMIN_IDENTITY" --network "$NETWORK" 2>/dev/null \
  || stellar contract id asset --asset "$DEMO_CODE:$ISSUER" --network "$NETWORK")
PUSD_SAC=$(printf '%s\n' "$PUSD_SAC" | tail -1 | tr -d '\r')
BALANCE=$(read_call "$PUSD_SAC" balance --id "$HOLDER")
if [ "$BALANCE" = "0" ]; then
  stellar tx new change-trust --source plimsoll-holder --network "$NETWORK" \
    --line "$DEMO_CODE:$ISSUER" >/dev/null 2>&1 || true
  stellar tx new payment --source plimsoll-issuer --network "$NETWORK" \
    --destination "$HOLDER" --asset "$DEMO_CODE:$ISSUER" --amount "$DEMO_SUPPLY" >/dev/null
else
  echo "($HOLDER already holds $DEMO_CODE)"
fi
echo "PUSD SAC $PUSD_SAC"

echo "==> list assets"
if [ "$(read_call "$LEDGER" get_asset --sac "$PUSD_SAC")" = "null" ]; then
  stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- \
    list_asset --sac "$PUSD_SAC" >/dev/null
else
  echo "(PUSD already listed)"
fi

USDC_SAC=""
USDC_ISSUER=""
if [ "$NETWORK" = "testnet" ]; then
  echo "==> testnet USDC SAC"
  USDC_ISSUER=$TESTNET_USDC_ISSUER
  USDC_SAC=$(stellar contract id asset --asset "USDC:$USDC_ISSUER" --network "$NETWORK" | tail -1 | tr -d '\r')
  stellar contract asset deploy --asset "USDC:$USDC_ISSUER" \
    --source "$ADMIN_IDENTITY" --network "$NETWORK" >/dev/null 2>&1 || true
  if [ "$(read_call "$LEDGER" get_asset --sac "$USDC_SAC")" = "null" ]; then
    stellar contract invoke --id "$LEDGER" --source "$ADMIN_IDENTITY" --network "$NETWORK" -- \
      list_asset --sac "$USDC_SAC" >/dev/null
  else
    echo "(USDC already listed)"
  fi
  echo "USDC SAC $USDC_SAC"
fi

echo "==> deploy covered-vault (PUSD, 100%, 7 days, issuer-signed or better)"
VAULT=$(json_optional "contracts.coveredVault")
if [ -z "$VAULT" ]; then
  [ -f "$WASM_DIR/covered_vault.wasm" ] || {
    echo "error: $WASM_DIR/covered_vault.wasm was not built. Run stellar contract build." >&2
    exit 1
  }
  VAULT=$(stellar contract deploy --wasm "$WASM_DIR/covered_vault.wasm" \
    --source "$ADMIN_IDENTITY" --network "$NETWORK" -- \
    --admin "$ADMIN" --ledger "$LEDGER" --asset "$PUSD_SAC" --min_bps 10000 --max_age 604800 --min_tier 2)
  VAULT=$(printf '%s\n' "$VAULT" | tail -1 | tr -d '\r')
else
  echo "(reusing $VAULT from $DEP)"
fi
echo "vault $VAULT"

"$PY" - "$DEP" "$PUSD_SAC" "$ISSUER" "$HOLDER" "$POSTER" "$AUDITOR" \
  "$VAULT" "$USDC_ISSUER" "$USDC_SAC" <<'PY'
import json, sys

path, pusd_sac, issuer, holder, poster, auditor, vault, usdc_issuer, usdc_sac = sys.argv[1:]
with open(path, encoding="utf-8") as f:
    d = json.load(f)

d.setdefault("contracts", {})["coveredVault"] = vault
d.setdefault("assets", {})["PUSD"] = {"code": "PUSD", "issuer": issuer, "sac": pusd_sac}
if usdc_sac:
    d["assets"]["USDC"] = {"code": "USDC", "issuer": usdc_issuer, "sac": usdc_sac}
d.setdefault("accounts", {}).update(
    {
        "supplyPoster": poster,
        "demoAuditor": auditor,
        "demoIssuer": issuer,
        "demoHolder": holder,
    }
)

with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump(d, f, indent=2)
    f.write("\n")
PY

cat <<EOF

================ Plimsoll $NETWORK demo ================
PUSD_SAC=$PUSD_SAC
COVERED_VAULT_ID=$VAULT
Written to $DEP
=======================================================
EOF
