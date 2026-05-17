#!/usr/bin/env python3
"""Probe-output analysis companion: diff, fingerprint, regress.

This is the Python-side companion to the `.probe.toml` runtime. It
operates on the CSV outputs that probes produce (see
`scripts/pcsx-redux/probes/*.probe.toml`) and provides three operations
that the Lua side intentionally doesn't try to do in-emulator:

    probe.py diff       BASELINE CURRENT     [--ignore COL[,COL...]]
    probe.py fingerprint RUN                 [--ignore COL[,COL...]]
    probe.py regress    BASELINE CURRENT     [--ignore COL[,COL...]]
    probe.py summary    RUN                  [--ignore COL[,COL...]]

`diff`        - line-by-line set-diff of canonicalised rows.
`fingerprint` - emit a SHA-256 digest of canonicalised rows. Stable
                across row reordering and across columns listed in --ignore.
`regress`     - compare two runs by fingerprint; exit 0 on match, 1 on
                regression. Prints a summary of additions/removals on
                regression. Foundation for Phase G (probe regression CI).
`summary`     - header + row count + fingerprint of a single run.

Canonicalisation:
    * --ignore drops named columns before comparison.
    * Rows are sorted lexicographically before hashing/comparing so two
      runs that emit the same hits in different temporal order match.

Probe CSV shape (vocab defined in scripts/pcsx-redux/lib/probe/spec.lua):
    * Header line first (column names from capture_columns).
    * Each subsequent line is one breakpoint hit.

Examples:
    # One-time baseline capture committed (per probe spec):
    probe.py fingerprint title_overlay_tick.csv > title_overlay_tick.fingerprint

    # CI gate: re-run the probe, fingerprint it, compare:
    diff <(probe.py fingerprint title_overlay_tick.csv) title_overlay_tick.fingerprint

    # Manual diff after a code change:
    probe.py diff baseline.csv current.csv --ignore tick
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import sys
from pathlib import Path
from typing import Iterable


def load(path: Path) -> tuple[list[str], list[tuple[str, ...]]]:
    """Return ``(header, rows)`` for a probe CSV.

    Empty file -> ``([], [])``. Header-only -> ``(header, [])``.
    """
    if not path.exists():
        raise SystemExit(f"probe.py: {path} not found")
    with open(path, "r", newline="", encoding="utf-8") as f:
        rd = csv.reader(f)
        rows = list(rd)
    if not rows:
        return [], []
    header = [c.strip() for c in rows[0]]
    body = [tuple(r) for r in rows[1:]]
    return header, body


def drop_columns(header: list[str], rows: list[tuple[str, ...]],
                 ignore: set[str]) -> tuple[list[str], list[tuple[str, ...]]]:
    """Return (header', rows') with `ignore` columns removed.

    Raises if any `ignore` name isn't present in `header` (cheap typo
    guard - silently ignoring a missing column would mask regressions).
    """
    if not ignore:
        return header, rows
    missing = ignore - set(header)
    if missing:
        raise SystemExit(
            f"probe.py: --ignore mentions columns not in CSV header: "
            f"{sorted(missing)}; header is {header}")
    keep = [i for i, c in enumerate(header) if c not in ignore]
    new_header = [header[i] for i in keep]
    new_rows = [tuple(r[i] for i in keep) for r in rows]
    return new_header, new_rows


def canonicalise(header: list[str], rows: list[tuple[str, ...]],
                 ignore: set[str]) -> tuple[list[str], list[tuple[str, ...]]]:
    """Drop ignored columns and sort rows lex-ascending."""
    h, r = drop_columns(header, rows, ignore)
    return h, sorted(r)


def fingerprint(header: list[str], rows: list[tuple[str, ...]],
                ignore: set[str]) -> str:
    """Stable SHA-256 over canonical-form CSV bytes.

    The hash inputs are the column names (joined by `|`) followed by
    each canonical row's joined cells, each line newline-terminated.
    Independent of row order; the `|` separator is uncommon enough in
    probe-emitted values (which are mostly hex addresses + tick counts)
    to avoid the cell-vs-newline ambiguity that comma-separated forms
    have for ad-hoc strings.
    """
    h, r = canonicalise(header, rows, ignore)
    sha = hashlib.sha256()
    sha.update(("|".join(h) + "\n").encode("utf-8"))
    for row in r:
        sha.update(("|".join(row) + "\n").encode("utf-8"))
    return sha.hexdigest()


def diff_rows(header_a: list[str], rows_a: list[tuple[str, ...]],
              header_b: list[str], rows_b: list[tuple[str, ...]],
              ignore: set[str]) -> tuple[list[str], list[tuple[str, ...]], list[tuple[str, ...]]]:
    """Return (final_header, added_in_b, removed_from_a).

    Headers must agree after dropping ignored columns (probe shape
    changes between runs are a separate concern; this tool only
    compares same-shape runs).
    """
    h_a, r_a = drop_columns(header_a, rows_a, ignore)
    h_b, r_b = drop_columns(header_b, rows_b, ignore)
    if h_a != h_b:
        raise SystemExit(
            f"probe.py: post-ignore headers differ:\n"
            f"  baseline: {h_a}\n"
            f"  current:  {h_b}")
    set_a, set_b = set(r_a), set(r_b)
    added = sorted(set_b - set_a)
    removed = sorted(set_a - set_b)
    return h_a, added, removed


def fmt_diff_block(label: str, header: list[str],
                   rows: Iterable[tuple[str, ...]]) -> str:
    rows = list(rows)
    if not rows:
        return f"{label}: 0 rows\n"
    cols = ",".join(header)
    body = "\n".join("    " + ",".join(r) for r in rows[:50])
    suffix = ""
    if len(rows) > 50:
        suffix = f"\n    ... and {len(rows) - 50} more"
    return f"{label}: {len(rows)} rows\n  columns: {cols}\n{body}{suffix}\n"


def cmd_summary(args) -> int:
    header, rows = load(args.run)
    ignore = set(args.ignore) if args.ignore else set()
    fp = fingerprint(header, rows, ignore)
    print(f"file:        {args.run}")
    print(f"header:      {header}")
    print(f"row count:   {len(rows)}")
    if ignore:
        print(f"ignored:     {sorted(ignore)}")
    print(f"fingerprint: {fp}")
    return 0


def cmd_fingerprint(args) -> int:
    header, rows = load(args.run)
    ignore = set(args.ignore) if args.ignore else set()
    print(fingerprint(header, rows, ignore))
    return 0


def cmd_diff(args) -> int:
    header_a, rows_a = load(args.baseline)
    header_b, rows_b = load(args.current)
    ignore = set(args.ignore) if args.ignore else set()
    final_header, added, removed = diff_rows(
        header_a, rows_a, header_b, rows_b, ignore)
    print(fmt_diff_block("Added (in current, not baseline)", final_header, added))
    print(fmt_diff_block("Removed (in baseline, not current)", final_header, removed))
    if not added and not removed:
        print("(no differences after canonicalisation)")
        return 0
    return 1


def cmd_regress(args) -> int:
    header_a, rows_a = load(args.baseline)
    header_b, rows_b = load(args.current)
    ignore = set(args.ignore) if args.ignore else set()
    fp_a = fingerprint(header_a, rows_a, ignore)
    fp_b = fingerprint(header_b, rows_b, ignore)
    if fp_a == fp_b:
        print(f"REGRESS OK  {args.baseline} == {args.current}  ({fp_a[:16]}...)")
        return 0
    final_header, added, removed = diff_rows(
        header_a, rows_a, header_b, rows_b, ignore)
    print(f"REGRESS FAIL  {args.baseline} -> {args.current}", file=sys.stderr)
    print(f"  baseline fingerprint: {fp_a}", file=sys.stderr)
    print(f"  current  fingerprint: {fp_b}", file=sys.stderr)
    print(file=sys.stderr)
    sys.stderr.write(fmt_diff_block("Added", final_header, added))
    sys.stderr.write(fmt_diff_block("Removed", final_header, removed))
    return 1


def _ignore_arg(s: str) -> list[str]:
    """Parse ``--ignore tick,pc`` into a list of column names."""
    return [c.strip() for c in s.split(",") if c.strip()]


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        prog="probe.py",
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_sum = sub.add_parser("summary", help="header + row count + fingerprint")
    p_sum.add_argument("run", type=Path)
    p_sum.add_argument("--ignore", type=_ignore_arg, default=[])
    p_sum.set_defaults(fn=cmd_summary)

    p_fp = sub.add_parser("fingerprint", help="emit SHA-256 of canonical rows")
    p_fp.add_argument("run", type=Path)
    p_fp.add_argument("--ignore", type=_ignore_arg, default=[])
    p_fp.set_defaults(fn=cmd_fingerprint)

    p_df = sub.add_parser("diff", help="row-level set diff between two CSVs")
    p_df.add_argument("baseline", type=Path)
    p_df.add_argument("current", type=Path)
    p_df.add_argument("--ignore", type=_ignore_arg, default=[])
    p_df.set_defaults(fn=cmd_diff)

    p_rg = sub.add_parser("regress", help="fingerprint compare; exit 1 on mismatch")
    p_rg.add_argument("baseline", type=Path)
    p_rg.add_argument("current", type=Path)
    p_rg.add_argument("--ignore", type=_ignore_arg, default=[])
    p_rg.set_defaults(fn=cmd_regress)

    args = ap.parse_args(argv)
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
