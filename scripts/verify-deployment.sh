#!/usr/bin/env bash
# Check that the contracts recorded in deployments/<network>.json still run the
# code in this repository.
#
#   ./scripts/verify-deployment.sh <network>
#
# For every contract id in the deployments file this reads the wasm hash
# installed on chain, builds the wasm locally, and compares the two. It prints
# one line per contract and exits non-zero if any of them differ, naming the
# contract and showing both hashes.
#
# Needs no keys: the on-chain hash is read through RPC, which is anonymous. The
# RPC endpoint and passphrase come from the deployments file, so the network
# does not have to be configured in the Stellar CLI. STELLAR_RPC_URL overrides
# the endpoint.
#
# The comparison is only meaningful if the local build is reproducible. The
# wasm is byte-for-byte the same on Linux and macOS for the same inputs, but
# those inputs include the toolchain: rustc (pinned in rust-toolchain.toml, which
# rustup selects here), the locked dependencies (hence --locked), and the
# Stellar CLI, which writes its own version into the wasm's metadata. A
# different CLI version alone changes the hash, so this warns when the local CLI
# is not the one CI pins, and on a mismatch prints the toolchain each side was
# built with. Set SKIP_BUILD=1 to compare wasm already in WASM_DIR (for example
# in CI, straight after its own build step).
#
# Exit status: 0 all match, 1 at least one mismatch, 2 usage or setup error,
# 3 an on-chain hash could not be read.
set -euo pipefail

usage() {
  echo "usage: $0 <network>" >&2
  exit 2
}

[ $# -eq 1 ] || usage
NETWORK=$1
WASM_DIR=${WASM_DIR:-target/wasm32v1-none/release}
SKIP_BUILD=${SKIP_BUILD:-0}

cd "$(dirname "$0")/.."
DEP=deployments/$NETWORK.json

command -v stellar > /dev/null 2>&1 || {
  echo "error: the Stellar CLI is not on PATH. Install stellar-cli 28 or later." >&2
  exit 2
}
[ -f "$DEP" ] || {
  echo "error: $DEP does not exist. Known deployments:" >&2
  ls deployments >&2
  exit 2
}

# Linux CI has python3 and no python; Windows has a python3 that is a Microsoft
# Store stub resolving on PATH and exiting 49. Run the probe, not `command -v`.
PY=""
for candidate in python3 python; do
  if "$candidate" -c "" > /dev/null 2>&1; then
    PY=$candidate
    break
  fi
done
[ -n "$PY" ] || {
  echo "error: no working python interpreter on PATH" >&2
  exit 2
}

# Prints "<key> <contract id> <wasm name>" per contract, then the RPC URL and
# passphrase on lines of their own prefixed with "rpc " and "passphrase ".
# Every contract key must map to a wasm built in this repository; a key that
# does not is an error rather than something silently skipped.
PLAN=$("$PY" - "$DEP" << 'EOF'
import json, sys

dep = json.load(open(sys.argv[1]))
wasm_for = {
    "reporterRegistry": "reporter_registry",
    "coverageLedger": "coverage_ledger",
}
unknown = []
for key, cid in dep.get("contracts", {}).items():
    # The demo has one covered-vault per asset (coveredVault,
    # coveredVaultQusd, ...), all deployed from the same wasm.
    wasm = "covered_vault" if key.startswith("coveredVault") else wasm_for.get(key)
    if wasm is None:
        unknown.append(key)
        continue
    print(key, cid, wasm)
if unknown:
    print("error: no wasm known for contract key(s): " + ", ".join(unknown), file=sys.stderr)
    sys.exit(2)
print("rpc", dep.get("rpcUrl", ""))
print("passphrase", dep.get("networkPassphrase", ""))
EOF
)

RPC_URL=${STELLAR_RPC_URL:-$(printf '%s\n' "$PLAN" | sed -n 's/^rpc //p')}
PASSPHRASE=$(printf '%s\n' "$PLAN" | sed -n 's/^passphrase //p')
CONTRACTS=$(printf '%s\n' "$PLAN" | grep -vE '^(rpc|passphrase) ' || true)

[ -n "$RPC_URL" ] || {
  echo "error: $DEP has no rpcUrl. Set STELLAR_RPC_URL." >&2
  exit 2
}
[ -n "$PASSPHRASE" ] || {
  echo "error: $DEP has no networkPassphrase." >&2
  exit 2
}
[ -n "$CONTRACTS" ] || {
  echo "error: $DEP lists no contracts." >&2
  exit 2
}

# The CLI version CI builds with, from the action that installs it.
PINNED_CLI=$(sed -nE 's/^ *STELLAR_CLI_VERSION: *"([^"]+)".*/\1/p' \
  .github/actions/install-stellar-cli/action.yml 2> /dev/null || true)
LOCAL_CLI=$(stellar --version | sed -nE '1s/^stellar ([^ ]+).*/\1/p')
if [ -n "$PINNED_CLI" ] && [ "$LOCAL_CLI" != "$PINNED_CLI" ]; then
  echo "warning: building with stellar-cli $LOCAL_CLI, but CI pins $PINNED_CLI." >&2
  echo "         The CLI version is embedded in the wasm, so hashes will differ" >&2
  echo "         from a build made with $PINNED_CLI even for identical source." >&2
fi

if [ "$SKIP_BUILD" != "1" ]; then
  echo "==> stellar contract build --locked"
  if ! build_log=$(stellar contract build --locked 2>&1); then
    printf '%s\n' "$build_log" >&2
    echo "error: the local build failed" >&2
    exit 2
  fi
fi

local_hash() {
  local wasm=$WASM_DIR/$1.wasm
  [ -f "$wasm" ] || {
    echo "error: $wasm was not built" >&2
    exit 2
  }
  stellar contract info hash --wasm "$wasm" | tr -d '\r'
}

# A read that fails is not a mismatch: retry, then stop with its own exit code
# so a flaky RPC is never reported as "the deployment runs other code".
onchain_hash() {
  local id=$1 out attempt
  for attempt in 1 2 3; do
    if out=$(env -u STELLAR_NETWORK stellar contract info hash --id "$id" \
      --rpc-url "$RPC_URL" --network-passphrase "$PASSPHRASE" 2> /dev/null); then
      printf '%s\n' "$out" | tail -1 | tr -d '\r'
      return 0
    fi
    sleep $((attempt * 2))
  done
  echo "error: could not read the wasm hash of $id from $RPC_URL after 3 attempts" >&2
  exit 3
}

# "rustc X, soroban-sdk Y, stellar-cli Z" from a wasm's contract metadata. It
# only explains a mismatch, so a failed read prints "unknown" rather than
# stopping the run.
build_info() {
  local meta
  if ! meta=$(env -u STELLAR_NETWORK stellar contract info meta "$@" --output json 2> /dev/null); then
    echo "unknown"
    return 0
  fi
  META="$meta" "$PY" -c '
import json, os
entries = json.loads(os.environ["META"].strip().splitlines()[-1])
meta = {}
for entry in entries:
    if "sc_meta_v0" in entry:
        meta[entry["sc_meta_v0"]["key"]] = entry["sc_meta_v0"]["val"]
def short(key):
    return meta.get(key, "?").split("#")[0]
print("rustc %s, soroban-sdk %s, stellar-cli %s" % (short("rsver"), short("rssdkver"), short("cliver")))
' 2> /dev/null || echo "unknown"
}

echo "==> $NETWORK ($RPC_URL)"
mismatches=0
while read -r key id wasm; do
  expected=$(local_hash "$wasm")
  actual=$(onchain_hash "$id")
  if [ "$expected" = "$actual" ]; then
    printf 'ok        %-18s %s  %s\n' "$key" "$id" "$actual"
  else
    mismatches=$((mismatches + 1))
    printf 'MISMATCH  %-18s %s\n' "$key" "$id"
    printf '          expected  %s  (local %s/%s.wasm)\n' "$expected" "$WASM_DIR" "$wasm"
    printf '          actual    %s  (on chain)\n' "$actual"
    built_here=$(build_info --wasm "$WASM_DIR/$wasm.wasm")
    built_there=$(build_info --id "$id" --rpc-url "$RPC_URL" --network-passphrase "$PASSPHRASE")
    printf '          local build:    %s\n' "$built_here"
    printf '          on-chain build: %s\n' "$built_there"
    if [ "$built_here" != "$built_there" ] && [ "$built_there" != "unknown" ]; then
      echo "          (the toolchains differ, so the hashes differ even if the source is the same)"
    fi
  fi
done <<< "$CONTRACTS"

if [ "$mismatches" -gt 0 ]; then
  echo "$mismatches contract(s) on $NETWORK do not run the code built from this checkout ($(git rev-parse --short HEAD 2> /dev/null || echo 'unknown commit'))." >&2
  exit 1
fi
echo "All contracts on $NETWORK match the local build."
