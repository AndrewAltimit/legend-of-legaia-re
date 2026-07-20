#!/usr/bin/env python3
"""Apply the savestate-resume fix to a recomp runtime checkout.

The recomp runtime cannot resume a savestate as shipped: ``boot_state.c``'s
``apply_section`` forces ``cpu->pc = entry_pc`` on load, and that entry is the
game's BSS-clear routine, so every load restores the RAM image and then zeroes
the region holding the game-mode word ``0x8007B83C``. The machine drops back
into the boot chain and parks there, while the debug server still acks
``{"ok":true}``. The fix is to honour the PC ``savestate_poll`` recorded,
falling back to the entry only for snapshots that predate resume-PC capture.

The recomp workspace is an untracked sibling tree, so this edit has no home
there and has been lost to a stray ``git checkout`` before. This script is the
durable copy.

**Why a script and not a .patch:** psxrecomp is PolyForm Noncommercial, this
repo is MIT OR Unlicense. A context diff would vendor third-party source lines
under a license this repo does not grant. An anchored replacement carries only
the line it rewrites, and as a bonus it survives upstream line-number drift
that would break a diff.

Usage:

    python3 scripts/recomp/apply_boot_state_fix.py            # apply
    python3 scripts/recomp/apply_boot_state_fix.py --check    # report only
    python3 scripts/recomp/apply_boot_state_fix.py --revert   # back to stock

Applying is idempotent. Rebuild afterwards with ``make psx-runtime`` (see
docs/tooling/recomp-differential.md - the executable's own name is a no-op
make target).
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import preflight  # noqa: E402

# The stock assignment, and the one that resumes correctly. Whitespace-tolerant
# so an upstream reformat does not silently turn this into a no-op.
STOCK_RE = re.compile(r"^([ \t]*)cpu->pc\s*=\s*entry_pc\s*;.*$", re.MULTILINE)

FIXED_LINE = "cpu->pc = c->pc ? c->pc : entry_pc;"

COMMENT = """/* Resume at the SAVED PC. savestate.c (the sole caller of
 * boot_state_load) stages the block-leader resume PC into c->pc before the
 * snapshot, then re-dispatches via psx_scheduler_resume_at(cpu->pc). Forcing
 * entry_pc here re-runs the BSS-clear boot routine and zeroes the game-state
 * region (e.g. the mode word at 0x8007B83C) on every load, so mid-game
 * savestates silently self-wipe. Fall back to entry_pc only for legacy
 * snapshots predating resume-PC capture (c->pc == 0).
 * Patched by legend-of-legaia-re: scripts/recomp/apply_boot_state_fix.py */"""

STOCK_LINE = "cpu->pc = entry_pc;   /* always enter at the game entry, never a mid-PC */"


def render_fixed(indent: str) -> str:
    body = "\n".join(
        indent + line if line else "" for line in COMMENT.split("\n")
    )
    return body + "\n" + indent + FIXED_LINE


def apply_fix(path: str) -> str:
    """Returns 'applied', 'already-applied', or raises."""
    text = Path(path).read_text()
    if preflight._FORM_FIXED.search(text):
        return "already-applied"
    matches = list(STOCK_RE.finditer(text))
    if not matches:
        raise SystemExit(
            f"{path}: could not find the stock 'cpu->pc = entry_pc;' assignment.\n"
            "The runtime may have been restructured - re-derive the fix by hand "
            "rather than trusting this script."
        )
    if len(matches) > 1:
        raise SystemExit(
            f"{path}: found {len(matches)} 'cpu->pc = entry_pc;' assignments, "
            "expected exactly 1 - refusing to guess which one resumes a "
            "savestate."
        )
    m = matches[0]
    new = text[: m.start()] + render_fixed(m.group(1)) + text[m.end():]
    Path(path).write_text(new)
    return "applied"


def revert_fix(path: str) -> str:
    """Restore the stock assignment (drops our comment block with it)."""
    text = Path(path).read_text()
    if not preflight._FORM_FIXED.search(text):
        return "already-stock"
    lines = text.split("\n")
    idx = next(i for i, ln in enumerate(lines) if preflight._FORM_FIXED.search(ln))
    indent = re.match(r"[ \t]*", lines[idx]).group(0)

    # Walk back over the contiguous comment block directly above the
    # assignment; drop it only if it is the one this script wrote.
    start = idx
    while start > 0 and lines[start - 1].lstrip().startswith(("/*", "*")):
        start -= 1
    block = "\n".join(lines[start:idx])
    if "apply_boot_state_fix.py" not in block:
        start = idx  # somebody else's comment - leave it alone

    lines[start:idx + 1] = [indent + STOCK_LINE]
    Path(path).write_text("\n".join(lines))
    return "reverted"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument(
        "--recomp-dir", help="recomp workspace or runtime checkout "
                             "(default $LEGAIA_RECOMP_DIR)"
    )
    g = ap.add_mutually_exclusive_group()
    g.add_argument("--check", action="store_true", help="report the form, change nothing")
    g.add_argument("--revert", action="store_true", help="restore the stock assignment")
    args = ap.parse_args(argv)

    recomp = args.recomp_dir or os.environ.get("LEGAIA_RECOMP_DIR")
    if not recomp:
        print("recomp workspace not configured: set LEGAIA_RECOMP_DIR or pass "
              "--recomp-dir", file=sys.stderr)
        return 2
    recomp = os.path.expanduser(recomp)
    src = preflight.boot_state_source(recomp)
    if src is None:
        print(f"boot_state.c not found under {recomp} "
              "(looked in psxrecomp/runtime/src and runtime/src)", file=sys.stderr)
        return 2

    if args.check:
        print(f"{src}\n  form: {preflight.runtime_form(recomp)}")
        stale = preflight.build_is_stale(recomp)
        print(f"  build stale: {'unknown' if stale is None else stale}")
        return 0

    if args.revert:
        print(f"{src}: {revert_fix(src)}")
        return 0

    result = apply_fix(src)
    print(f"{src}: {result}")
    if result == "applied":
        print("now rebuild:  cd "
              f"{os.path.join(recomp, 'build-dbg')} && make psx-runtime")
        print("(NOT `make Legend_of_Legaia_Recompiled` - that is a silent no-op)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
