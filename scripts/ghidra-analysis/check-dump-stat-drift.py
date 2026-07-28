#!/usr/bin/env python3
"""Find committed claims that quote dump statistics the dump no longer reports.

`check-dump-base-integrity.py` asks whether a dump's *printed addresses* are
trustworthy. This asks the sibling question about the dump's *header*: a dump
re-extracted by a better extent walker gets longer, and every sentence written
against the old one keeps asserting a truncation or an emptiness that is no
longer there. Nothing re-reads a caveat when its underlying dump improves, so
the stale sentence outlives the condition it describes - and a caveat is
load-bearing in the worst way, because "the dump is empty, do not port this"
suppresses work silently and leaves no trace that it did.

Two instances motivated the check. A dump quoted at `752 bytes, 188
instructions` and called truncated reads `1528 bytes, 382 instructions` today,
ending on a real `jr ra`; three things had been left unported on the strength
of that caveat. And an address recorded as "**Not a function.** The dump
reports `size=1 bytes, 0 instructions`" decodes today to a complete 11-
instruction `RotTransPers`.

## Why this matches filenames, not addresses

The obvious implementation - pull the address out of the sentence, glob the
dump directory for it - has a false-positive rate that makes it unusable as a
gate, for two reasons this repo hits constantly:

  * **VA aliasing.** One address has a dump per importing program, and the
    siblings legitimately differ. Worse, many of the sentences are *about*
    that aliasing, so "the siblings disagree" is the sentence being right.
  * **Neighbouring counts.** A sentence often quotes a count belonging to a
    different function mentioned in the same breath.

So a line is only checked when it **names exactly one dump file** and quotes a
statistic on that same line. That pins which file the claim is about, which is
the whole ambiguity. A claim that quotes a count without citing its dump is
not checkable by any tool; `--uncited` lists those as prose to fix by hand,
never as a gate failure.

    scripts/ghidra-analysis/check-dump-stat-drift.py
    scripts/ghidra-analysis/check-dump-stat-drift.py --uncited
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
FUNCS = REPO / "ghidra" / "scripts" / "funcs"

# Where committed prose lives. The dumps themselves are gitignored, and the
# ignore list is in scope because its justifications quote dump statistics too.
SCAN_GLOBS = ("docs/**/*.md", "crates/*/README.md", "*.md", "scripts/ci/*.toml")

# `see ghidra/scripts/funcs/<name>.txt`, with or without the directory prefix.
DUMP_REF = re.compile(r"(?:ghidra/scripts/funcs/)?([A-Za-z0-9_]*8[0-9a-fA-F]{7}[A-Za-z0-9_]*\.txt)")

# Prose writes counts with digit-group separators - a comma, or the plain or
# thin space behind "12 104 bytes / 3 026 instructions". Matching a bare `\d+`
# clips the leading group and reports a drift that is only a formatting
# difference; that was three of this checker's first seven flags. So a number
# may carry separators, but only in strict thousands grouping - a separator has
# to be followed by exactly three digits - which stops "reports 3 instructions"
# from swallowing the digits of whatever preceded it. The grouped alternative
# is tried first so the whole number is consumed, and the lookbehind only has
# to bar a digit: a space before a number is the normal case, not a separator.
_SEP = ",\u0020\u00a0\u2009\u202f"
_NUM = "(?<!\\d)(\\d{1,3}(?:[" + _SEP + "]\\d{3})+|\\d+)"
_GAP = "[\u0020\u00a0\u2009\u202f]"
SIZE_RE = re.compile("size=" + _NUM + _GAP + "bytes")
INSN_RE = re.compile(_NUM + _GAP + "instructions?")
_STRIP = re.compile("[" + _SEP + "]")


def as_int(token: str) -> int:
    return int(_STRIP.sub("", token))


def dump_header(name: str) -> tuple[int | None, int | None] | str:
    """`(size_bytes, instruction_count)` from a dump's header.

    Returns a *reason string* instead of a tuple when no comparison is
    possible, so the caller can bucket the skip by cause rather than dropping
    it. The two causes are not the same finding: `absent` means committed prose
    cites a dump nobody in this clone has (unverifiable, and possibly a dump
    that was never produced), while `headerless` means the file is here and
    carries no `size=` line for the claim to be checked against.
    """
    path = FUNCS / name
    if not path.exists():
        return "absent"
    head = path.read_text(errors="replace").split("\n", 6)[:6]
    size = insn = None
    for line in head:
        m = SIZE_RE.search(line)
        if m and size is None:
            size = as_int(m.group(1))
        m = INSN_RE.search(line)
        if m and insn is None:
            insn = as_int(m.group(1))
    return "headerless" if size is None and insn is None else (size, insn)


def scan_lines():
    for pattern in SCAN_GLOBS:
        for path in sorted(REPO.glob(pattern)):
            if not path.is_file():
                continue
            try:
                text = path.read_text(errors="replace")
            except OSError:
                continue
            for n, line in enumerate(text.splitlines(), 1):
                yield path, n, line


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument(
        "--uncited",
        action="store_true",
        help="also list lines quoting a statistic that cite no dump file (advisory, never a failure)",
    )
    args = ap.parse_args()

    if not FUNCS.is_dir():
        print(f"# no dump corpus at {FUNCS} - nothing to check", file=sys.stderr)
        return 0

    drifted, checked, uncited = [], 0, []
    for path, n, line in scan_lines():
        refs = set(DUMP_REF.findall(line))
        sizes = SIZE_RE.findall(line)
        insns = INSN_RE.findall(line)
        if not (sizes or insns):
            continue
        if len(refs) != 1:
            if args.uncited and not refs:
                uncited.append((path, n, line.strip()))
            continue
        header = dump_header(next(iter(refs)))
        if header is None:
            continue
        checked += 1
        size, insn = header
        bad = [f"size={s} bytes (dump reports {size})" for s in sizes if size is not None and as_int(s) != size]
        bad += [f"{i} instructions (dump reports {insn})" for i in insns if insn is not None and as_int(i) != insn]
        if bad:
            drifted.append((path, n, next(iter(refs)), bad, line.strip()))

    for path, n, name, bad, line in drifted:
        print(f"{path.relative_to(REPO)}:{n}: quotes {'; '.join(bad)}  [{name}]")
        print(f"    {line[:200]}")
    if args.uncited:
        print(f"\n# {len(uncited)} line(s) quote a statistic with no dump cited (advisory):")
        for path, n, line in uncited:
            print(f"{path.relative_to(REPO)}:{n}: {line[:160]}")

    print(f"\n# checked {checked} cited claim(s); {len(drifted)} drifted", file=sys.stderr)
    return 1 if drifted else 0


if __name__ == "__main__":
    sys.exit(main())
