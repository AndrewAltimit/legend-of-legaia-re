# @category Legaia
# @runtime Jython
#
# Find functions that read from the TMD pointer table at 0x8007C018 + idx*4.
#
# FUN_80026b4c writes registered TMDs to *(0x8007C018 + DAT_8007b774 * 4).
# This script looks for indexed reads of that table.
#
# IMPORTANT: a previous version of this script had a register-tracking bug --
# it didn't invalidate per-register knowledge when a non-LUI/ADDIU instruction
# wrote to the register. That gave false positives where `lui v0,0x8008; lw
# v0,-0x72b4(v0); lw a0,0x18(v0)` was misread as "lw a0 at 0x8007C018" when
# actually the second lw had loaded a totally different pointer into v0
# (PTR_PTR_80078d4c, the PSX GPU library function table).
#
# Now we use Ghidra's `getResultObjects()` to detect any write to a register
# and drop our tracked LUI/ADDIU value when that happens.

from ghidra.program.model.lang import Register

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

TABLE_ADDR = 0x8007C018  # exact start of the TMD pointer table
# (recovered from FUN_80026b4c: lui 0x8008 + addiu -0x3FE8 = 0x8007C018,
# NOT 0x80080018 as previous notes claimed)


def parse_load_offset(operand):
    if "(" not in operand or not operand.endswith(")"):
        return None, None
    offstr, basestr = operand.split("(")
    basestr = basestr.rstrip(")")
    try:
        if offstr.startswith("-0x"):
            return -int(offstr[3:], 16), basestr
        if offstr.startswith("0x"):
            return int(offstr[2:], 16), basestr
        return int(offstr, 0), basestr
    except ValueError:
        return None, None


def regs_written(insn):
    out = set()
    for o in insn.getResultObjects():
        if isinstance(o, Register):
            out.add(o.getName())
    return out


# Per-function pass to keep tracking honest.
table_hits = {}  # fa -> [(insn_addr, kind, eff_addr_str)]
gp820_hits = {}  # fa -> [insn_addr]

for func in fm.getFunctions(True):
    fa = func.getEntryPoint().getOffset()
    body = func.getBody()
    insns = listing.getInstructions(body, True)
    lui = {}  # reg -> imm<<16  (LUI just happened, no addiu yet)
    full = {}  # reg -> combined u32  (LUI followed by ADDIU)
    sll2 = set()  # regs that just got sll'd by 2
    addu_base = {}  # reg -> base u32 (regs holding base + (idx<<2))
    for insn in insns:
        mnem = insn.getMnemonicString()
        # Ghidra prefixes delay-slot instructions with `_`. Normalize so we
        # treat `sw` and `_sw` identically.
        if mnem.startswith("_"):
            mnem = mnem[1:]
        # gp+0x820 read?
        if mnem in ("lw", "lhu", "lh", "lbu", "lb"):
            try:
                op = insn.getDefaultOperandRepresentation(1)
                off, base = parse_load_offset(op)
                if base == "gp" and off == 0x820:
                    gp820_hits.setdefault(fa, []).append(str(insn.getAddress()))
            except Exception:
                pass

        # Detect TABLE reads/writes BEFORE we update register state with this insn.
        if mnem in ("lw", "lhu", "sw", "sh", "sb", "lh", "lb", "lbu"):
            try:
                op = insn.getDefaultOperandRepresentation(1)
                off, base_reg = parse_load_offset(op)
                if off is not None and base_reg is not None:
                    is_write = mnem.startswith("s")
                    tag = "WRITE" if is_write else "READ "
                    # Pattern A: indexed via addu_base
                    if base_reg in addu_base:
                        eff = (addu_base[base_reg] + off) & 0xFFFFFFFF
                        if 0x8007C018 <= eff <= 0x8007C018 + 0x800:
                            table_hits.setdefault(fa, []).append(
                                (str(insn.getAddress()), tag + " indexed", "0x{:08X}".format(eff))
                            )
                    # Pattern B: full address direct
                    if base_reg in full:
                        eff = (full[base_reg] + off) & 0xFFFFFFFF
                        if eff == TABLE_ADDR:
                            table_hits.setdefault(fa, []).append(
                                (str(insn.getAddress()), tag + " direct", "0x{:08X}".format(eff))
                            )
                    # Pattern C: bare LUI base + offset == TABLE_ADDR
                    if base_reg in lui:
                        eff = (lui[base_reg] + off) & 0xFFFFFFFF
                        if eff == TABLE_ADDR:
                            table_hits.setdefault(fa, []).append(
                                (str(insn.getAddress()), tag + " lui+off", "0x{:08X}".format(eff))
                            )
            except Exception:
                pass

        # Compute the new state BEFORE invalidating, since addiu/addu often
        # read the same register they write to.
        new_lui = None  # (reg, value)
        new_full = None  # (reg, value)
        new_sll2 = None  # reg
        new_addu_base = None  # (reg, value)
        if mnem == "lui":
            try:
                reg = insn.getDefaultOperandRepresentation(0)
                imm = insn.getOpObjects(1)[0].getValue() & 0xFFFF
                new_lui = (reg, imm << 16)
            except Exception:
                pass
        elif mnem == "addiu":
            try:
                dst = insn.getDefaultOperandRepresentation(0)
                src = insn.getDefaultOperandRepresentation(1)
                imm = insn.getOpObjects(2)[0].getValue()
                # imm may come back as a Java Long; treat as signed 16-bit.
                if imm >= 0x8000:
                    imm = imm - 0x10000
                if src in lui:
                    new_full = (dst, (lui[src] + imm) & 0xFFFFFFFF)
                elif src in full:
                    new_full = (dst, (full[src] + imm) & 0xFFFFFFFF)
            except Exception:
                pass
        elif mnem == "sll":
            try:
                dst = insn.getDefaultOperandRepresentation(0)
                shift = insn.getOpObjects(2)[0].getValue()
                if shift == 2:
                    new_sll2 = dst
            except Exception:
                pass
        elif mnem == "addu":
            try:
                dst = insn.getDefaultOperandRepresentation(0)
                s = insn.getDefaultOperandRepresentation(1)
                t = insn.getDefaultOperandRepresentation(2)
                base = None
                for src_base, src_idx in ((s, t), (t, s)):
                    if src_idx in sll2:
                        if src_base in full:
                            base = full[src_base]
                            break
                        if src_base in lui:
                            base = lui[src_base]
                            break
                if base is not None:
                    new_addu_base = (dst, base)
            except Exception:
                pass

        # Now invalidate any register this instruction writes.
        written = regs_written(insn)
        for r in written:
            lui.pop(r, None)
            full.pop(r, None)
            sll2.discard(r)
            addu_base.pop(r, None)

        # Apply the new tracked state we computed (overrides invalidation).
        if new_lui is not None:
            lui[new_lui[0]] = new_lui[1]
        if new_full is not None:
            full[new_full[0]] = new_full[1]
        if new_sll2 is not None:
            sll2.add(new_sll2)
        if new_addu_base is not None:
            addu_base[new_addu_base[0]] = new_addu_base[1]


print("=== TABLE READERS (0x8007C018..0x8007C818) ===")
ranked = sorted(table_hits.items(), key=lambda kv: (-len(kv[1]),))
print("found {} functions touching the range".format(len(ranked)))
for fa, hits in ranked[:30]:
    func = fm.getFunctionAt(af.getAddress("{:x}".format(fa)))
    fname = func.getName() if func else "?"
    print("\n  {} @ 0x{:08X}  hits={}".format(fname, fa, len(hits)))
    for ia, kind, tgt in hits[:10]:
        print("    {}  {}  {}".format(ia, kind, tgt))

print("\n=== gp[0x820] READERS (TMD-count consumers) ===")
print("found {} functions reading gp[0x820]".format(len(gp820_hits)))
ranked = sorted(gp820_hits.items(), key=lambda kv: (-len(kv[1]),))
for fa, hits in ranked[:30]:
    func = fm.getFunctionAt(af.getAddress("{:x}".format(fa)))
    fname = func.getName() if func else "?"
    print("  {} @ 0x{:08X}  reads={}  ({})".format(fname, fa, len(hits), hits[0]))
