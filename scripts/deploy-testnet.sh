#!/usr/bin/env bash
# Backwards-compatible testnet wrapper: deploy the contracts, then seed the
# PUSD demo. New deployments should call deploy.sh and seed-demo.sh directly so
# the two steps can be run against any network.
#
#   ./scripts/deploy-testnet.sh
#
# Requires: stellar CLI >= 28. Creates (or reuses) and funds the admin identity
# on testnet, then leaves the demo identities to seed-demo.sh.
set -euo pipefail

cd "$(dirname "$0")/.."

NETWORK=testnet
ADMIN_IDENTITY=${ADMIN_IDENTITY:-plimsoll-admin}

if ! stellar keys address "$ADMIN_IDENTITY" >/dev/null 2>&1; then
  stellar keys generate "$ADMIN_IDENTITY" --network "$NETWORK" --fund >/dev/null
else
  stellar keys fund "$ADMIN_IDENTITY" --network "$NETWORK" >/dev/null 2>&1 || true
fi

./scripts/deploy.sh "$NETWORK"
./scripts/seed-demo.sh "$NETWORK"
