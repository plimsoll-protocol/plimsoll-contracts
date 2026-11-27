#!/usr/bin/env python3
"""Measure built contract wasm sizes and report the size change between two
measurements (e.g. a pull request's head and its base branch).

    # After `stellar contract build` on each ref:
    python3 scripts/wasm_size_diff.py measure sizes.json

    # Once both refs have been measured:
    python3 scripts/wasm_size_diff.py report base.json head.json report.md

`report` exits non-zero if any contract grew by more than 10%, so CI fails
the same way scripts/check_wasm_sizes.py does for an absolute budget.
"""

import json
import pathlib
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
WASM_DIR = REPO / "target" / "wasm32v1-none" / "release"
MAX_GROWTH_PCT = 10.0


def measure(out_path: str) -> int:
    if not WASM_DIR.is_dir():
        print(f"error: {WASM_DIR} not found. Run 'stellar contract build' first.", file=sys.stderr)
        return 1
    sizes = {p.stem: p.stat().st_size for p in sorted(WASM_DIR.glob("*.wasm"))}
    pathlib.Path(out_path).write_text(json.dumps(sizes, indent=2, sort_keys=True), encoding="utf-8")
    return 0


def report(base_path: str, head_path: str, out_path: str) -> int:
    base = json.loads(pathlib.Path(base_path).read_text(encoding="utf-8"))
    head = json.loads(pathlib.Path(head_path).read_text(encoding="utf-8"))

    names = sorted(set(base) | set(head))
    lines = [
        "<!-- wasm-size-report -->",
        "### Wasm size change",
        "",
        "| Contract | Base (KB) | PR (KB) | Change |",
        "| --- | ---: | ---: | ---: |",
    ]
    over_budget = []
    for name in names:
        base_bytes = base.get(name)
        head_bytes = head.get(name)
        base_kb = f"{base_bytes / 1024:.2f}" if base_bytes is not None else "-"
        head_kb = f"{head_bytes / 1024:.2f}" if head_bytes is not None else "-"
        if base_bytes is None:
            change = "new"
        elif head_bytes is None:
            change = "removed"
        else:
            delta = head_bytes - base_bytes
            pct = (delta / base_bytes * 100.0) if base_bytes else 0.0
            sign = "+" if delta >= 0 else ""
            change = f"{sign}{delta} B ({sign}{pct:.1f}%)"
            if pct > MAX_GROWTH_PCT:
                over_budget.append(f"{name} grew by {pct:.1f}%, over the {MAX_GROWTH_PCT:.0f}% limit")
        lines.append(f"| {name} | {base_kb} | {head_kb} | {change} |")

    if over_budget:
        lines.append("")
        lines.append("**Fails: growth exceeds the 10% limit.**")
        for problem in over_budget:
            lines.append(f"- {problem}")

    pathlib.Path(out_path).write_text("\n".join(lines) + "\n", encoding="utf-8")

    for problem in over_budget:
        print(f"::error::{problem}", file=sys.stderr)
    return 1 if over_budget else 0


def main() -> int:
    if len(sys.argv) >= 2 and sys.argv[1] == "measure" and len(sys.argv) == 3:
        return measure(sys.argv[2])
    if len(sys.argv) >= 2 and sys.argv[1] == "report" and len(sys.argv) == 5:
        return report(sys.argv[2], sys.argv[3], sys.argv[4])
    print(__doc__, file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
