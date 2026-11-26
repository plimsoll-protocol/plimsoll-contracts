#!/usr/bin/env bash
# Local mirror of the CI commit-lint check (.github/workflows/commitlint.yml).
# Validates commit messages (and optionally pull request titles) against the
# project's conventional-commits convention, the same rule set enforced by
# .commitlintrc.json:
#
#   type(scope): subject
#
#   type    one of feat, fix, docs, ci, test, chore, perf, security
#   scope   one of types, registry, ledger, vault, spec, deploy, ci, deps
#             (may be omitted when a change spans several crates)
#   subject at least 1 character after ": "; whole header at most 100 characters
#
# Usage:
#   validate-commits.sh --pr-title "feat(ledger): add supply age endpoint"
#   validate-commits.sh --range <from-sha> <to-sha>
#   validate-commits.sh            # defaults to commits on this branch not on main
set -euo pipefail

TYPES='feat|fix|docs|ci|test|chore|perf|security'
SCOPES='types|registry|ledger|vault|spec|deploy|ci|deps'
MAX_HEADER_LEN=100

fail() {
    echo "validate-commits: error: $*" >&2
    exit 1
}

# Return 0 if $1 is a member of the pipe-separated list in $2.
in_list() {
    local item="$1" list="$2" entry
    for entry in ${list//|/ }; do
        [ "$item" = "$entry" ] && return 0
    done
    return 1
}

# Validate a single commit message's header line.
validate_header() {
    local header="$1" source="${2:-}" rest type subject scope

    if [ "${#header}" -gt "$MAX_HEADER_LEN" ]; then
        fail "${source:+($source) }header longer than $MAX_HEADER_LEN characters: $header"
    fi

    # Header must be "type(scope): subject" or "type: subject".
    case "$header" in
        *": "*)
            subject="${header#*: }"
            rest="${header%%:*}"
            ;;
        *)
            fail "${source:+($source) }not a conventional commit: $header"
            ;;
    esac

    [ -n "$subject" ] || fail "${source:+($source) }empty subject: $header"

    case "$rest" in
        *"("*")"*)
            type="${rest%%(*}"
            scope="${rest#*(}"
            scope="${scope%)}"
            [ -n "$scope" ] || fail "${source:+($source) }empty scope: $header"
            ;;
        *)
            type="$rest"
            scope=""
            ;;
    esac

    in_list "$type" "$TYPES" || fail "${source:+($source) }unknown type '$type': $header"
    if [ -n "$scope" ]; then
        in_list "$scope" "$SCOPES" || fail "${source:+($source) }unknown scope '$scope': $header"
    fi
}

pr_title=""
range_from=""
range_to=""

while [ $# -gt 0 ]; do
    case "$1" in
        --pr-title)
            [ $# -ge 2 ] || fail "--pr-title requires an argument"
            pr_title="$2"
            shift 2
            ;;
        --range)
            [ $# -ge 3 ] || fail "--range requires two arguments"
            range_from="$2"
            range_to="$3"
            shift 3
            ;;
        --help|-h)
            echo "Usage: $0 [--pr-title <title>] [--range <from-sha> <to-sha>]"
            exit 0
            ;;
        *)
            fail "unknown argument: $1"
            ;;
    esac
done

if [ -n "$pr_title" ]; then
    validate_header "$pr_title" "pull request title"
    exit 0
fi

if [ -n "$range_from" ] && [ -n "$range_to" ]; then
    git rev-parse --verify --quiet "$range_from" >/dev/null ||
        fail "unknown revision: $range_from"
    git rev-parse --verify --quiet "$range_to" >/dev/null ||
        fail "unknown revision: $range_to"
    range="$range_from..$range_to"
else
    git rev-parse --verify --quiet main >/dev/null ||
        fail "no main branch and no range given"
    range="main..HEAD"
fi

# Only commits introduced by the branch are checked; merge commits themselves
# are skipped, the same way CI skips GitHub merge-commit metadata.
while IFS= read -r subject; do
    [ -n "$subject" ] || continue
    validate_header "$subject" "$range commit"
done < <(git log --no-merges --format=%s "$range")

echo "validate-commits: all commit messages in $range are conventional"
