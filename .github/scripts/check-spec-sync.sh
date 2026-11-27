#!/usr/bin/env bash
#
# Fail a change that alters a contract's public function signature without
# updating docs/SPEC.md in the same change.
#
# docs/SPEC.md is the source of truth for the contract interface: the indexer,
# the SDKs and every front end decode against it. A signature change is a
# specification change, so the two belong in one pull request (see README.md,
# "Keeping docs/SPEC.md in step with the code").
#
# Usage:
#   .github/scripts/check-spec-sync.sh [<base-ref> [<head-ref>]]
#
# base defaults to $SPEC_SYNC_BASE, then to origin/main; head defaults to
# $SPEC_SYNC_HEAD, then to HEAD. Both must be commits in this clone.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

base=${1:-${SPEC_SYNC_BASE:-origin/main}}
head=${2:-${SPEC_SYNC_HEAD:-HEAD}}

for ref in "$base" "$head"; do
  if ! git rev-parse --verify --quiet "${ref}^{commit}" >/dev/null; then
    echo "check-spec-sync: '$ref' is not a commit in this clone." >&2
    echo "Fetch it first, or pass a ref that exists locally." >&2
    exit 1
  fi
done

# Compare the change itself, not everything the base branch gained since the
# branch point. On a pull-request merge commit the merge base is the base.
base_commit=$(git merge-base "$base" "$head" 2>/dev/null || git rev-parse "${base}^{commit}")

# The contract sources whose public functions are part of the interface. The
# shared types crate is included because it holds the cross-contract client
# traits and the types those traits encode.
contract_sources() {
  git ls-tree -r --name-only "$1" |
    grep -E '^(contracts/[^/]+/src/lib\.rs|crates/types/src/lib\.rs)$' || true
}

# One normalised `pub fn` signature per line, read from stdin. A signature that
# spans several lines is joined until the line that opens the body or ends the
# declaration, so re-wrapping a signature is not a change.
signatures() {
  awk '
    function emit() {
      gsub(/[[:space:]]+/, " ", buf)
      sub(/^ /, "", buf)
      print buf
      buf = ""
    }
    /^[[:space:]]*pub fn / {
      buf = $0
      if ($0 ~ /[{;][[:space:]]*$/) {
        emit()
      } else {
        collecting = 1
      }
      next
    }
    collecting {
      buf = buf " " $0
      if ($0 ~ /[{;][[:space:]]*$/) {
        collecting = 0
        emit()
      }
    }
  '
}

# Every signature in the tree at <ref>, prefixed with its path, one per line.
list_signatures() {
  local ref=$1 out=$2 path
  : >"$out"
  while IFS= read -r path; do
    git show "$ref:$path" | signatures | sed "s|^|$path: |" >>"$out"
  done < <(contract_sources "$ref")
  sort -o "$out" "$out"
}

before=$(mktemp)
after=$(mktemp)
trap 'rm -f "$before" "$after"' EXIT

list_signatures "$base_commit" "$before"
list_signatures "$head" "$after"

changed=$(diff -u --label "before ($base_commit)" --label "after ($head)" "$before" "$after" || true)

if [ -z "$changed" ]; then
  echo "check-spec-sync: no public contract signature changed."
  exit 0
fi

if [ -n "$(git diff --name-only "$base_commit" "$head" -- docs/SPEC.md)" ]; then
  echo "check-spec-sync: public signatures changed and docs/SPEC.md was updated."
  exit 0
fi

{
  echo "check-spec-sync: public contract signatures changed but docs/SPEC.md did not."
  echo
  echo "$changed"
  echo
  echo "docs/SPEC.md is the source of truth for the contract interface. Update it"
  echo "in this change (and docs/ERRORS.md or docs/EVENTS.md when an error code or"
  echo "an event layout changed too), or revert the signature change."
} >&2
exit 1
