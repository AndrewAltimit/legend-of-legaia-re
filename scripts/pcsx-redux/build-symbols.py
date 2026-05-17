#!/usr/bin/env python3
"""
Generate ghidra/scripts/symbols.json (and a parallel symbols.lua) from the
per-function decompilation dumps under ghidra/scripts/funcs/.

Each dump's first line has the canonical name + entry address, e.g.
    == FUN_801de840 801de840 (entry=801de840) ==

The address here is the source of truth: the filename (e.g. 8001de840.txt)
is just a slot, and a single function can appear under multiple slots
(an inline body referenced from several callers gets duplicated by the
dump script). Names dedupe by (name -> address) with a warning on conflict.

Output:
    ghidra/scripts/symbols.json   {"FUN_801de840": "0x801de840", ...}
    ghidra/scripts/symbols.lua    -- return { ... } table for direct dofile()
                                  -- from the PCSX-Redux Lua side without
                                  -- needing a JSON parser.

Both files are committed so probes don't have to regenerate them at
launch. Run this script after adding new dumps under ghidra/scripts/funcs/.
"""

import argparse
import json
import os
import re
import sys
from typing import Dict, List, Tuple

# Address ranges in which a DAT_/`_DAT_` reference is a plausible global
# symbol. Decomp output cites `_DAT_00000004`-style stack-offset names
# that aren't symbols at all; filtering by these ranges drops the noise.
DAT_RANGES = (
    (0x80000000, 0x80200000),  # main RAM (mirrored)
    (0x1F800000, 0x1F800400),  # scratchpad
)


def addr_in_dat_ranges(addr: int) -> bool:
    for lo, hi in DAT_RANGES:
        if lo <= addr < hi:
            return True
    return False


# Decomp body references like `_DAT_8007BCD0` or `&DAT_8007078c`. Match
# either form; the leading underscore is just Ghidra's pointer-syntax
# decoration and points at the same address.
DAT_REF_RE = re.compile(r"\b(_?DAT_)([0-9a-fA-F]{8})\b")


def _record(symbols: Dict[str, int], name: str, addr: int,
            source: str, warnings: List[str]) -> None:
    """Add (name -> addr); warn on conflicting reassignment, keep first."""
    prev = symbols.get(name)
    if prev is None:
        symbols[name] = addr
    elif prev != addr:
        warnings.append(
            f"{name}: conflicting addresses 0x{prev:08X} vs 0x{addr:08X} "
            f"(latter from {source}); keeping first"
        )

# Header-line shapes the dump scripts emit:
#   "== FUN_801de840 801de840 (entry=801de840) =="            (canonical)
#   "== FUN_8001fa34 0x8001fa34 (entry=0x8001fa34) =="        (0x-prefixed)
#   "== FUN_xxx ADDR (entry=ADDR) [SCUS_942.54] =="           (program tag)
#   "== FUN_xxx ADDR (entry=ADDR) free-form note"             (trailing note,
#                                                              no closing ==)
#
# We accept all four by anchoring on the name + entry= field and tolerating
# any tail. The `entry=` address is the source of truth.
HEADER_RE = re.compile(
    r"^==[^=]*?(FUN_[0-9a-fA-F]+|[A-Za-z_]\w*)\s+"
    r"(?:(?:0x)?[0-9a-fA-F]+\s+)?"          # optional redundant ADDR before the paren
    r"\(entry=(?:0x)?([0-9a-fA-F]+)(?:\s*,\s*label=([A-Za-z_]\w*))?\)"
)
# Data-table dumps use `(len=N)` instead of `(entry=ADDR)`. The address is
# the symbol's location; we keep it the same as the function path.
DATA_HEADER_RE = re.compile(
    r"^==\s*([A-Za-z_][\w]*)\s+(?:0x)?([0-9a-fA-F]+)\s*\(len="
)
# Block-data dumps: "== data_0x801c8f00 DATA REGION 0x801C8F00..0x801C93FF =="
DATA_REGION_RE = re.compile(
    r"^==\s*([A-Za-z_][\w]*)\s+DATA REGION\s+0x([0-9a-fA-F]+)\.\.0x[0-9a-fA-F]+"
)

# Known alternate shapes that don't define a new symbol:
#   "== 801c5cf8 (cite of FUN_801c5c90) =="  - inline-call mirror; the
#       citation site is dumped under its own filename, but the canonical
#       symbol is the original FUN_xxx already covered by another dump.
#   "== 0896_bat_back_dat overlay survey =="  - overlay survey report.
#   "program: overlay_xxx.bin"               - dump-script header preamble.
#   "========================================================================"  - separator.
SKIPPABLE_HEADER_RES = [
    re.compile(r"^==\s*[0-9a-fA-F]+\s*\(cite of\s+\w+\)\s*==\s*$"),
    re.compile(r"^==\s*citation pointer\s+0x[0-9a-fA-F]+\s*==\s*$"),
    re.compile(r"^==\s+\w+\s+overlay survey\s+==\s*$"),
    re.compile(r"^program:\s+\S+"),
    re.compile(r"^=+\s*$"),
]


def parse_funcs_dir(funcs_dir: str) -> Tuple[Dict[str, int], List[str]]:
    """Walk funcs/*.txt and collect (name, address) pairs.

    Returns (symbols, warnings). `symbols` maps the canonical FUN_/named
    symbol to its address as an int.

    Each file is scanned twice:
      1. The first line for the canonical entry-point header (FUN_*, named
         data tables, data regions, OR per-line globals dumps emitted by
         dump_globals.py).
      2. The body for `DAT_xxxxxxxx` / `_DAT_xxxxxxxx` references. Each
         in-range hit is added to the table as an alias-pair (both name
         forms point at the same address). This gives the probe layer
         immediate symbol coverage without requiring a separate Ghidra
         run after every new function dump.
    """
    symbols: Dict[str, int] = {}
    warnings: List[str] = []
    skipped_known = 0
    skipped_unknown: List[str] = []
    body_harvested = 0

    for entry in sorted(os.listdir(funcs_dir)):
        if not entry.endswith(".txt"):
            continue
        path = os.path.join(funcs_dir, entry)
        try:
            with open(path, "r", encoding="utf-8", errors="replace") as fh:
                lines = fh.readlines()
        except OSError:
            warnings.append(f"cannot open {path}")
            continue

        if not lines:
            continue

        # The globals_<program>.txt sidecar files have many `== NAME ADDR
        # (len=N) ==` lines, not just one. Detect that shape up front and
        # treat every matching line as a header.
        is_globals_dump = entry.startswith("globals_")

        if is_globals_dump:
            for ln in lines:
                ln = ln.rstrip("\r\n")
                m = DATA_HEADER_RE.match(ln) or DATA_REGION_RE.match(ln)
                if not m:
                    continue
                name, addr_hex = m.group(1), m.group(2)
                addr = int(addr_hex, 16)
                _record(symbols, name, addr, entry, warnings)
                # Also record the alias form: DAT_xxx <-> _DAT_xxx point at
                # the same address. Decomp output uses _DAT_ for indirect-
                # value reads and DAT_ for address-of, but both names should
                # resolve in probe specs.
                if name.startswith("_DAT_"):
                    _record(symbols, name[1:], addr, entry, warnings)
                elif name.startswith("DAT_"):
                    _record(symbols, "_" + name, addr, entry, warnings)
            continue

        first_line = lines[0].rstrip("\r\n")
        m = (HEADER_RE.match(first_line)
             or DATA_HEADER_RE.match(first_line)
             or DATA_REGION_RE.match(first_line))
        if not m:
            if any(r.match(first_line) for r in SKIPPABLE_HEADER_RES):
                skipped_known += 1
            else:
                skipped_unknown.append(f"{entry}: {first_line[:80]!r}")
            # Note: we still harvest body refs from this file below even if
            # the header didn't parse - the body content is still valuable.
        else:
            name = m.group(1)
            entry_addr_hex = m.group(2)
            addr = int(entry_addr_hex, 16)
            _record(symbols, name, addr, entry, warnings)
            # When the header carries `label=ALIAS`, also record that alias.
            # HEADER_RE captures group 3 only; the data variants don't.
            if m.lastindex and m.lastindex >= 3 and m.group(3):
                _record(symbols, m.group(3), addr, entry, warnings)

        # Harvest DAT_/`_DAT_` references from the body. Both forms point
        # at the same address; add aliases so probe specs can use either.
        body = "".join(lines[1:])
        for prefix, addr_hex in DAT_REF_RE.findall(body):
            addr = int(addr_hex, 16)
            if not addr_in_dat_ranges(addr):
                continue
            canonical_dat = "DAT_" + addr_hex
            canonical_under = "_DAT_" + addr_hex
            if canonical_dat not in symbols and canonical_under not in symbols:
                body_harvested += 1
            _record(symbols, canonical_dat, addr, entry, warnings)
            _record(symbols, canonical_under, addr, entry, warnings)

    if body_harvested:
        warnings.append(
            f"harvested {body_harvested} DAT_*/_DAT_* references from body content "
            "(set via dump_globals.py for authoritative names + lengths)"
        )

    # Warn for genuinely unknown header shapes (signal that the regex
    # needs broadening). The known-skip count is informational only.
    for line in skipped_unknown[:10]:
        warnings.append(f"unrecognised header: {line}")
    if len(skipped_unknown) > 10:
        warnings.append(f"... and {len(skipped_unknown) - 10} more unrecognised headers")
    if skipped_known:
        warnings.append(
            f"skipped {skipped_known} non-symbol dumps (citation mirrors / "
            "overlay surveys / program-preamble lines); these don't define new symbols"
        )
    return symbols, warnings


def emit_json(symbols: Dict[str, int], path: str) -> None:
    # Sort by address for stable diffs across regenerations. JSON values are
    # zero-padded uppercase hex strings so consumers don't have to guess
    # whether the integer overflowed their language's signed 32-bit range.
    payload = {
        "_about": (
            "Auto-generated from ghidra/scripts/funcs/*.txt headers "
            "by scripts/pcsx-redux/build-symbols.py. Do not edit by hand; "
            "regenerate after adding new function dumps."
        ),
        "symbols": {
            name: f"0x{addr:08X}"
            for name, addr in sorted(symbols.items(), key=lambda kv: (kv[1], kv[0]))
        },
    }
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(payload, fh, indent=2)
        fh.write("\n")


def emit_lua(symbols: Dict[str, int], path: str) -> None:
    # Plain `return { ... }` table so the PCSX-Redux Lua side can do
    #   local symbols = dofile("ghidra/scripts/symbols.lua")
    # without bundling a JSON parser. LuaJIT handles 32-bit unsigned
    # literals natively; emit as 0x prefixed lowercase for readability.
    lines = [
        "-- Auto-generated by scripts/pcsx-redux/build-symbols.py.",
        "-- Source of truth: ghidra/scripts/funcs/*.txt headers.",
        "-- Do not edit by hand; regenerate after adding new dumps.",
        "return {",
    ]
    for name, addr in sorted(symbols.items(), key=lambda kv: (kv[1], kv[0])):
        # Lua identifiers must start with a letter or _; FUN_xxx satisfies
        # this. Quote-bracket the key for any name that wouldn't be a valid
        # bareword (defensive; the funcs dumps don't currently produce any).
        if re.match(r"^[A-Za-z_][\w]*$", name):
            lines.append(f"    {name} = 0x{addr:08x},")
        else:
            lines.append(f"    [{name!r}] = 0x{addr:08x},")
    lines.append("}\n")
    with open(path, "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines))


def main() -> int:
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    default_funcs = os.path.join(repo_root, "ghidra", "scripts", "funcs")
    default_json = os.path.join(repo_root, "ghidra", "scripts", "symbols.json")
    default_lua = os.path.join(repo_root, "ghidra", "scripts", "symbols.lua")

    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--funcs", default=default_funcs,
                    help=f"funcs/ directory (default: {default_funcs})")
    ap.add_argument("--json", default=default_json,
                    help=f"output JSON path (default: {default_json})")
    ap.add_argument("--lua", default=default_lua,
                    help=f"output Lua path (default: {default_lua})")
    args = ap.parse_args()

    if not os.path.isdir(args.funcs):
        print(f"error: funcs dir not found: {args.funcs}", file=sys.stderr)
        return 1

    symbols, warnings = parse_funcs_dir(args.funcs)

    for w in warnings:
        print(f"warning: {w}", file=sys.stderr)

    if not symbols:
        print("error: no symbols extracted; nothing to emit", file=sys.stderr)
        return 1

    emit_json(symbols, args.json)
    emit_lua(symbols, args.lua)
    print(f"wrote {len(symbols):,} symbols to {args.json}")
    print(f"wrote {len(symbols):,} symbols to {args.lua}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
