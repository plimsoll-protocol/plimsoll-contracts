#!/usr/bin/env bash
# Keep the testnet demo's reserve reports fresh, and check that the demo still
# answers what it claims to answer.
#
#   ./scripts/refresh-demo-reserves.sh verify   # is the demo intact?
#   ./scripts/refresh-demo-reserves.sh due      # does anything need restating?
#   ./scripts/refresh-demo-reserves.sh post     # restate the reserves on chain
#
# The demo's supply side is posted by the indexer every 15 minutes, but its
# reserve side was a set of hand-written statements with a fixed as_of. The
# PUSD vault asks for a report at most 7 days old (`max_age 604800`), so when
# the newest statement was dated 2026-10-07 it expired on 2026-10-14, and for
# the sixteen days that followed `is_covered` answered false for an asset
# backed at 102%: the well-backed vault refused deposits because its statement
# had expired, the under-backed one refused them for two reasons at once, and
# the demo showed no contrast at all.
#
# `verify` is the check that catches that. `due` and `post` are how it gets
# avoided: the statements in docs/demo/{pusd,qusd}-reserves.md are standing
# documents with fixed figures and no date of their own, so restating costs one
# transaction per tier and no commit.
#
# Reads need no key. `post` needs the demo reporter identities
# (plimsoll-issuer, plimsoll-auditor, plimsoll-transcriber), and restates only
# what needs it unless FORCE=true.
set -euo pipefail

MODE=${1:-}
case "$MODE" in
  verify | due | post) ;;
  *)
    echo "usage: $0 verify|due|post" >&2
    exit 2
    ;;
esac

cd "$(dirname "$0")/.."

NETWORK=${NETWORK:-testnet}
DEP=deployments/testnet.json
RAW=https://raw.githubusercontent.com/plimsoll-protocol/plimsoll-contracts/main
NOW=$(date -u +%s)
# Restate while the reports still have days of life left, so a few missed
# scheduled runs cannot expire the demo.
REFRESH_AFTER_DAYS=${REFRESH_AFTER_DAYS:-3}
MAX_AGE=$((REFRESH_AFTER_DAYS * 86400))

# code | reserves in stroops | signing identity | tier the contract will assign
REPORTS="
PUSD|10200000000000|plimsoll-issuer|2
PUSD|10200000000000|plimsoll-auditor|3
QUSD|4800000000000|plimsoll-transcriber|1
"

# What the demo promises, mirroring the vault constructor arguments in
# deploy-testnet.sh and seed-underbacked-demo.sh. PUSD is backed over 100% and
# must be accepted; QUSD is backed at 96% and must be refused for that reason
# and not because its report went stale.
# code | min_bps | max_age | min_tier | expected is_covered
CLAIMS="
PUSD|10000|604800|2|true
QUSD|10000|2678400|1|false
"

# Linux CI has python3 and no python; Windows has a python3 that is a Microsoft
# Store stub resolving on PATH and exiting 49. Run the probe, not `command -v`.
PY=""
for candidate in python3 python; do
  if "$candidate" -c "" > /dev/null 2>&1; then
    PY=$candidate
    break
  fi
done
[ -n "$PY" ] || { echo "no working python interpreter on PATH" >&2; exit 1; }

json() { "$PY" -c "import json,sys;d=json.load(open('$DEP'));print(eval(sys.argv[1]))" "$1"; }
LEDGER=$(json "d['contracts']['coverageLedger']")
# Any account works as the source of a read-only simulation, and none of its
# keys are needed.
READ_SOURCE=${READ_SOURCE:-$(json "d['accounts']['admin']")}

# A read that fails is not a read that returned nothing. Conflating the two
# once reported a tier as having no report at all while it had a valid one, and
# in `verify` that conflation is a false alarm rather than a spurious write, so
# a read retries and then gives up loudly. A contract returning `null` exits 0
# and is passed through as "null".
read_call() {
  local out attempt
  for attempt in 1 2 3; do
    if out=$(stellar contract invoke --network "$NETWORK" --source-account "$READ_SOURCE" \
      --send=no --id "$LEDGER" -- "$@" 2> /dev/null); then
      printf '%s\n' "$out" | tail -1 | tr -d '\r'
      return 0
    fi
    sleep $((attempt * 3))
  done
  echo "read failed after 3 attempts: $*" >&2
  return 1
}
sac_of() { json "d['assets']['$1']['sac']"; }
doc_path() { echo "docs/demo/$(echo "$1" | tr '[:upper:]' '[:lower:]')-reserves.md"; }
hash_of() { sha256sum < "$1" | cut -c1-64; }

# Why the report at a tier needs restating, or nothing if it does not.
#
# Age is one reason. A doc_hash that no longer matches the document is the
# other: these statements live at a stable path, so editing one invalidates the
# hash already on chain and has to be followed by a restatement rather than
# waiting for the next due date.
restate_reason() {
  local code=$1 tier=$2 report want
  report=$(read_call get_report --sac "$(sac_of "$code")" --tier "$tier")
  want=$(hash_of "$(doc_path "$code")")
  REPORT_JSON="$report" AT="$NOW" MAX_AGE="$MAX_AGE" WANT="$want" "$PY" -c "
import json, os
r = json.loads(os.environ['REPORT_JSON'] or 'null')
if not r:
    print('no report at all')
    raise SystemExit
age = int(os.environ['AT']) - r['as_of']
if age > int(os.environ['MAX_AGE']):
    print(f\"stated {age // 86400}d {age % 86400 // 3600}h ago\")
elif r['doc_hash'] != os.environ['WANT']:
    print(f\"doc_hash {r['doc_hash'][:12]}... no longer matches the document\")
"
}

case "$MODE" in

verify)
  failed=0

  while IFS='|' read -r CODE MIN_BPS AGE_LIMIT MIN_TIER WANT; do
    [ -n "$CODE" ] || continue
    SAC=$(sac_of "$CODE")
    GOT=$(read_call is_covered --sac "$SAC" --min_bps "$MIN_BPS" --max_age "$AGE_LIMIT" --min_tier "$MIN_TIER")
    if [ "$GOT" = "$WANT" ]; then
      echo "ok   $CODE is_covered($MIN_BPS, $AGE_LIMIT, $MIN_TIER) = $GOT"
      continue
    fi
    echo "FAIL $CODE is_covered($MIN_BPS, $AGE_LIMIT, $MIN_TIER) = ${GOT:-<no answer>}, expected $WANT"
    echo "     coverage: $(read_call coverage --sac "$SAC" --min_tier "$MIN_TIER")"
    echo "     now:      $NOW ($(date -u +%FT%TZ))"
    failed=1
  done <<< "$CLAIMS"

  # Plimsoll's promise is that anyone can download the document a report names
  # and reproduce its hash. Check it rather than assume it.
  while IFS='|' read -r CODE _RESERVES _SIGNER TIER; do
    [ -n "$CODE" ] || continue
    REPORT=$(read_call get_report --sac "$(sac_of "$CODE")" --tier "$TIER")
    if [ -z "$REPORT" ] || [ "$REPORT" = null ]; then
      echo "FAIL $CODE tier $TIER has no report"
      failed=1
      continue
    fi
    URI=$(REPORT_JSON="$REPORT" "$PY" -c "import json,os;print(json.loads(os.environ['REPORT_JSON'])['doc_uri'])")
    ON_CHAIN=$(REPORT_JSON="$REPORT" "$PY" -c "import json,os;print(json.loads(os.environ['REPORT_JSON'])['doc_hash'])")
    SERVED=$(curl -sfL --max-time 60 --retry 3 --retry-delay 5 "$URI" | sha256sum | cut -c1-64)
    if [ "$SERVED" = "$ON_CHAIN" ]; then
      echo "ok   $CODE tier $TIER doc_hash matches $URI"
    else
      echo "FAIL $CODE tier $TIER doc_hash is $ON_CHAIN but $URI hashes to ${SERVED:-<unreachable>}"
      failed=1
    fi
  done <<< "$REPORTS"

  if [ "$failed" != 0 ]; then
    echo
    echo "The demo is not showing what it claims. Restate the reserves with:"
    echo "  $0 post"
    exit 1
  fi
  echo "The demo's coverage answers are intact and every document verifies."
  ;;

due)
  due=1
  while IFS='|' read -r CODE _RESERVES _SIGNER TIER; do
    [ -n "$CODE" ] || continue
    REASON=$(restate_reason "$CODE" "$TIER")
    if [ -n "$REASON" ]; then
      echo "$CODE tier $TIER needs restating: $REASON"
      due=0
    else
      echo "$CODE tier $TIER is current"
    fi
  done <<< "$REPORTS"
  [ "$due" = 0 ] && echo "due" || echo "not due: every report is within $REFRESH_AFTER_DAYS days"
  exit "$due"
  ;;

post)
  # The hash we record has to be the hash of the document GitHub serves, or the
  # report points at something nobody else can verify.
  for CODE in $(awk -F'|' 'NF{print $1}' <<< "$REPORTS" | awk '!seen[$0]++'); do
    DOC=$(doc_path "$CODE")
    [ -f "$DOC" ] || { echo "$DOC is missing from the working tree" >&2; exit 1; }
    WANT=$(hash_of "$DOC")
    # Right after a push, raw.githubusercontent.com can serve the previous
    # content for a short while, so a mismatch is worth waiting out before
    # calling it an unpushed edit.
    for attempt in $(seq 1 10); do
      SERVED=$(curl -sfL --max-time 60 --retry 2 --retry-delay 5 "$RAW/$DOC" | sha256sum | cut -c1-64)
      [ "$SERVED" = "$WANT" ] && break
      if [ "$attempt" = 10 ]; then
        echo "$RAW/$DOC hashes to ${SERVED:-<unreachable>}, not $WANT." >&2
        echo "Commit and push the document before recording its hash on chain." >&2
        exit 1
      fi
      echo "waiting for GitHub to serve the current $DOC (attempt $attempt)"
      sleep 20
    done
  done

  while IFS='|' read -r CODE RESERVES SIGNER TIER; do
    [ -n "$CODE" ] || continue
    REASON=$(restate_reason "$CODE" "$TIER")
    if [ -z "$REASON" ] && [ "${FORCE:-}" = true ]; then
      REASON="forced"
    fi
    if [ -z "$REASON" ]; then
      echo "$CODE tier $TIER: current, leaving it alone"
      continue
    fi
    DOC=$(doc_path "$CODE")
    echo "$CODE tier $TIER: restating $RESERVES ($REASON), signed by $SIGNER"
    stellar contract invoke --network "$NETWORK" --source "$SIGNER" --id "$LEDGER" -- \
      post_reserve --reporter "$(stellar keys address "$SIGNER" | tr -d '\r')" \
      --sac "$(sac_of "$CODE")" --amount "$RESERVES" --as_of "$NOW" \
      --doc_hash "$(hash_of "$DOC")" --doc_uri "$RAW/$DOC" > /dev/null
  done <<< "$REPORTS"

  echo
  echo "Reserve reports are current. The indexer picks the reserve_posted"
  echo "events up within ~15 minutes."
  ;;
esac
