#!/usr/bin/env python3
"""
Byte-level owner resolution for a dump's printed VA, against named candidate
readings.

check-dump-base-integrity.py answers "where do these bytes live?" by indexing
every extracted image and looking the dump's opening tokens up. That needs a
long, unambiguous instruction stream, so it returns no verdict for exactly the
dumps this script exists for: short bodies, and data regions that Ghidra
rendered as bogus instructions (a pointer table decodes as `lb rt,imm(zero)`
rows; capstone and Ghidra disagree on much of that rendering, so token
matching fails even when the bytes are right there).

This script asks the sharper question: given a dump and an explicit list of
candidate readings - (image, VA the image's byte 0 occupies under that
reading) - which candidate's bytes reproduce the dump at its printed VA?
Two comparisons run per candidate, and either can decide:

  tokens  capstone-disassemble the candidate's bytes at the implied offset and
          compare canonicalised tokens row-by-row (the check-dump-base-
          integrity.py canon). Decides code regions.
  words   re-encode the dump rows that admit exact re-encoding - `nop`, and
          the `<load/store> rt,imm(zero)` rendering Ghidra gives a raw data
          word - and compare the 32-bit words directly. Decides data regions,
          where the token comparison cannot: a pointer word carries 32 bits of
          discriminating value, and the re-encoding is exact, so a word match
          is a byte match.

A candidate whose every compared row agrees, while every rival disagrees, owns
the printed VA under that reading. OUT_OF_IMAGE means the reading does not map
the VA at all - itself evidence, since a dump cannot have been taken from a
program that has no memory there.

`--search` additionally scans each candidate image at every offset for the
dump's opening token run, to catch an owner not in the candidate list (the
check that a stated re-key law is complete rather than merely consistent).

Candidate syntax: `label=path@0xVA`, VA = where the image's first byte sits
under that reading. For a footprint import at base B, a stratum starting at
footprint offset S gets VA B+S. Example - the untagged 0x801C0000 import of
PROT 0897's pre-correction footprint (own content 0x25000, then 0898's file):

  resolve-phantom-va.py ghidra/scripts/funcs/overlay_0897_801e4c38.txt \
    --cand field_own=extracted/PROT/0897_xxx_dat.BIN@0x801C0000 \
    --cand battle_tail=extracted/PROT/0898_xxx_dat.BIN@0x801E5000 \
    --cand battle_slotA=extracted/PROT/0898_xxx_dat.BIN@0x801CE818

Reads only gitignored, disc-derived inputs; prints verdicts, deltas and match
counts - no game bytes beyond what a mnemonic carries.
See docs/reference/overlay-va-aliases.md for the standing results.
"""

import argparse
import glob
import importlib.util
import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location(
    "dumpbase", os.path.join(HERE, "check-dump-base-integrity.py"))
dumpbase = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(dumpbase)
canon = dumpbase.canon
_md = dumpbase._md

DIS_ROW_RE = re.compile(r"^([0-9a-fA-F]{8})\s+(\S+)\s*(.*)$")

# MIPS o32 register numbering, Ghidra spellings (s8 = r30).
REGNUM = {}
for _i, _n in enumerate(
        "zero at v0 v1 a0 a1 a2 a3 t0 t1 t2 t3 t4 t5 t6 t7 "
        "s0 s1 s2 s3 s4 s5 s6 s7 t8 t9 k0 k1 gp sp s8 ra".split()):
    REGNUM[_n] = _i
REGNUM["fp"] = 30

# I-format load/store opcodes as Ghidra spells them. A raw data word whose top
# bits form one of these opcodes renders as `<mnem> rt,imm(base)`; with the
# fields re-packed the rendering round-trips to the exact word.
LS_OP = {"lb": 0x20, "lh": 0x21, "lwl": 0x22, "lw": 0x23, "lbu": 0x24,
         "lhu": 0x25, "lwr": 0x26, "sb": 0x28, "sh": 0x29, "swl": 0x2A,
         "sw": 0x2B, "swr": 0x2E, "lwc2": 0x32, "swc2": 0x3A}
LS_RE = re.compile(r"^(\w+)\s*,\s*(-?0x[0-9a-fA-F]+|-?\d+)\s*\((\w+)\)$")


def reencode(mnem, ops):
    """Exact 32-bit word for the re-encodable renderings, else None."""
    mnem = mnem.lower().lstrip("_")
    ops = ops.replace("$", "").strip()
    if mnem == "nop" and not ops:
        return 0
    op = LS_OP.get(mnem)
    if op is None:
        return None
    m = LS_RE.match(ops)
    if not m:
        return None
    rt, base = REGNUM.get(m.group(1)), REGNUM.get(m.group(3))
    if rt is None or base is None:
        return None
    imm = int(m.group(2), 0)
    return (op << 26) | (base << 21) | (rt << 16) | (imm & 0xFFFF)


def parse_rows(path):
    """[(printed_va, mnem, ops)] from the dump's disassembly section."""
    rows, in_dis = [], False
    with open(path, "r", errors="replace") as f:
        for line in f:
            s = line.rstrip("\n").strip()
            if s.startswith("--- DISASSEMBLY"):
                in_dis = True
                continue
            if not in_dis:
                continue
            if s.startswith("---"):
                break
            m = DIS_ROW_RE.match(s)
            if m:
                rows.append((int(m.group(1), 16), m.group(2), m.group(3)))
    return rows


def image_token(data, off):
    for ins in _md.disasm(data[off:off + 4], 0x80000000):
        return canon(ins.mnemonic, ins.op_str)
    return None


def compare(rows, data, off, limit):
    """(tok_match, tok_cmp, word_match, word_cmp) for rows against data@off.

    Each row is keyed by its own printed VA, not by its ordinal - a Ghidra
    listing over a data region skips undefined bytes, so rows are not
    contiguous and `off + 4*i` walks off the dump's own addresses.
    """
    rows = rows[:limit]
    va0 = rows[0][0]
    tm = tc = wm = wc = 0
    for va, mn, op in rows:
        o = off + (va - va0)
        if o < 0 or o + 4 > len(data):
            continue
        tc += 1
        if image_token(data, o) == canon(mn, op):
            tm += 1
        w = reencode(mn, op)
        if w is None:
            continue
        wc += 1
        if struct.unpack_from("<I", data, o)[0] == w:
            wm += 1
    return tm, tc, wm, wc


def search_tokens(rows, data, n):
    """File offsets where the dump's first n tokens reproduce.

    Requires the opening n rows to be printed contiguously (a gapped opening
    cannot anchor a scan); returns [] otherwise.
    """
    if len(rows) < n or rows[n - 1][0] - rows[0][0] != 4 * (n - 1):
        return []
    want = [canon(mn, op) for _, mn, op in rows[:n]]
    toks = []
    for ins in _md.disasm(data, 0):
        toks.append(canon(ins.mnemonic, ins.op_str))
    hits = []
    for i in range(len(toks) - n + 1):
        if toks[i:i + n] == want:
            hits.append(i * 4)
    return hits


def main():
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("dumps", nargs="+",
                    help="dump file(s) or glob(s) under ghidra/scripts/funcs/")
    ap.add_argument("--cand", action="append", required=True,
                    metavar="label=path@0xVA",
                    help="candidate reading: image file + VA of its byte 0")
    ap.add_argument("--limit", type=int, default=64,
                    help="compare at most this many rows (default 64)")
    ap.add_argument("--search", action="store_true",
                    help="also scan each image for the opening token run")
    ap.add_argument("--search-n", type=int, default=10,
                    help="token-run length for --search (default 10)")
    args = ap.parse_args()

    cands = []
    for c in args.cand:
        m = re.match(r"^([^=]+)=(.+)@(0x[0-9a-fA-F]+)$", c)
        if not m:
            ap.error("bad --cand %r (want label=path@0xVA)" % c)
        cands.append((m.group(1), open(m.group(2), "rb").read(),
                      int(m.group(3), 16)))

    files = []
    for pat in args.dumps:
        hits = sorted(glob.glob(pat))
        files.extend(hits if hits else [pat])

    exit_amb = False
    for path in files:
        rows = parse_rows(path)
        name = os.path.basename(path)
        if not rows:
            print("%-48s NO_ROWS (no address-keyed disassembly to compare)"
                  % name)
            continue
        va0 = rows[0][0]
        print("%-48s printed %08x, %d row(s)" % (name, va0, len(rows)))
        owners = []
        for label, data, cva in cands:
            off = va0 - cva
            if off < 0 or off >= len(data):
                print("    %-16s OUT_OF_IMAGE (offset %#x)" % (label, off))
                continue
            tm, tc, wm, wc = compare(rows, data, off, args.limit)
            full = (tc and tm == tc) or (wc >= 4 and wm == wc)
            none = tm == 0 and wm == 0
            verdict = "FULL" if full else ("NONE" if none else "PARTIAL")
            print("    %-16s off=%-#9x tok %d/%d  word %d/%d  %s"
                  % (label, off, tm, tc, wm, wc, verdict))
            if full:
                owners.append(label)
            if args.search:
                for h in search_tokens(rows, data, args.search_n):
                    if h != off:
                        print("    %-16s   token run also at off=%#x "
                              "(va-if-this-reading %08x)"
                              % (label, h, cva + h))
        if len(owners) == 1:
            print("    => OWNER %s" % owners[0])
        elif owners:
            print("    => AMBIGUOUS: %s (byte-identical here)"
                  % ", ".join(owners))
            exit_amb = True
        else:
            print("    => UNRESOLVED under these candidates")
            exit_amb = True
    return 1 if exit_amb else 0


if __name__ == "__main__":
    sys.exit(main())
