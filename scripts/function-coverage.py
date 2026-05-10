#!/usr/bin/env python3
"""Function-coverage tracker for the decompilation track.

Scans every dump under `ghidra/scripts/funcs/*.txt`, extracts every cited
`FUN_xxxxxxxx` / `func_0x80xxxxxx` / `0x80xxxxxx` reference, and reports:

  - which referenced helpers DO have their own dump (covered)
  - which DO NOT (the "missing helpers" punch-list)
  - per-helper incoming-reference count (helpers cited from many places
    are higher-leverage to dump first)

Goal: every function reachable in retail gameplay from the boot path has
either a Ghidra dump file or a documented stub finding.

Usage:
    python3 scripts/function-coverage.py              # text report
    python3 scripts/function-coverage.py --json       # machine-readable
    python3 scripts/function-coverage.py --top 30     # top-N missing helpers
    python3 scripts/function-coverage.py --overlay-breakdown  # per-overlay stats
    python3 scripts/function-coverage.py --graph citations.dot  # Graphviz DOT
    python3 scripts/function-coverage.py --stale      # dump files never cited
"""

import argparse
import json
import re
from collections import Counter, defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
FUNCS_DIR = REPO / "ghidra" / "scripts" / "funcs"

# Match cite forms that are unambiguously *function-call* in nature:
#   - FUN_801c5f40 / FUN_8003ce08    (Ghidra's auto-named function calls)
#   - func_0x801c5f40                (decompile output for unresolved jumps)
#   - jal 0x801c5f40                 (raw disassembly call)
#   - jalr ...; (addr-implied)       (covered via the jal pattern below in
#                                     the rare cases Ghidra emits one)
#
# We deliberately exclude bare 0x80xxxxxx because that pattern catches
# data globals (DAT_8007xxxx etc.) and large LUI/ADDIU constants that are
# not code references. The PSX RAM code surface is roughly:
#   - SCUS_942.54   : 0x80010000 - 0x8006FFFF
#   - Overlays      : 0x801C0000 - 0x8020FFFF (some 0897-loaded code lives
#                                              in the extended 0x80200000+
#                                              region when imported at base
#                                              0x801C0000).
ADDR_RE = re.compile(
    r"(?:FUN_|func_0x|jal\s+0x|jalr\s+\w+,0x|->\s*func_0x)"
    r"(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})"
)


def collect_dumped_addresses() -> set[str]:
    addrs: set[str] = set()
    for p in FUNCS_DIR.glob("*.txt"):
        # Filename is "<addr>.txt" or "overlay_<label>_<addr>.txt".
        m = re.search(r"([0-9a-fA-F]{8})\.txt$", p.name)
        if m:
            addrs.add(m.group(1).lower())
    return addrs


def collect_citations() -> tuple[Counter, dict[str, list[str]]]:
    refs: Counter = Counter()
    sources: dict[str, list[str]] = defaultdict(list)
    for p in sorted(FUNCS_DIR.glob("*.txt")):
        # Skip stats/summary files - they list addresses for inventory
        # purposes, not as real code references, and stale entries pollute
        # the missing-helpers list.
        if (p.name.endswith("_unique_index.txt")
                or p.name.endswith("_index.txt")
                or p.name.endswith("_survey.txt")):
            continue
        try:
            text = p.read_text(errors="ignore")
        except PermissionError:
            # Some Ghidra-container outputs land as root-owned and can't be
            # chowned without sudo. Skip them - they'll show up as missing
            # references but not crash the tool.
            continue
        seen_in_file: set[str] = set()
        for m in ADDR_RE.finditer(text):
            a = m.group(1).lower()
            if a in seen_in_file:
                continue
            seen_in_file.add(a)
            refs[a] += 1
            sources[a].append(p.stem)
    return refs, sources


def classify_stem(stem: str) -> str:
    """Map a dump filename stem to 'scus' or 'overlay/<label>'.

    Overlay dump files are named overlay_<label>_<8hexaddr> by the
    import-overlay-named.sh workflow.  Everything else is SCUS-resident.
    """
    m = re.match(
        r"overlay_([a-zA-Z0-9]+(?:_[a-zA-Z0-9]+)*?)_[0-9a-fA-F]{8}$",
        stem,
        re.IGNORECASE,
    )
    return f"overlay/{m.group(1)}" if m else "scus"


def classify_addr(addr: str) -> str:
    """Return 'scus' or 'overlay' based on the address value."""
    return "scus" if int(addr, 16) < 0x801C0000 else "overlay"


def compute_overlay_breakdown(
    dumped: set,
    sources: dict[str, list[str]],
) -> list[tuple[str, int, int, int, float]]:
    """Return (label, total_cited, covered, missing, pct) rows by source bucket.

    Groups cited addresses by the overlay label of the dump files that
    reference them.  An address cited from both a SCUS dump and an overlay
    dump counts in both buckets; the totals are therefore not additive.
    """
    bucket_cited: dict[str, set] = defaultdict(set)
    for addr, src_files in sources.items():
        for src in src_files:
            bucket_cited[classify_stem(src)].add(addr)

    rows = []
    for label in sorted(bucket_cited):
        addrs = bucket_cited[label]
        total = len(addrs)
        cov = sum(1 for a in addrs if a in dumped)
        miss = total - cov
        pct = cov * 100.0 / total if total else 100.0
        rows.append((label, total, cov, miss, pct))
    return rows


def write_citation_graph(
    out_path: str,
    dumped: set,
    sources: dict[str, list[str]],
) -> int:
    """Write a Graphviz DOT citation graph (covered -> covered edges only).

    Nodes are coloured by address range: SCUS (lightblue) vs overlay
    (lightyellow).  Returns the number of edges written.

    Render with:  dot -Tsvg citations.dot -o citations.svg
    """
    edges: set[tuple[str, str]] = set()
    for cited_addr, src_files in sources.items():
        if cited_addr not in dumped:
            continue
        for src in src_files:
            m = re.search(r"([0-9a-fA-F]{8})$", src, re.IGNORECASE)
            if not m:
                continue
            src_addr = m.group(1).lower()
            if src_addr in dumped and src_addr != cited_addr:
                edges.add((src_addr, cited_addr))

    node_set: set[str] = set()
    for s, d in edges:
        node_set.add(s)
        node_set.add(d)

    with Path(out_path).open("w") as f:
        f.write("digraph citations {\n")
        f.write("  rankdir=LR;\n")
        f.write('  node [shape=box fontsize=9 fontname="monospace"];\n')
        for addr in sorted(node_set):
            color = "lightblue" if classify_addr(addr) == "scus" else "lightyellow"
            f.write(
                f'  "{addr}" [label="{addr}" style=filled fillcolor="{color}"];\n'
            )
        for src, dst in sorted(edges):
            f.write(f'  "{src}" -> "{dst}";\n')
        f.write("}\n")
    return len(edges)


def find_uncited_dumps(dumped: set, refs: Counter) -> list[str]:
    """Return addresses in the dump set that no other dump file cites.

    These are potential root entry-points (expected) or orphan dumps not
    reachable from any traced call graph (potentially stale).
    """
    return sorted(a for a in dumped if a not in refs)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--json", action="store_true", help="emit JSON")
    ap.add_argument(
        "--top",
        type=int,
        default=20,
        help="show top-N missing helpers (default 20)",
    )
    ap.add_argument(
        "--all-missing",
        action="store_true",
        help="show every missing helper, not just top-N",
    )
    ap.add_argument(
        "--overlay-breakdown",
        action="store_true",
        help="print per-overlay citation coverage breakdown",
    )
    ap.add_argument(
        "--graph",
        metavar="FILE",
        help="write Graphviz DOT citation graph (covered->covered) to FILE",
    )
    ap.add_argument(
        "--stale",
        action="store_true",
        help="report dump files never cited by any other dump (potential orphans)",
    )
    args = ap.parse_args()

    if not FUNCS_DIR.exists():
        print(f"funcs dir missing: {FUNCS_DIR}")
        return 1

    dumped = collect_dumped_addresses()
    refs, sources = collect_citations()

    cited = set(refs.keys())
    # Self-references (a dump's own header) shouldn't count as missing.
    covered = cited & dumped
    missing = cited - dumped

    # Sort missing by reference count (highest leverage first), then addr.
    missing_ranked = sorted(
        ((a, refs[a]) for a in missing),
        key=lambda kv: (-kv[1], kv[0]),
    )

    if args.graph:
        n = write_citation_graph(args.graph, dumped, sources)
        print(f"wrote {n} edges to {args.graph}")

    if args.json:
        data: dict = {
            "dumps": len(dumped),
            "cited": len(cited),
            "covered": len(covered),
            "missing": len(missing),
            "missing_ranked": [
                {"addr": a, "refs": n, "first_sources": sources[a][:3]}
                for a, n in missing_ranked
            ],
        }
        if args.overlay_breakdown:
            data["overlay_breakdown"] = [
                {
                    "label": lbl,
                    "total": tot,
                    "covered": cov,
                    "missing": miss,
                    "pct": round(pct, 1),
                }
                for lbl, tot, cov, miss, pct in compute_overlay_breakdown(
                    dumped, sources
                )
            ]
        if args.stale:
            data["stale_dumps"] = find_uncited_dumps(dumped, refs)
        print(json.dumps(data, indent=2))
        return 0

    pct = (len(covered) * 100.0 / len(cited)) if cited else 100.0
    print(f"function dumps           : {len(dumped)}")
    print(f"unique cited addresses   : {len(cited)}")
    print(f"covered (cited & dumped) : {len(covered)} ({pct:.1f}%)")
    print(f"missing helpers          : {len(missing)}")
    print()
    print(f"top {min(args.top, len(missing_ranked))} missing helpers (sorted by citation count):")
    print(f"{'addr':<12} {'refs':>4}  first cited in")
    print(f"{'-' * 12} {'-' * 4}  {'-' * 40}")
    iterable = missing_ranked if args.all_missing else missing_ranked[: args.top]
    for addr, n in iterable:
        first = ", ".join(sources[addr][:3])
        print(f"{addr:<12} {n:>4}  {first}")

    if args.overlay_breakdown:
        print()
        print("per-overlay citation coverage:")
        print(
            f"{'label':<32} {'cited':>6} {'covered':>8} {'missing':>8} {'pct':>6}"
        )
        print(f"{'-' * 32} {'-' * 6} {'-' * 8} {'-' * 8} {'-' * 6}")
        for lbl, tot, cov, miss, pct_val in compute_overlay_breakdown(dumped, sources):
            print(f"{lbl:<32} {tot:>6} {cov:>8} {miss:>8} {pct_val:>5.1f}%")

    if args.stale:
        uncited = find_uncited_dumps(dumped, refs)
        print()
        print(
            f"uncited dump files (never referenced by any other dump): {len(uncited)}"
        )
        for addr in uncited:
            print(f"  {addr}  [{classify_addr(addr)}]")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
