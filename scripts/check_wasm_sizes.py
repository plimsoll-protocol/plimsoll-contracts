#!/usr/bin/env python3
"""Compare each built contract wasm against its budget in size-budgets.json.

Run after `stellar contract build`. Prints a table and exits non-zero naming
every contract over budget, so a size regression shows up in review instead of
at deploy time, where a wasm too large to install is an awkward surprise.
"""

import json
import pathlib
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
BUDGETS = REPO / "size-budgets.json"
WASM_DIR = REPO / "target" / "wasm32v1-none" / "release"


def main() -> int:
    if not WASM_DIR.is_dir():
        print(f"error: {WASM_DIR} not found. Run 'stellar contract build' first.", file=sys.stderr)
        return 1

    budgets = json.loads(BUDGETS.read_text(encoding="utf-8"))["budgets"]
    built = {p.stem: p for p in sorted(WASM_DIR.glob("*.wasm"))}
    problems = []

    print(f"{'CONTRACT':<20}{'BYTES':>10}{'BUDGET':>10}{'HEADROOM':>10}")
    print(f"{'-' * 20:<20}{'-' * 10:>10}{'-' * 10:>10}{'-' * 10:>10}")

    for name, budget in sorted(budgets.items()):
        wasm = built.get(name)
        if wasm is None:
            print(f"{name:<20}{'not built':>10}{budget:>10}{'-':>10}")
            problems.append(
                f"{name}.wasm was not built, but size-budgets.json has a budget "
                f"for it. Was the contract renamed or removed?"
            )
            continue
        size = wasm.stat().st_size
        print(f"{name:<20}{size:>10}{budget:>10}{budget - size:>10}")
        if size > budget:
            problems.append(
                f"{name} grew to {size} bytes, over its {budget}-byte budget by "
                f"{size - budget}. Make it smaller, or raise the budget in "
                f"size-budgets.json in this commit and say why."
            )

    # A contract with no budget is a contract nobody is watching.
    for name in built:
        if name not in budgets:
            problems.append(f"{name}.wasm has no budget in size-budgets.json. Add one.")

    for problem in problems:
        print(f"::error::{problem}", file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
