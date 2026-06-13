#!/usr/bin/env python3
"""Scan ghidra/scripts/funcs/*.txt disassembly dumps for static reads/writes
that land in a given address range, by pairing lui+addiu and lui+(load/store
with offset) within the same function.

Mirrors the in-Ghidra scan in `ghidra/scripts/find_xp_table_readers.py`, but
runs against the on-disc dump corpus so it doesn't need the Ghidra container.
Useful for sweeping "does ANY captured function reach this address range
statically" questions across 3000+ overlay dumps in seconds.

Limitations: the scanner walks instructions linearly without tracking branch
targets or delay-slot reorderings. A `lui` placed in a conditional-branch
delay slot followed by an `lw` past the branch target can be mis-attributed
(see the FUN_801d4fc8 baka_fighter case in the world-map-overlay docs). Cross-
check with grep on the Ghidra-emitted decomp lines (e.g. `_DAT_8007b888`)
when the result needs to be exhaustive.

Usage:
  python3 scripts/ghidra-analysis/scan_funcs_for_addr_range.py --lo 0x8007C190 --hi 0x8007C1E0
  python3 scripts/ghidra-analysis/scan_funcs_for_addr_range.py --lo 0x8007B888 --hi 0x8007B88C
"""
import argparse
import os
import re
from pathlib import Path

INSN_RE = re.compile(r'^([0-9a-f]{8})\s+(\S+)\s+(.*)$')
HEADER_RE = re.compile(r'^==\s+(\S+)\s+([0-9a-f]+)')
DEF_CLEAR = {
    "lw", "lhu", "lh", "lbu", "lb", "li", "move", "or", "and", "subu", "addu",
    "andi", "ori", "xori", "sll", "srl", "sra", "mflo", "mfhi", "lwc1", "lwl",
    "lwr",
}
LOAD_STORE = {"lw", "lh", "lhu", "lb", "lbu", "sw", "sh", "sb"}


def parse_imm(s):
    s = s.strip()
    neg = False
    if s.startswith("-"):
        neg = True
        s = s[1:]
    base = 16 if s.lower().startswith("0x") else 10
    try:
        v = int(s, base)
        return -v if neg else v
    except ValueError:
        return None


def scan_file(path, lo, hi):
    hits = []
    last_lui = {}
    func_label = None
    with open(path, "r", errors="replace") as f:
        for line in f:
            line = line.strip()
            mh = HEADER_RE.match(line)
            if mh:
                func_label = mh.group(1)
                last_lui = {}
                continue
            m = INSN_RE.match(line)
            if not m:
                continue
            pc, mnem, rest = m.group(1), m.group(2), m.group(3).strip()
            if mnem.startswith("_"):
                mnem = mnem[1:]
            ops = [o.strip() for o in rest.split(",")]
            if mnem == "lui" and len(ops) == 2:
                imm = parse_imm(ops[1])
                if imm is not None:
                    last_lui[ops[0]] = (imm & 0xFFFF) << 16
                continue
            if mnem == "addiu" and len(ops) == 3:
                src, dst, imm_s = ops[1], ops[0], ops[2]
                imm = parse_imm(imm_s)
                if imm is not None and src in last_lui:
                    base = last_lui[src]
                    combined = (base + imm) & 0xFFFFFFFF
                    if lo <= combined < hi:
                        hits.append((path, func_label, pc, "addiu",
                                     "0x{:08X}".format(combined), line))
                    last_lui[dst] = combined
                else:
                    last_lui.pop(dst, None)
                continue
            if mnem in LOAD_STORE and len(ops) == 2:
                operand = ops[1]
                lp = operand.find("(")
                rp = operand.find(")")
                if lp > 0 and rp > lp:
                    off_s = operand[:lp]
                    base_reg = operand[lp + 1:rp]
                    off = parse_imm(off_s)
                    if off is not None and base_reg in last_lui:
                        combined = (last_lui[base_reg] + off) & 0xFFFFFFFF
                        if lo <= combined < hi:
                            hits.append((path, func_label, pc, mnem,
                                         "0x{:08X}".format(combined), line))
                continue
            if mnem in DEF_CLEAR and ops:
                last_lui.pop(ops[0], None)
    return hits


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--lo", required=True, help="low addr (incl, hex)")
    ap.add_argument("--hi", required=True, help="hi addr (excl, hex)")
    ap.add_argument("--root", default="ghidra/scripts/funcs")
    ap.add_argument("--show", type=int, default=50,
                    help="show first N hits per file (0 = unlimited)")
    args = ap.parse_args()
    lo = int(args.lo, 16)
    hi = int(args.hi, 16)
    root = Path(args.root)
    all_hits = []
    file_count = 0
    for path in sorted(root.glob("*.txt")):
        file_count += 1
        all_hits.extend(scan_file(path, lo, hi))
    print("Scanned {} files".format(file_count))
    print("Range: 0x{:08X} .. 0x{:08X}".format(lo, hi))
    print("Hits: {}".format(len(all_hits)))
    by_file = {}
    for h in all_hits:
        by_file.setdefault(os.path.basename(h[0]), []).append(h)
    for fname, hs in sorted(by_file.items()):
        print("\n=== {} ({} hits) ===".format(fname, len(hs)))
        cap = args.show if args.show else 999999
        for _, _, pc, mn, addr, line in hs[:cap]:
            print("  {} {:5} {}  -> {}".format(pc, mn, addr, line))


if __name__ == "__main__":
    main()
