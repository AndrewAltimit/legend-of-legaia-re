#!/usr/bin/env python3
"""
mips_gte.py - shared PSX GTE (COP2) instruction annotation.

Capstone's MIPS backend decodes a PSX GTE instruction only as a bare `cop2`
with the raw immediate; it doesn't name the GTE function. Several scripts that
disassemble PSX MIPS (the TMD renderer, world-map terrain emitter, battle/effect
overlays) need to read those ops, so this is the one place the GTE function
table lives. It is the public PSX GTE opcode encoding (PSX-SPX / nocash), not
any Sony-derived data.

## Encoding

A GTE "command" instruction is `COP2` with bit 25 set:

    bits 31..26 = 0x12 (COP2)
    bit  25     = 1     (GTE command, vs COP2 data move)
    bits 24..0  = cofun payload; the 6-bit function selector is bits 0..5,
                  with shift/lm/cv/v/mx fields in the higher bits.

`annotate_cop2(word)` returns the GTE mnemonic (e.g. `"rtps"`) for such a word,
or `""` for a non-GTE-command COP2 / non-COP2 word. A few full-word forms common
in TMD render loops are matched exactly first; everything else falls back to the
6-bit function field.
"""

from __future__ import annotations

# Exact full-cofun forms seen in real render loops (shift/flag bits included),
# matched before the 6-bit-function fallback.
GTE_OPS = {
    0x0180001: "rtps",
    0x0280030: "rtpt",
    0x0A00428: "sqr",
    0x170000C: "op",
    0x158002D: "avsz3",
    0x168002E: "avsz4",
    0x1400006: "nclip",
    0x190003D: "gpf",
    0x1A0003E: "gpl",
    0x1280030: "rtpt",
    0x108041E: "ncct",
    0x10C0008: "ncct",
    0x10C002D: "avsz3",
}

# 6-bit GTE function selector (cofun bits 0..5) -> mnemonic.
GTE_FUNC = {
    0x01: "rtps",
    0x06: "nclip",
    0x0C: "op",
    0x10: "dpcs",
    0x11: "intpl",
    0x12: "mvmva",
    0x13: "ncds",
    0x14: "cdp",
    0x16: "ncdt",
    0x1B: "nccs",
    0x1C: "cc",
    0x1E: "ncs",
    0x20: "nct",
    0x28: "sqr",
    0x29: "dcpl",
    0x2A: "dpct",
    0x2D: "avsz3",
    0x2E: "avsz4",
    0x30: "rtpt",
    0x3D: "gpf",
    0x3E: "gpl",
    0x3F: "ncct",
}


def annotate_cop2(raw: int) -> str:
    """GTE op mnemonic for a 32-bit COP2 instruction word, or "".

    `raw` is the full instruction word; the cofun payload is bits 0..24 (bit 25,
    the command flag, is masked off - real GTE commands set it, e.g. RTPS is the
    full word 0x4A180001 / cofun 0x0180001).
    """
    if (raw >> 26) != 0x12:
        return ""
    cofun = raw & 0x01FF_FFFF
    if cofun in GTE_OPS:
        return GTE_OPS[cofun]
    return GTE_FUNC.get(cofun & 0x3F, "")


if __name__ == "__main__":
    import sys

    # RTPS = full COP2 command word 0x4A180001 (cofun 0x0180001).
    assert annotate_cop2(0x4A180001) == "rtps", hex(0x4A180001)
    # mvmva via the 6-bit fallback (function 0x12).
    assert annotate_cop2((0x12 << 26) | (1 << 25) | 0x12) == "mvmva"
    # A non-COP2 word annotates to "".
    assert annotate_cop2(0x00000000) == ""
    print("mips_gte self-test OK", file=sys.stderr)
