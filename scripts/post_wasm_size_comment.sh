#!/usr/bin/env bash
# Create or update the wasm-size-report comment on a pull request.
#
#   ./scripts/post_wasm_size_comment.sh <repo> <pr-number> <body-file>
#
# Finds any existing comment containing the "wasm-size-report" marker (see
# scripts/wasm_size_diff.py) and edits it in place, so a PR gets one comment
# that updates on every push instead of a new one each time. Requires GH_TOKEN
# and the gh CLI, both already present on GitHub-hosted runners.
set -euo pipefail

REPO="$1"
PR_NUMBER="$2"
BODY_FILE="$3"

EXISTING=$(gh api "repos/${REPO}/issues/${PR_NUMBER}/comments" --paginate \
  --jq '.[] | select(.body | contains("wasm-size-report")) | .id' | head -n1)

if [ -n "$EXISTING" ]; then
  gh api "repos/${REPO}/issues/comments/${EXISTING}" -X PATCH -f body="@${BODY_FILE}"
else
  gh api "repos/${REPO}/issues/${PR_NUMBER}/comments" -X POST -f body="@${BODY_FILE}"
fi
