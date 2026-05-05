#!/usr/bin/env python3
# Scan extracted PROT entries for MIPS-code-likelihood. The 0x801C0000+
# overlay was confirmed in second-pass recon to live in dynamically loaded
# PROT data, not in SCUS_942.54. Heuristic ranking:
#
#   * entropy in the range typical for MIPS code (5.5 - 6.7 bits/byte)
#   * many `jr $ra` (08 00 e0 03 LE) at word-aligned offsets -- function
#     epilogues are dense in real code, sparse in non-code data
#   * many `addiu $sp, $sp, -N` (... 27 bd ff XX LE) -- function prologues
#   * size in the plausible overlay range (32 KB .. 256 KB)
#
# Run from repo root:
#   python3 ghidra/scripts/find_overlay_candidates.py
#
# This is a host-side script, not an analyzeHeadless one -- no Ghidra
# dependency. Kept under ghidra/scripts/ because that's where the rest of
# the static-analysis tooling lives.

import math
import os
import struct
import sys
from collections import Counter

PROT_DIR = "extracted/PROT"

JR_RA_LE = bytes([0x08, 0x00, 0xE0, 0x03])    # jr $ra
SP_PROLOGUE_PREFIX = bytes([0xBD, 0x27])       # ... 27 bd XX XX -- addiu $sp, $sp, -imm
NOP_LE = bytes([0x00, 0x00, 0x00, 0x00])


def entropy(buf):
    if not buf:
        return 0.0
    counts = Counter(buf)
    n = float(len(buf))
    return -sum((c / n) * math.log2(c / n) for c in counts.values())


def count_word_aligned(buf, needle):
    """Count occurrences of `needle` (4 bytes) at word-aligned offsets."""
    n = 0
    for i in range(0, len(buf) - 3, 4):
        if buf[i:i + 4] == needle:
            n += 1
    return n


def count_sp_prologue(buf):
    """Count `addiu $sp, $sp, -N` (encoding: 27 bd ff XX, instruction `imm sp sp 001001`).
    LE bytes: XX YY BD 27 (where YY BD is split). Actually instruction = 0x27BD_FFXX,
    LE bytes = XX FF BD 27. Match the high two LE bytes (BD 27) at offset+2."""
    n = 0
    for i in range(0, len(buf) - 3, 4):
        if buf[i + 2:i + 4] == SP_PROLOGUE_PREFIX and buf[i + 1] == 0xFF:
            n += 1
    return n


def starts_like_function(buf):
    """Real overlay code usually starts with addiu $sp, $sp, -N or a jump.
    Returns 1 if first 4 bytes look like a plausible function start."""
    if len(buf) < 4:
        return 0
    word = struct.unpack("<I", buf[:4])[0]
    # addiu $sp, $sp, -imm
    if word & 0xFFFF_0000 == 0x27BD_0000 and (word & 0x8000):
        return 1
    # jal / j (top 6 bits = 000010 / 000011)
    if (word >> 26) in (0x02, 0x03):
        return 1
    return 0


def scan_one(path):
    with open(path, "rb") as f:
        buf = f.read()
    size = len(buf)
    if size < 4096:
        return None

    ent = entropy(buf)
    n_jr_ra = count_word_aligned(buf, JR_RA_LE)
    n_prologue = count_sp_prologue(buf)
    n_nop = count_word_aligned(buf, NOP_LE)
    head_ok = starts_like_function(buf)

    # Density per KB: > ~0.5 jr-ra / KB and > ~0.5 prologue / KB suggests code.
    kb = size / 1024.0
    jr_ra_density = n_jr_ra / kb
    prologue_density = n_prologue / kb

    # Score: weight code-pattern density, penalise high entropy.
    score = 0.0
    if 5.0 <= ent <= 6.8:
        score += 5.0 * (1.0 - abs(ent - 6.0) / 2.0)
    score += min(jr_ra_density, 5.0)
    score += min(prologue_density, 5.0)
    if 32 * 1024 <= size <= 256 * 1024:
        score += 2.0
    if head_ok:
        score += 1.0
    # Penalise pathological NOP runs (often padding / not real overlay).
    if n_nop > size // 16:
        score -= 2.0

    return {
        "path": path,
        "size": size,
        "entropy": ent,
        "jr_ra": n_jr_ra,
        "prologue": n_prologue,
        "nop": n_nop,
        "head_ok": head_ok,
        "jr_ra_density": jr_ra_density,
        "prologue_density": prologue_density,
        "score": score,
    }


def main():
    if not os.path.isdir(PROT_DIR):
        print(f"missing {PROT_DIR} -- run legaia-extract first", file=sys.stderr)
        sys.exit(1)

    rows = []
    for name in sorted(os.listdir(PROT_DIR)):
        if not name.endswith(".BIN"):
            continue
        r = scan_one(os.path.join(PROT_DIR, name))
        if r is not None:
            rows.append(r)

    rows.sort(key=lambda r: r["score"], reverse=True)
    top = rows[:25]

    print(f"{'idx':>4} {'size':>8} {'entropy':>7} {'jr_ra':>5} {'prol':>5} "
          f"{'jr/KB':>6} {'pr/KB':>6} {'hd':>2} {'score':>6}  path")
    for r in top:
        idx = os.path.basename(r["path"])[:4]
        print(f"{idx:>4} {r['size']:>8} {r['entropy']:>7.3f} "
              f"{r['jr_ra']:>5} {r['prologue']:>5} "
              f"{r['jr_ra_density']:>6.2f} {r['prologue_density']:>6.2f} "
              f"{r['head_ok']:>2} {r['score']:>6.2f}  "
              f"{os.path.basename(r['path'])}")


if __name__ == "__main__":
    main()
