#!/usr/bin/env python3
"""Fail if a `#[contracterror]` variant is never asserted by a test.

Scans every Rust source under `contracts/` and `examples/` for
`#[contracterror]` enums, then looks for `Type::Variant` in the test files of
the same package (`src/test.rs` next to the contract's `lib.rs`, plus anything
under the package's `tests/` directory). A variant that no test names is either
dead code or a missing test; either way it should not ship silently.

The marker is the qualified variant, because that is what an assertion looks
like here:

    assert_eq!(res, Err(Ok(RegistryError::InvalidName.into())));

Only code counts: a mention that survives stripping `//` comments. The
contract's own `lib.rs` is never a test file, so the `panic_with_error!` that
raises the error cannot accidentally count as covering it.

Variants that genuinely cannot be reached from a test belong in
INTENTIONALLY_UNTESTED with a reason. The script fails when an entry there has
gone stale (the variant no longer exists, or it is covered after all), so the
list cannot hide a missing test.

Run locally with:

    python3 scripts/check_error_coverage.py

Exit status: 0 when every variant is covered or documented, 1 otherwise.
"""

import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
SOURCE_ROOTS = ("contracts", "examples")

# Qualified variants that no unit test can reach, mapped to the reason.
# Nothing is allowlisted today: every error code in the workspace has a test.
INTENTIONALLY_UNTESTED = {}

ATTR = re.compile(r"#\[contracterror\]")
ENUM_HEADER = re.compile(r"pub\s+enum\s+(?P<name>[A-Za-z_]\w*)\s*\{")
VARIANT = re.compile(r"^(?P<name>[A-Za-z_]\w*)\s*=\s*(?P<code>\d+)\s*,?\s*$")


def error_enums(source: str):
    """Yield (enum name, [(variant, code), ...]) for each #[contracterror]."""
    for attr in ATTR.finditer(source):
        header = ENUM_HEADER.search(source, attr.end())
        if header is None:
            continue
        body_start = header.end()
        body_end = source.find("\n}", body_start)
        if body_end == -1:
            raise ValueError(f"unterminated `{header['name']}` enum")
        variants = []
        for raw in source[body_start:body_end].splitlines():
            line = raw.split("//", 1)[0].strip()
            if not line or line.startswith("#"):
                continue
            match = VARIANT.match(line)
            if match is None:
                raise ValueError(
                    f"cannot parse `{header['name']}` variant line: {line!r}"
                )
            variants.append((match["name"], int(match["code"])))
        if not variants:
            raise ValueError(f"`{header['name']}` has no variants")
        yield header["name"], variants


def test_sources(package: pathlib.Path):
    """Every file in the package that is allowed to count as a test."""
    found = []
    unit = package / "src" / "test.rs"
    if unit.is_file():
        found.append(unit)
    tests_dir = package / "tests"
    if tests_dir.is_dir():
        found.extend(sorted(tests_dir.rglob("*.rs")))
    return found


def code_only(text: str) -> str:
    """Drop `//` comments, so a variant named only in prose counts for nothing."""
    return "\n".join(line.split("//", 1)[0] for line in text.splitlines())


def main() -> int:
    problems = []
    rows = []
    covered_ids = set()
    real_ids = set()

    sources = []
    for root in SOURCE_ROOTS:
        directory = REPO / root
        if directory.is_dir():
            sources.extend(sorted(directory.rglob("*.rs")))

    for path in sources:
        source = path.read_text(encoding="utf-8")
        if not ATTR.search(source):
            continue

        relative = path.relative_to(REPO)
        package = path.parent.parent
        tests = test_sources(package)
        if not tests:
            problems.append(
                f"{relative} defines a #[contracterror] enum but its package has "
                f"no src/test.rs or tests/ directory."
            )
            continue
        test_code = "\n".join(code_only(p.read_text(encoding="utf-8")) for p in tests)
        test_list = ", ".join(str(p.relative_to(REPO)) for p in tests)

        for enum_name, variants in error_enums(source):
            codes = [code for _, code in variants]
            for code in sorted({c for c in codes if codes.count(c) > 1}):
                problems.append(
                    f"{relative}: {enum_name} uses error code #{code} for more "
                    f"than one variant. Codes are part of the wire format; give "
                    f"the new variant the next free number."
                )

            covered = 0
            for variant, code in variants:
                qualified = f"{enum_name}::{variant}"
                real_ids.add(qualified)
                pattern = rf"\b{re.escape(enum_name)}::{re.escape(variant)}\b"
                if re.search(pattern, test_code):
                    covered += 1
                    covered_ids.add(qualified)
                    continue
                if qualified in INTENTIONALLY_UNTESTED:
                    print(f"documented as untestable: {relative}: {qualified} (#{code})")
                    continue
                problems.append(
                    f"{relative}: {qualified} (#{code}) is exercised by no test. "
                    f"Add an assertion such as `Err(Ok({qualified}.into()))` to "
                    f"{test_list}, or list it in INTENTIONALLY_UNTESTED in this "
                    f"script with the reason it cannot be reached."
                )
            rows.append((str(relative.parent.parent), enum_name, covered, len(variants)))

    for qualified, reason in INTENTIONALLY_UNTESTED.items():
        if qualified not in real_ids:
            problems.append(
                f"INTENTIONALLY_UNTESTED lists {qualified}, which is not a "
                f"variant of any #[contracterror] enum. Remove the entry."
            )
        elif qualified in covered_ids:
            problems.append(
                f"INTENTIONALLY_UNTESTED lists {qualified}, but a test now covers "
                f"it. Remove the entry ({reason})."
            )

    print(f"{'PACKAGE':<32}{'ERROR TYPE':<18}{'COVERED':>8}{'TOTAL':>7}")
    print(f"{'-' * 32:<32}{'-' * 18:<18}{'-' * 8:>8}{'-' * 7:>7}")
    for package, enum_name, covered, total in sorted(rows):
        print(f"{package:<32}{enum_name:<18}{covered:>8}{total:>7}")

    for problem in problems:
        print(f"::error::{problem}", file=sys.stderr)

    if problems:
        print(
            f"\nerror: {len(problems)} error-coverage problem(s). Every contract "
            f"error must be reachable from a test (see docs/ERRORS.md).",
            file=sys.stderr,
        )
        return 1
    print("\nok: every contract error code is covered by a test.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
