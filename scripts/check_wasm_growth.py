#!/usr/bin/env python3
"""Warn, but never fail, when a built contract wasm outgrows its baseline.

`scripts/check_wasm_sizes.py` is the hard ceiling: a wasm over its budget in
`size-budgets.json` fails CI. This script is the early-warning layer between
budgets, where a slow drip of features can eat the headroom without anything
turning red. It compares each built wasm with `wasm-baseline.json` and prints a
GitHub `::warning::` annotation when the file is more than 5% larger, then
exits 0 either way.

Why warn instead of fail: a wasm that grows is not automatically wrong. What
matters is that the growth is seen and argued for, in review, rather than
absorbed silently. When the growth is intended, re-measure and record the new
size in `wasm-baseline.json` in the same pull request; `docs/wasm-size.md`
explains how to produce the numbers and where the bytes go.

Run locally after `stellar contract build`:

    python3 scripts/check_wasm_growth.py

Exit status is always 0; warnings go to stdout as annotations.
"""

import json
import pathlib
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
BASELINE = REPO / "wasm-baseline.json"
WASM_DIR = REPO / "target" / "wasm32v1-none" / "release"

# More growth than this over the recorded baseline gets an annotation.
GROWTH_WARN_RATIO = 0.05


def main() -> int:
    if not WASM_DIR.is_dir():
        print(
            f"::warning::{WASM_DIR} not found, so wasm growth was not checked. "
            "Run 'stellar contract build' first.",
            file=sys.stderr,
        )
        return 0

    baseline = json.loads(BASELINE.read_text(encoding="utf-8"))["baseline"]
    built = {p.stem: p for p in sorted(WASM_DIR.glob("*.wasm"))}

    print(f"{'CONTRACT':<20}{'BYTES':>10}{'BASELINE':>10}{'GROWTH':>10}")
    print(f"{'-' * 20:<20}{'-' * 10:>10}{'-' * 10:>10}{'-' * 10:>10}")

    for name, recorded in sorted(baseline.items()):
        wasm = built.pop(name, None)
        if wasm is None:
            print(f"{name:<20}{'not built':>10}{recorded:>10}{'-':>10}")
            print(
                f"::warning::{name}.wasm was not built, so its growth was not "
                f"checked. Was the contract renamed or removed?",
                file=sys.stderr,
            )
            continue
        size = wasm.stat().st_size
        growth = (size - recorded) / recorded * 100
        print(f"{name:<20}{size:>10}{recorded:>10}{growth:>9.1f}%")
        if size > recorded * (1 + GROWTH_WARN_RATIO):
            print(
                f"::warning::{name} is {size} bytes, {growth:.1f}% over its "
                f"{recorded}-byte baseline (warning line is "
                f"{GROWTH_WARN_RATIO:.0%}). See docs/wasm-size.md for where the "
                f"bytes go; if the growth is intended, record the new size in "
                f"wasm-baseline.json in this pull request and say why.",
                file=sys.stderr,
            )

    # A wasm nobody compares is a wasm nobody watches.
    for name in built:
        print(
            f"::warning::{name}.wasm has no entry in wasm-baseline.json, so its "
            f"growth was not checked. Add one.",
            file=sys.stderr,
        )

    return 0


if __name__ == "__main__":
    sys.exit(main())
