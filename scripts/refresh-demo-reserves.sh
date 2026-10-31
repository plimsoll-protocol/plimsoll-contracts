#!/usr/bin/env bash
# Keep the testnet demo's reserve reports fresh, and check that the demo still
# answers what it claims to answer.
#
#   ./scripts/refresh-demo-reserves.sh verify   # is the demo intact?
#   ./scripts/refresh-demo-reserves.sh due      # are the reports ageing out?
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
# (plimsoll-issuer, plimsoll-auditor, plimsoll-transcriber).
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
TODAY=$(date -u +%F)
AS_OF=$(date -u -d "$TODAY 00:00:00" +%s)
NOW=$(date -u +%s)
# Restate while the reports still have days of life left, so a few missed
# scheduled runs cannot expire the demo.
REFRESH_AFTER_DAYS=${REFRESH_AFTER_DAYS:-3}

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

json() { python -c "import json,sys;d=json.load(open('$DEP'));print(eval(sys.argv[1]))" "$1"; }
LEDGER=$(json "d['contracts']['coverageLedger']")
# Any account works as the source of a read-only simulation, and none of its
# keys are needed.
READ_SOURCE=${READ_SOURCE:-$(json "d['accounts']['admin']")}

read_call() {
  stellar contract invoke --network "$NETWORK" --source-account "$READ_SOURCE" \
    --send=no --id "$LEDGER" -- "$@" 2>/dev/null | tail -1 | tr -d '\r'
}
sac_of() { json "d['assets']['$1']['sac']"; }
doc_path() { echo "docs/demo/$(echo "$1" | tr '[:upper:]' '[:lower:]')-reserves.md"; }

# Prints the age in seconds of the report at a tier, or -1 if there is none.
report_age() {
  local report
  report=$(read_call get_report --sac "$(sac_of "$1")" --tier "$2")
  REPORT_JSON="$report" AT="$NOW" python -c "
import json, os
r = json.loads(os.environ['REPORT_JSON'] or 'null')
print(-1 if not r else int(os.environ['AT']) - r['as_of'])
"
}

case "$MODE" in

verify)
  failed=0
  while IFS='|' read -r CODE MIN_BPS MAX_AGE MIN_TIER WANT; do
    [ -n "$CODE" ] || continue
    SAC=$(sac_of "$CODE")
    GOT=$(read_call is_covered --sac "$SAC" --min_bps "$MIN_BPS" --max_age "$MAX_AGE" --min_tier "$MIN_TIER")
    if [ "$GOT" = "$WANT" ]; then
      echo "ok   $CODE is_covered($MIN_BPS, $MAX_AGE, $MIN_TIER) = $GOT"
      continue
    fi
    echo "FAIL $CODE is_covered($MIN_BPS, $MAX_AGE, $MIN_TIER) = ${GOT:-<no answer>}, expected $WANT"
    echo "     coverage: $(read_call coverage --sac "$SAC" --min_tier "$MIN_TIER")"
    echo "     now:      $NOW ($(date -u +%FT%TZ))"
    failed=1
  done <<< "$CLAIMS"
  if [ "$failed" != 0 ]; then
    echo
    echo "The demo is not showing what it claims. If a report aged out, restate"
    echo "the reserves with: $0 post"
    exit 1
  fi
  echo "The demo's coverage answers are intact."
  ;;

due)
  oldest=0
  limit=$((REFRESH_AFTER_DAYS * 86400))
  while IFS='|' read -r CODE _RESERVES _SIGNER TIER; do
    [ -n "$CODE" ] || continue
    age=$(report_age "$CODE" "$TIER")
    if [ "$age" -lt 0 ]; then
      echo "$CODE tier $TIER has no report at all"
      oldest=$((limit + 1))
      continue
    fi
    echo "$CODE tier $TIER was stated $((age / 86400))d $(((age % 86400) / 3600))h ago"
    [ "$age" -gt "$oldest" ] && oldest=$age
  done <<< "$REPORTS"
  if [ "$oldest" -gt "$limit" ]; then
    echo "due: a report is older than $REFRESH_AFTER_DAYS days"
    exit 0
  fi
  echo "not due: every report is within $REFRESH_AFTER_DAYS days"
  exit 1
  ;;

post)
  # The hash we record has to be the hash of the document GitHub serves, or the
  # report points at something nobody else can verify.
  for DOC in $(awk -F'|' 'NF{print $1}' <<< "$REPORTS" | awk '!seen[$0]++' | while read -r c; do doc_path "$c"; done); do
    [ -f "$DOC" ] || { echo "$DOC is missing from the working tree" >&2; exit 1; }
    WANT=$(sha256sum < "$DOC" | cut -c1-64)
    GOT=$(curl -sfL --retry 3 --retry-delay 5 "$RAW/$DOC" | sha256sum | cut -c1-64)
    if [ "$GOT" != "$WANT" ]; then
      echo "$RAW/$DOC does not match the local file; commit and push it first" >&2
      exit 1
    fi
  done

  while IFS='|' read -r CODE RESERVES SIGNER TIER; do
    [ -n "$CODE" ] || continue
    DOC=$(doc_path "$CODE")
    age=$(report_age "$CODE" "$TIER")
    # post_reserve requires a strictly newer as_of, and refuses one in the
    # future, so there is exactly one restatement per tier per day.
    if [ "$age" -ge 0 ] && [ "$age" -le $((NOW - AS_OF)) ]; then
      echo "$CODE tier $TIER: already restated today"
      continue
    fi
    echo "$CODE tier $TIER: restating $RESERVES as of $TODAY, signed by $SIGNER"
    stellar contract invoke --network "$NETWORK" --source "$SIGNER" --id "$LEDGER" -- \
      post_reserve --reporter "$(stellar keys address "$SIGNER" | tr -d '\r')" \
      --sac "$(sac_of "$CODE")" --amount "$RESERVES" --as_of "$AS_OF" \
      --doc_hash "$(sha256sum < "$DOC" | cut -c1-64)" --doc_uri "$RAW/$DOC" > /dev/null
  done <<< "$REPORTS"

  echo
  echo "Reserve reports are current as of $TODAY. The indexer picks the"
  echo "reserve_posted events up within ~15 minutes."
  ;;
esac
