# @category Legaia
# @runtime Jython
#
# Look for the specific `lui+addiu` pair that materializes 0x8007C018
# (or 0x8007BB38) into a register but DOES NOT immediately load from it -
# i.e., the address is being passed as a function argument or used as a
# store base. These are the prime candidates for the install-time writer
# that Ghidra's reference manager misses (because the actual sw happens
# inside a helper that receives the address via $a0/$a1).

prog = currentProgram
prog_name = prog.getName()
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

TARGETS = [0x8007C018, 0x8007BB38, 0x8007B7DC]

inst_iter = listing.getInstructions(True)
last_lui = {}
current_func = None
out = []

# State machine: when an addiu produces our target, look ahead a few
# instructions for a `jal`, `sw`, or `lw`. Report category.
while inst_iter.hasNext():
    ins = inst_iter.next()
    func = fm.getFunctionContaining(ins.getAddress())
    fa = func.getEntryPoint().getOffset() if func else None
    if fa != current_func:
        last_lui = {}
        current_func = fa
    mnem = ins.getMnemonicString()
    if mnem == "lui" and ins.getNumOperands() == 2:
        try:
            reg = ins.getDefaultOperandRepresentation(0)
            imm = ins.getOpObjects(1)[0].getValue() & 0xFFFF
            last_lui[reg] = imm << 16
        except:
            pass
        continue
    if mnem == "addiu" and ins.getNumOperands() == 3:
        try:
            dst = ins.getDefaultOperandRepresentation(0)
            src = ins.getDefaultOperandRepresentation(1)
            imm = ins.getOpObjects(2)[0].getValue()
            if src in last_lui:
                base = last_lui[src]
                combined = (base + imm) & 0xFFFFFFFF
                if combined in TARGETS:
                    # Look at the next few instructions to classify usage.
                    fname = func.getName() if func else "?"
                    look_ahead = []
                    nxt = ins.getAddress().add(4)
                    for _ in range(6):
                        nins = listing.getInstructionAt(nxt)
                        if nins is None:
                            break
                        look_ahead.append("%s  %s" % (nxt, nins.toString()))
                        nxt = nxt.add(4)
                    out.append((str(ins.getAddress()), "0x%08X" % combined,
                                fname, dst, look_ahead))
                last_lui[dst] = combined
        except:
            pass
        continue
    # Handle other instructions that may def the register; clear last_lui.
    if mnem in ("lw", "lhu", "lh", "lbu", "lb", "li", "move", "or", "and",
                "subu", "addu", "andi", "ori", "xori", "sll", "srl", "sra",
                "mflo", "mfhi", "lwc1", "lwl", "lwr"):
        try:
            if ins.getNumOperands() >= 1:
                d = ins.getDefaultOperandRepresentation(0)
                if d in last_lui:
                    del last_lui[d]
        except:
            pass

print("=== %s : %d materializations ===" % (prog_name, len(out)))
for site, tgt, fname, reg, ahead in out:
    print("\n%s  in %s  (reg %s = %s)" % (site, fname, reg, tgt))
    for line in ahead:
        print("    %s" % line)
