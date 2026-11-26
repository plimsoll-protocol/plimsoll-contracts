#!/usr/bin/env bash
# Validate that pull request titles and commits follow Conventional Commits
# (feat/fix/docs/ci/test/chore/perf/security). See .commitlintrc.json.
#
# Usage:
#   validate-commits.sh --pr-title "feat(ledger): add supply age endpoint"
#   validate-commits.sh --range <from-sha> <to-sha>
#
# Merge commits are skipped: GitHub's own merge-commit metadata is not
# conventional, and the PR title plus the individual commits are the signals
# that matter. Only newly introduced commits are linted so existing history on
# `main` never breaks CI.
set -uo pipefail

COMMITLINT="./node_modules/.bin/commitlint"
fail=0

lint_message() {
  local msg="$1"
  [ -n "$msg" ] || return 0
  # commitlint exits non-zero on a non-conventional message; capture its
  # output so the failing message is visible in the log.
  if ! output=$(printf '%s\n' "$msg" | "$COMMITLINT" 2>&1); then
    echo "::error::Not a conventional commit: $msg"
    echo "$output"
    fail=1
  fi
}

mode="${1:-}"
case "$mode" in
  --pr-title)
    lint_message "${2:-}"
    ;;
  --range)
    from="${2:-}"
    to="${3:-}"
    if [ -z "$from" ] || [ -z "$to" ]; then
      echo "::error::--range requires <from-sha> <to-sha>"
      exit 2
    fi
    # --first-parent follows the branch's main line; --no-merges skips merge
    # commits whose messages are GitHub merge metadata, not conventional commits.
    while IFS= read -r sha; do
      [ -n "$sha" ] || continue
      msg="$(git log -1 --format='%s' "$sha")"
      lint_message "$msg"
    done < <(git log --first-parent --no-merges --format='%H' "${from}..${to}")
    ;;
  *)
    echo "::error::Unknown mode '$mode' (expected --pr-title or --range)"
    exit 2
    ;;
esac

exit "$fail"
