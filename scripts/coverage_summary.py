#!/usr/bin/env python3
"""Summarise an lcov report per crate.

`cargo llvm-cov` reports per file. A crate is the unit a reviewer thinks in
("did this PR leave coverage-ledger less tested?"), so this folds the lcov
records into one row per crate: line and function totals taken
straight from the LF/LH and FNF/FNH counters llvm-cov writes.

    python3 scripts/coverage_summary.py target/coverage/lcov.info

Prints a plain table. When GITHUB_STEP_SUMMARY is set (inside a GitHub Actions
job) it also appends the same table as Markdown, so it shows on the run page.
Exits non-zero only if the report cannot be read or has no source files.
"""

import os
import pathlib
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent


def crate_of(path: pathlib.Path) -> str:
    """The workspace directory a source file belongs to, e.g. contracts/covered-vault."""
    try:
        rel = path.resolve().relative_to(REPO)
    except ValueError:
        return str(path)
    parts = rel.parts
    if len(parts) >= 2 and parts[0] in ("contracts", "crates", "examples", "tests"):
        return f"{parts[0]}/{parts[1]}"
    return parts[0] if parts else str(rel)


def parse(lcov: pathlib.Path) -> dict:
    totals: dict = {}
    current = None
    for raw in lcov.read_text(encoding="utf-8").splitlines():
        key, _, value = raw.partition(":")
        if key == "SF":
            current = totals.setdefault(crate_of(pathlib.Path(value)), [0, 0, 0, 0])
        elif current is None:
            continue
        elif key == "LF":
            current[0] += int(value)
        elif key == "LH":
            current[1] += int(value)
        elif key == "FNF":
            current[2] += int(value)
        elif key == "FNH":
            current[3] += int(value)
        elif key == "end_of_record":
            current = None
    return totals


def pct(hit: int, found: int) -> str:
    return f"{100.0 * hit / found:.2f}%" if found else "-"


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <lcov.info>", file=sys.stderr)
        return 2
    lcov = pathlib.Path(sys.argv[1])
    if not lcov.is_file():
        print(f"error: {lcov} not found. Run scripts/coverage.sh first.", file=sys.stderr)
        return 1
    totals = parse(lcov)
    if not totals:
        print(f"error: {lcov} lists no source files.", file=sys.stderr)
        return 1

    rows = []
    sums = [0, 0, 0, 0]
    for crate in sorted(totals):
        lf, lh, fnf, fnh = totals[crate]
        sums = [a + b for a, b in zip(sums, (lf, lh, fnf, fnh))]
        rows.append((crate, f"{lh}/{lf}", pct(lh, lf), f"{fnh}/{fnf}", pct(fnh, fnf)))
    lf, lh, fnf, fnh = sums
    rows.append(("TOTAL", f"{lh}/{lf}", pct(lh, lf), f"{fnh}/{fnf}", pct(fnh, fnf)))

    header = ("CRATE", "LINES", "LINE %", "FUNCTIONS", "FUNC %")
    width = max(len(r[0]) for r in rows + [header]) + 2
    line = f"{{:<{width}}}{{:>14}}{{:>10}}{{:>12}}{{:>10}}"
    print(line.format(*header))
    print(line.format("-" * (width - 2), "-" * 12, "-" * 8, "-" * 10, "-" * 8))
    for row in rows:
        if row[0] == "TOTAL":
            print(line.format("-" * (width - 2), "-" * 12, "-" * 8, "-" * 10, "-" * 8))
        print(line.format(*row))

    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as out:
            out.write("## Test coverage by crate\n\n")
            out.write("| " + " | ".join(header) + " |\n")
            out.write("| --- | ---: | ---: | ---: | ---: |\n")
            for crate, lines, lp, funcs, fp in rows:
                name = f"**{crate}**" if crate == "TOTAL" else f"`{crate}`"
                out.write(f"| {name} | {lines} | {lp} | {funcs} | {fp} |\n")
            out.write(
                "\nTest files, `tests/` and `examples/` are excluded. "
                "The full lcov report is attached to this run as the "
                "`coverage-lcov` artifact.\n"
            )
    return 0


if __name__ == "__main__":
    sys.exit(main())
