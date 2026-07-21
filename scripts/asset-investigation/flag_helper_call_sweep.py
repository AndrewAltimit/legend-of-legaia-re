#!/usr/bin/env python3
"""Disc-wide sweep for native callers of the story-flag helpers.

Finds every `jal`/`j` word targeting FUN_8003CE08 (SET) / FUN_8003CE34 (CLEAR)
/ FUN_8003CE64 (TEST) across SCUS_942.54 + every extracted PROT entry, then
classifies the `a0` operand at each site (constant / computed) by decoding the
delay slot + up to 12 preceding instructions with capstone.

A decoded call word is a property of the bytes, not of the load base
(docs/tooling/call-target-integrity.md), so no base map is needed to FIND
sites; only the printed VA of a site needs the overlay's base
(crates/asset/data/static-overlays.toml).

This is the sweep that surfaced the native minigame-overlay writers of the
Sol game-hall result toggle 0x50A (Muscle Dome PROT 0977 + the dance trio
0978..0980) and exhausted the native space for the writer-less 0x5D6 - see
docs/subsystems/script-vm.md ("Native flag-bank writers").

Usage: flag_helper_call_sweep.py <extracted-root> [flag-hex ...]
Optional flag arguments (e.g. 0x50A 0x5D6) mark matching constant sites.
"""
import glob
import os
import re
import struct
import sys

from capstone import Cs, CS_ARCH_MIPS, CS_MODE_MIPS32, CS_MODE_LITTLE_ENDIAN

TARGETS = {
    0x8003CE08: "SET",
    0x8003CE34: "CLEAR",
    0x8003CE64: "TEST",
}


def call_words(va):
    idx = (va & 0x0FFFFFFF) >> 2
    return [0x0C000000 | idx, 0x08000000 | idx]  # jal, j


PATTERNS = {}
for va, kind in TARGETS.items():
    for w in call_words(va):
        PATTERNS[struct.pack("<I", w)] = (kind, "jal" if (w >> 26) == 3 else "j")

md = Cs(CS_ARCH_MIPS, CS_MODE_MIPS32 | CS_MODE_LITTLE_ENDIAN)

A0_WRITERS = (
    "addiu", "ori", "li", "lui", "move", "addu", "or", "andi", "lbu", "lhu",
    "lw", "lb", "lh", "sll", "srl", "sra", "subu", "xori", "sllv", "srav",
)


def classify_a0(buf, off):
    """Walk the window ending at the call's delay slot for the last a0 write."""
    last = None
    lo = max(0, off - 12 * 4)
    for o in range(lo, off + 8, 4):
        if o == off or o + 4 > len(buf):
            continue
        dis = list(md.disasm(buf[o : o + 4], o))
        if not dis:
            continue
        i = dis[0]
        if i.op_str.startswith("$a0,") and i.mnemonic in A0_WRITERS:
            last = (o, i.mnemonic, i.op_str)
    if last is None:
        return "unknown"
    o, mn, ops = last
    m = re.match(r"\$a0, \$zero, (-?\w+)", ops)
    if mn in ("addiu", "ori") and m:
        return "const:%#x" % (int(m.group(1), 0) & 0xFFFF)
    m = re.match(r"\$a0, (-?\w+)$", ops)
    if mn == "li" and m:
        return "const:%#x" % (int(m.group(1), 0) & 0xFFFFFFFF)
    return "computed:%s %s @+%#x" % (mn, ops, o)


def main():
    root = sys.argv[1]
    marks = {int(a, 0) for a in sys.argv[2:]}
    files = [os.path.join(root, "SCUS_942.54")] + sorted(
        glob.glob(os.path.join(root, "PROT", "*.BIN"))
    )
    const, computed = {}, []
    for path in files:
        buf = open(path, "rb").read()
        name = os.path.basename(path)
        for pat, (kind, form) in PATTERNS.items():
            start = 0
            while True:
                i = buf.find(pat, start)
                if i < 0:
                    break
                start = i + 1
                if i % 4:
                    continue
                cls = classify_a0(buf, i)
                if cls.startswith("const:"):
                    v = int(cls.split(":")[1], 0)
                    const.setdefault((kind, v), []).append("%s+%#x" % (name, i))
                else:
                    computed.append((name, i, kind, form, cls))
    print("== constant-operand sites ==")
    for (kind, v), sites in sorted(const.items(), key=lambda kv: kv[0][1]):
        mark = "  <== target" if v in marks else ""
        print("%-5s %#6x  x%d  %s%s" % (kind, v, len(sites), ", ".join(sites[:6]), mark))
    print("== computed/unknown-operand sites ==")
    for name, off, kind, form, cls in computed:
        print("%-5s %s+%#x  %s (%s)" % (kind, name, off, cls, form))


if __name__ == "__main__":
    main()
