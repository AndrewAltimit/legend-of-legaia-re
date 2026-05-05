# @category Legaia
# @runtime Jython
#
# Sweep SCUS for LUI+ADDIU pairs that combine to land in the sound-driver
# string cluster (0x8007B38C..0x8007B3C8). Reports each instruction whose
# computed address falls in that range, plus the function it belongs to.
#
# This unblocks PRD section 4.1 row "Sound-driver output formats" -- we need
# to find the path-builder consumers before we can match extension to PROT
# entry.
#
# Run with:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process SCUS_942.54 -noanalysis \
#       -postScript /scripts/find_sound_path_builders.py

import os

LO = 0x8007B380
HI = 0x8007B3D0

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()

# Track per-register LUI immediates as we walk instructions linearly.
hits = []

instrs = list(listing.getInstructions(prog.getMemory(), True))
print("scanning {} instructions for refs into 0x{:08X}..0x{:08X}".format(
    len(instrs), LO, HI))

regs = {}  # reg name -> upper16 value

for ins in instrs:
    mnem = ins.getMnemonicString()
    # Detect LUI loading the upper 16 bits of one of our target addrs.
    if mnem == "lui":
        rt = ins.getRegister(0)
        imm = ins.getScalar(1)
        if rt is not None and imm is not None:
            upper = (imm.getUnsignedValue() & 0xFFFF) << 16
            regs[rt.getName()] = upper
        continue
    # ADDIU + LBU / LB / LW / LH (load constants and load/store).
    if mnem in ("addiu", "addi", "ori", "lbu", "lb", "lh", "lhu", "lw", "sb", "sh", "sw"):
        # Operand 0 is dst register, operand-1 is base register (or src for lui),
        # operand-2 is the 16-bit signed immediate.
        # Format depends on instruction; we ask Ghidra for the operand objects.
        n_ops = ins.getNumOperands()
        # For ADDIU rt, rs, imm: rs = ins.getRegister(1); imm = ins.getScalar(2)
        # For LBU rt, imm(rs):  rs = ins.getRegister(1); imm = ins.getScalar(1)
        # We try both shapes.
        rs = None
        imm = None
        if n_ops >= 3:
            rs = ins.getRegister(1)
            imm = ins.getScalar(2)
        if rs is None or imm is None:
            rs = ins.getRegister(1)
            imm = ins.getScalar(1)
        if rs is None or imm is None:
            continue
        rs_name = rs.getName()
        if rs_name not in regs:
            continue
        addr = regs[rs_name] + (imm.getValue() & 0xFFFFFFFF)
        # Ghidra MIPS scalar is signed 16-bit; sign-extend properly.
        if (imm.getValue() & 0x8000):
            addr = (regs[rs_name] + imm.getValue()) & 0xFFFFFFFF
        if LO <= addr <= HI:
            func = fm.getFunctionContaining(ins.getAddress())
            fname = func.getName() if func else "(no-func)"
            fentry = func.getEntryPoint() if func else "?"
            hits.append((str(ins.getAddress()), fname, fentry, mnem, addr))

print("found {} hits:".format(len(hits)))
for ins_addr, fname, fentry, mnem, addr in hits:
    print("  {}  {} @ {}  {} -> 0x{:08X}".format(
        ins_addr, fname, fentry, mnem, addr))

OUT = "/scripts/sound_path_builders.csv"
with open(OUT, "w") as fh:
    fh.write("ins_addr,func_name,func_entry,mnemonic,target\n")
    for ins_addr, fname, fentry, mnem, addr in hits:
        fh.write("{},{},{},{},0x{:08X}\n".format(
            ins_addr, fname, fentry, mnem, addr))
print("wrote {}".format(OUT))
