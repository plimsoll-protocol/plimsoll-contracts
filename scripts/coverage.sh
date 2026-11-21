#!/usr/bin/env bash
# Measure which contract lines the tests reach.
#
#   ./scripts/coverage.sh          # lcov report + per-crate summary
#   ./scripts/coverage.sh --html   # also an HTML report to browse
#
# Writes target/coverage/lcov.info (and target/coverage/html/ with --html),
# then prints one row per crate. CI runs exactly this and uploads the lcov
# file as the `coverage-lcov` artifact.
#
# Requires cargo-llvm-cov and the llvm-tools-preview component:
#   rustup component add llvm-tools-preview
#   cargo install cargo-llvm-cov --locked
#
# Test files are excluded from the figures: a test always "covers" itself.
# cargo-llvm-cov also leaves out tests/ and examples/ by default, which drops
# tests/e2e (it runs the compiled wasm, which carries no instrumentation) and
# examples/coverage-gate (documentation, not a deployed contract).
set -euo pipefail

cd "$(dirname "$0")/.."

if ! cargo llvm-cov --version >/dev/null 2>&1; then
  echo "error: cargo-llvm-cov is not installed." >&2
  echo "  rustup component add llvm-tools-preview" >&2
  echo "  cargo install cargo-llvm-cov --locked" >&2
  exit 1
fi

IGNORE='(/test\.rs$|/tests/e2e/)'
OUT=target/coverage
mkdir -p "$OUT"

# Default workspace members only; see `default-members` in Cargo.toml.
cargo llvm-cov clean --workspace
cargo llvm-cov --no-report
cargo llvm-cov report --ignore-filename-regex "$IGNORE" --lcov --output-path "$OUT/lcov.info"
if [ "${1:-}" = "--html" ]; then
  cargo llvm-cov report --ignore-filename-regex "$IGNORE" --html --output-dir "$OUT"
  echo "HTML report: $OUT/html/index.html"
fi

echo
python3 scripts/coverage_summary.py "$OUT/lcov.info"
