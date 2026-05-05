#!/usr/bin/env python3
"""Function-coverage tracker for the decompilation track.

Scans every dump under `ghidra/scripts/funcs/*.txt`, extracts every cited
`FUN_xxxxxxxx` / `func_0x80xxxxxx` / `0x80xxxxxx` reference, and reports:

  - which referenced helpers DO have their own dump (covered)
  - which DO NOT (the "missing helpers" punch-list)
  - per-helper incoming-reference count (helpers cited from many places
    are higher-leverage to dump first)

Closes the loop on PRD §4.2 acceptance criterion: "every function reachable
in retail gameplay from the boot path has either a Ghidra dump or a
documented stub finding".

Usage:
    python3 scripts/function-coverage.py            # text report
    python3 scripts/function-coverage.py --json     # machine-readable
    python3 scripts/function-coverage.py --top 30   # top-N missing helpers
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
        # Skip stats/summary files — they list addresses for inventory
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
            # chowned without sudo. Skip them — they'll show up as missing
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

    if args.json:
        print(
            json.dumps(
                {
                    "dumps": len(dumped),
                    "cited": len(cited),
                    "covered": len(covered),
                    "missing": len(missing),
                    "missing_ranked": [
                        {"addr": a, "refs": n, "first_sources": sources[a][:3]}
                        for a, n in missing_ranked
                    ],
                },
                indent=2,
            )
        )
        return 0

    print(f"function dumps           : {len(dumped)}")
    print(f"unique cited addresses   : {len(cited)}")
    print(f"covered (cited & dumped) : {len(covered)}")
    print(f"missing helpers          : {len(missing)}")
    print()
    print(f"top {min(args.top, len(missing_ranked))} missing helpers (sorted by citation count):")
    print(f"{'addr':<12} {'refs':>4}  first cited in")
    print(f"{'-' * 12} {'-' * 4}  {'-' * 40}")
    iterable = missing_ranked if args.all_missing else missing_ranked[: args.top]
    for addr, n in iterable:
        first = ", ".join(sources[addr][:3])
        print(f"{addr:<12} {n:>4}  {first}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
