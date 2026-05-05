# @category Legaia
# @runtime Jython
#
# Find every store (sw/sh/sb) into the scene-name buffer at 0x80084548
# (read by FUN_8001f7c0 and FUN_800255b8). Walks all instructions, tracks
# LUI+ADDIU pairs per register so it can detect the combined 32-bit
# address even when Ghidra's reference manager misses the LUI/ADDIU pair.
#
# Output: each writer with the storing instruction + 8 instructions of
# context, grouped by enclosing function.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

TARGET_ADDR = 0x80084548
WIDTHS = ("sw", "sh", "sb", "swl", "swr")  # all MIPS store-to-memory ops

# Track LUI immediates per dest register; cleared on any other write to that reg.
lui = {}  # reg_name -> u32 (zero-extended << 16)


def reg_name(op):
    if op is None:
        return None
    s = str(op)
    return s if s.startswith("$") or s.startswith("a") or s.startswith("s") or s.startswith("t") or s.startswith("v") or s.startswith("k") or s.startswith("at") or s.startswith("gp") or s.startswith("sp") or s.startswith("fp") or s.startswith("ra") or s.startswith("zero") else None


def context_around(addr, n_before=8):
    out = []
    cursor = listing.getInstructionBefore(addr)
    while cursor is not None and len(out) < n_before:
        out.append(cursor)
        cursor = listing.getInstructionBefore(cursor.getAddress())
    out.reverse()
    out.append(listing.getInstructionAt(addr))
    return out


def fmt(ins):
    return "{}  {}".format(ins.getAddress(), ins.toString())


hits_by_func = {}

it = listing.getInstructions(True)
while it.hasNext():
    ins = it.next()
    mnem = ins.getMnemonicString().lower()

    # Track LUI dest <- imm<<16
    if mnem == "lui":
        ops = ins.getOpObjects(0)
        # op 0 = dest reg, op 1 = imm
        try:
            dst = str(ins.getRegister(0))
            imm = ins.getOpObjects(1)[0].getValue() & 0xFFFF
            lui[dst] = (imm << 16) & 0xFFFFFFFF
        except Exception:
            pass
        continue

    # ADDIU rt, rs, imm: if rs has a known LUI base, propagate sum to rt
    if mnem == "addiu":
        try:
            rt = str(ins.getRegister(0))
            rs = str(ins.getRegister(1))
            imm = ins.getOpObjects(2)[0].getValue()
            # sign-extend 16-bit
            if imm & 0x8000:
                imm -= 0x10000
            if rs in lui:
                lui[rt] = (lui[rs] + imm) & 0xFFFFFFFF
            elif rt in lui:
                # Pure rt = rt + imm without rs base - invalidate.
                del lui[rt]
        except Exception:
            pass
        continue

    # ORI rt, rs, imm: similar combine
    if mnem == "ori":
        try:
            rt = str(ins.getRegister(0))
            rs = str(ins.getRegister(1))
            imm = ins.getOpObjects(2)[0].getValue() & 0xFFFF
            if rs in lui:
                lui[rt] = (lui[rs] | imm) & 0xFFFFFFFF
        except Exception:
            pass
        continue

    # MOVE rt, rs (=> addu rt, rs, $zero): copy lui state
    if mnem == "move":
        try:
            rt = str(ins.getRegister(0))
            rs = str(ins.getRegister(1))
            if rs in lui:
                lui[rt] = lui[rs]
            elif rt in lui:
                del lui[rt]
        except Exception:
            pass
        continue

    # Store: sw/sh/sb rt, imm(rs)
    if mnem in WIDTHS:
        try:
            # operand 1 is the memory operand: imm and base reg
            ops = ins.getOpObjects(1)
            imm = 0
            base = None
            for op in ops:
                if hasattr(op, "getValue"):
                    imm = op.getValue()
                    if imm & 0x8000:
                        imm -= 0x10000
                else:
                    base = str(op)
            if base in lui:
                addr = (lui[base] + imm) & 0xFFFFFFFF
                if addr == TARGET_ADDR:
                    func = fm.getFunctionContaining(ins.getAddress())
                    fentry = func.getEntryPoint().getOffset() if func else 0
                    fname = func.getName() if func else "(none)"
                    hits_by_func.setdefault((fentry, fname), []).append(ins.getAddress())
        except Exception:
            pass

print("== writers to 0x{:08X} (scene-name buffer) ==".format(TARGET_ADDR))
print("hits across {} function(s)".format(len(hits_by_func)))
for (fentry, fname), sites in sorted(hits_by_func.items()):
    print("\n  {} @ 0x{:08X}  ({} writes)".format(fname, fentry, len(sites)))
    for s in sites:
        print("    site: {}".format(s))
        for i in context_around(s, 6):
            print("      {}".format(fmt(i)))
