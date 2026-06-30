#!/usr/bin/env python3
"""Generate the trace-driven-coverage gap-set worklist.

The trace-driven coverage program (see docs/tooling/playthrough-coverage.md)
arms a non-pausing exec breakpoint on every function-entry address that is
NOT-YET-UNDERSTOOD, then plays the opening of the game and records which of
those functions actually execute. A hit means "an unexplained function ran in
this segment" - the highest-value documentation target.

The gap-set is defined off the port catalog (single source of truth -
scripts/ci/port-catalog.py):

    dumped        = a Ghidra dump exists under ghidra/scripts/funcs/<addr>.txt
                    (so the address is a real function entry we can break on)
    documented    = the address is cited from at least one file under docs/
    ignored       = the address is in scripts/ci/port-catalog-ignore.toml
                    (host-replaced PsyQ / BIOS / libgte / libspu - excluded
                    because they are hot every frame and not RE targets)

    GAP-SET = dumped AND NOT documented AND NOT ignored

We deliberately exclude `documented` (already understood - re-tracing it wastes
breakpoints + PCSX interpreter budget) and `ignored` (host infra that fires
every frame and would flood the trace). The remaining set is ~the 780 entries
the handoff anticipated.

Output: a committed text worklist the Lua probe reads. One address per line:

    0x801dd35c  overlay_title_801dd35c   # bucket=overlay refs=12

Lines beginning with `#` are comments; the probe parses the leading 0x-address
token and ignores the rest. The `dump_stem` second token carries the overlay
identity so the offline triage step knows which overlay a hit belongs to
without a second lookup (important: overlay-range addresses are VA-aliased -
the same 0x801xxxxx can host different overlays, so the stem + the per-hit mode
column in the trace CSV together attribute an overlay hit).

Usage:
    scripts/pcsx-redux/build_gap_worklist.py            # write the default worklist
    scripts/pcsx-redux/build_gap_worklist.py --bucket scus   # SCUS-only (cleanest signal)
    scripts/pcsx-redux/build_gap_worklist.py --stdout   # print, don't write
"""

import argparse
import importlib.util
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
CATALOG_PY = REPO / "scripts" / "ci" / "port-catalog.py"
DEFAULT_OUT = REPO / "scripts" / "pcsx-redux" / "gap_worklist.txt"


def load_catalog_module():
    """Import scripts/ci/port-catalog.py (hyphenated name -> importlib)."""
    spec = importlib.util.spec_from_file_location("port_catalog", CATALOG_PY)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def build_rows():
    pc = load_catalog_module()
    dumped = pc.collect_dumped()
    refs, sources = pc.collect_citations()
    docs = pc.collect_doc_citations()
    ports = pc.collect_ports()
    ignore = pc.load_ignore()
    return pc.build_rows(dumped, refs, sources, docs, ports, ignore=ignore)


def gap_set(rows, bucket: str | None):
    out = [
        r
        for r in rows
        if r["dumped"] and not r["documented"] and not r["ignored"]
    ]
    if bucket:
        out = [r for r in out if r["bucket"] == bucket]
    # SCUS first (unambiguous, always-resident signal), then overlay; address
    # order within each bucket so the file diffs cleanly run-to-run.
    out.sort(key=lambda r: (r["bucket"] != "scus", r["addr"]))
    return out


def render(rows) -> str:
    n_scus = sum(1 for r in rows if r["bucket"] == "scus")
    n_overlay = sum(1 for r in rows if r["bucket"] == "overlay")
    lines = [
        "# gap_worklist.txt - trace-driven-coverage gap-set (GENERATED)",
        "#",
        "# Regenerate: scripts/pcsx-redux/build_gap_worklist.py",
        "# Definition: dumped AND NOT documented AND NOT ignored (port catalog).",
        "# Each row = a not-yet-understood function entry to arm an exec BP on.",
        "#",
        "# A hit in a segment's trace.csv = this function ran in the opening =",
        "# a documentation target. SCUS addresses (0x800xxxxx) are unambiguous;",
        "# overlay addresses (0x801c..0x8020) are VA-aliased - attribute them with",
        "# the trace CSV's first_mode column + the dump_stem here.",
        "#",
        f"# Counts: {len(rows)} total ({n_scus} scus, {n_overlay} overlay).",
        "#",
        "# Columns: <0x-addr>  <dump_stem>   # bucket=<b> refs=<n>",
    ]
    for r in rows:
        stem = r["dump_source"] or "?"
        lines.append(
            f"0x{r['addr']}  {stem}   # bucket={r['bucket']} refs={r['refs']}"
        )
    return "\n".join(lines) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--bucket",
        choices=["scus", "overlay"],
        help="restrict to one bucket (scus = cleanest, always-resident signal)",
    )
    ap.add_argument(
        "--out",
        type=Path,
        default=DEFAULT_OUT,
        help=f"output path (default: {DEFAULT_OUT.relative_to(REPO)})",
    )
    ap.add_argument(
        "--stdout", action="store_true", help="print to stdout, don't write a file"
    )
    args = ap.parse_args()

    rows = gap_set(build_rows(), args.bucket)
    text = render(rows)
    if args.stdout:
        sys.stdout.write(text)
    else:
        args.out.write_text(text)
        n_scus = sum(1 for r in rows if r["bucket"] == "scus")
        print(
            f"wrote {args.out.relative_to(REPO)}: {len(rows)} addresses "
            f"({n_scus} scus, {len(rows) - n_scus} overlay)"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
