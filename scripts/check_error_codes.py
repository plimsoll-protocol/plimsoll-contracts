#!/usr/bin/env python3
"""Fail when a contract error enum was renumbered instead of appended to.

The indexer, the web app and third-party front ends decode a contract error by
its number, so a variant's discriminant is fixed for good once it ships: new
variants are appended with the next free number, and no existing variant is
renumbered or removed (see docs/ERRORS.md). This compares every
`#[contracterror]` enum in the working tree with the same enum at a base Git
revision and fails on anything but an append.

    python3 scripts/check_error_codes.py --base origin/main

Reads files only; it never builds, imports or runs repository code.

Exit status: 0 = append-only, 1 = a renumbering, a removal or a bad append,
2 = usage error.
"""
from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys

# The attribute that marks a contract error enum.
CONTRACTERROR = re.compile(r"#\s*\[\s*contracterror\s*\]")
ENUM_NAME = re.compile(r"\benum\s+([A-Za-z_][A-Za-z0-9_]*)")
# `Name = 7,` or `Name,` (implicit, one past the previous variant).
VARIANT = re.compile(r"^([A-Za-z_][A-Za-z0-9_]*)\s*(?:=\s*([0-9]+))?\s*,?$")
SKIP_DIRS = {".git", "target", "node_modules"}


def enum_body(text: str, start: int):
    """The name and body of the first enum declared after `start`."""
    name = ENUM_NAME.search(text, start)
    if name is None:
        return None
    opening = text.find("{", name.end())
    if opening == -1:
        return None
    depth = 0
    for index in range(opening, len(text)):
        char = text[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return name.group(1), text[opening + 1 : index]
    return None


def variants(body: str) -> dict:
    """Variant name -> discriminant, counting implicit ones like the compiler."""
    found: dict = {}
    next_value = 0
    for raw in body.splitlines():
        line = raw.split("//", 1)[0].strip()
        if not line or line.startswith("#") or line.startswith("/*"):
            continue
        match = VARIANT.match(line)
        if match is None:
            continue
        name, explicit = match.group(1), match.group(2)
        value = int(explicit) if explicit is not None else next_value
        found[name] = value
        next_value = value + 1
    return found


def parse_errors(text: str, path: str) -> dict:
    """Enum name -> (path, {variant: discriminant}) for one source file."""
    found: dict = {}
    for marker in CONTRACTERROR.finditer(text):
        parsed = enum_body(text, marker.end())
        if parsed is None:
            continue
        name, body = parsed
        found[name] = (path, variants(body))
    return found


def rust_sources(root: str):
    """(relative path, text) for every .rs file in the working tree."""
    for base, dirs, files in os.walk(root):
        dirs[:] = sorted(d for d in dirs if d not in SKIP_DIRS and not d.startswith("."))
        for name in sorted(files):
            if not name.endswith(".rs"):
                continue
            full = os.path.join(base, name)
            rel = os.path.relpath(full, root).replace(os.sep, "/")
            with open(full, "r", encoding="utf-8", errors="replace") as handle:
                yield rel, handle.read()


def git(root: str, *args: str) -> str:
    done = subprocess.run(
        ["git", "-C", root, *args],
        capture_output=True,
        text=True,
        check=False,
    )
    if done.returncode != 0:
        raise SystemExit(f"git {' '.join(args)} failed:\n{done.stderr.strip()}")
    return done.stdout


def base_sources(root: str, ref: str):
    """(relative path, text) for every .rs file at `ref`."""
    listing = git(root, "ls-tree", "-r", "--name-only", ref)
    for rel in listing.splitlines():
        if not rel.endswith(".rs"):
            continue
        yield rel, git(root, "show", f"{ref}:{rel}")


def collect(root: str, ref: str | None) -> dict:
    """Every error enum, keyed by enum name, from `ref` or the working tree."""
    enums: dict = {}
    problems: list = []
    sources = base_sources(root, ref) if ref is not None else rust_sources(root)
    for path, text in sources:
        for name, (where, values) in parse_errors(text, path).items():
            if name in enums and enums[name][1] != values:
                problems.append(
                    f"{name}: defined twice with different numbers ({enums[name][0]} "
                    f"and {where}); annotate one enum per name"
                )
            enums.setdefault(name, (where, values))
    return {"enums": enums, "problems": problems}


def compare(before: dict, after: dict) -> list:
    """Problems that make `after` something other than an append to `before`."""
    problems: list = []
    for name in sorted(before):
        base_path, old = before[name]
        if name not in after:
            problems.append(
                f"{name} ({base_path}): enum was removed or renamed; keep it so "
                f"existing error numbers keep their meaning"
            )
            continue
        head_path, new = after[name]
        for variant in sorted(old, key=lambda v: old[v]):
            if variant not in new:
                problems.append(
                    f"{name}.{variant} = {old[variant]} ({base_path}): variant "
                    f"removed; leave it in place and stop raising it instead"
                )
            elif new[variant] != old[variant]:
                problems.append(
                    f"{name}.{variant} ({head_path}): {old[variant]} -> "
                    f"{new[variant]}; existing variants are never renumbered"
                )
        ceiling = max(old.values()) if old else 0
        for variant in sorted(new, key=lambda v: new[v]):
            if variant in old:
                continue
            if new[variant] <= ceiling:
                problems.append(
                    f"{name}.{variant} = {new[variant]} ({head_path}): a new "
                    f"variant is appended, so its number must be greater than "
                    f"{ceiling}; use {ceiling + 1} or higher"
                )
        by_number: dict = {}
        for variant, value in new.items():
            by_number.setdefault(value, []).append(variant)
        for value in sorted(by_number):
            if len(by_number[value]) > 1:
                problems.append(
                    f"{name} ({head_path}): {' and '.join(sorted(by_number[value]))} "
                    f"share the number {value}"
                )
    return problems


def main(argv) -> int:
    parser = argparse.ArgumentParser(
        description="Check that contract error numbers are only ever appended."
    )
    parser.add_argument(
        "--base",
        required=True,
        help="Git revision to compare against, for example origin/main",
    )
    parser.add_argument(
        "--root", default=".", help="Repository root (default: the current directory)"
    )
    args = parser.parse_args(argv)
    root = os.path.abspath(args.root)

    before = collect(root, args.base)
    after = collect(root, None)
    if not before["enums"]:
        print(
            f"no contract error enums found at {args.base}: is that a revision "
            f"of this repository? (try --base origin/main after `git fetch`)",
            file=sys.stderr,
        )
        return 2
    if not after["enums"]:
        print("no contract error enums found in the working tree", file=sys.stderr)
        return 2
    problems = before["problems"] + after["problems"] + compare(
        before["enums"], after["enums"]
    )

    if problems:
        print(
            "error codes are append-only: docs/ERRORS.md and AGENTS.md require it.",
            file=sys.stderr,
        )
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1

    found = len(after["enums"])
    print(f"ok: {found} contract error enum(s) append-only against {args.base}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
