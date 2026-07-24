#!/usr/bin/env python3
"""Audit capture-class per-spell modules (PROT 944..966) for stores to the
battle-actor HP triple: +0x14C live HP, +0x172 displayed HP, +0x10 pending
accumulator (and the +0x00 per-action total).

For each store site, classify:
  ACCUM  - a load of the SAME (base_reg, offset) feeds the stored register
           within a small window before the store (read-modify-write)
  ASSIGN - no such load: the old value is never read

The modules are raw uncompressed MIPS (the disc-patcher does same-size word
writes into them), position-independent for this purpose: store immediates
don't depend on the load base.
"""
import sys
from pathlib import Path
from capstone import Cs, CS_ARCH_MIPS, CS_MODE_MIPS32, CS_MODE_LITTLE_ENDIAN

OFFSETS = {0x14C: "live_hp", 0x172: "disp_hp", 0x10: "acc", 0x00: "dmg_total"}
STORES = {"sh", "sw"}
LOADS = {"lh", "lhu", "lw"}
WINDOW = 24  # instructions to scan backwards for the pairing load

md = Cs(CS_ARCH_MIPS, CS_MODE_MIPS32 | CS_MODE_LITTLE_ENDIAN)
md.detail = False


def parse_mem(op_str):
    # "$v0, 0x14c($v1)" -> (reg_stored, off, base)
    try:
        val, mem = [s.strip() for s in op_str.split(",", 1)]
        off_s, base = mem.split("(")
        base = base.rstrip(")")
        off = int(off_s, 0)
        return val, off, base
    except Exception:
        return None, None, None


def audit(path):
    data = Path(path).read_bytes()
    # decode the whole file as code; data words decode as junk we filter by
    # only reporting actor-offset stores whose base register is also used as
    # a base by other actor-field accesses nearby (heuristic: report all,
    # human-review the neighborhood dump).
    insns = []
    for i in md.disasm(data, 0):
        insns.append(i)
    # capstone stops at undecodable words; walk in chunks
    if len(insns) * 4 < len(data):
        insns = []
        pos = 0
        while pos + 4 <= len(data):
            got = list(md.disasm(data[pos:pos + 4], pos))
            insns.append(got[0] if got else None)
            pos += 4

    hits = []
    for idx, ins in enumerate(insns):
        if ins is None or ins.mnemonic not in STORES:
            continue
        val, off, base = parse_mem(ins.op_str)
        if off not in OFFSETS or base in ("$sp", "$gp", "$k0", "$k1", "$zero"):
            continue
        # 0x00 offset stores are overwhelmingly noise; require the same base
        # register to also touch another actor offset within the window.
        kind = "ASSIGN"
        for j in range(max(0, idx - WINDOW), idx):
            p = insns[j]
            if p is None or p.mnemonic not in LOADS:
                continue
            lval, loff, lbase = parse_mem(p.op_str)
            if loff == off and lbase == base:
                kind = "ACCUM(load seen)"
                break
        if off == 0x00 and kind == "ASSIGN":
            near = False
            for j in range(max(0, idx - 8), min(len(insns), idx + 8)):
                p = insns[j]
                if p is None:
                    continue
                if p.mnemonic in STORES | LOADS:
                    _, o2, b2 = parse_mem(p.op_str)
                    if b2 == base and o2 in (0x10, 0x14C, 0x172, 0x14E):
                        near = True
                        break
            if not near:
                continue
        hits.append((ins.address, ins.mnemonic, ins.op_str, OFFSETS[off], kind))
    return hits


for arg in sys.argv[1:]:
    hits = audit(arg)
    name = Path(arg).name
    if not hits:
        print(f"{name}: no actor-HP-triple stores")
        continue
    print(f"{name}:")
    for addr, mn, ops, field, kind in hits:
        print(f"  +0x{addr:05X}  {mn} {ops:<28} {field:<9} {kind}")
