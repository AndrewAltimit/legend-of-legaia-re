# @category Legaia
# @runtime Jython
# Find all callers of 0x80023070 (the move-table opcode interpreter)
TARGET = 0x80023070

prog = currentProgram
prog_name = prog.getName()
listing = prog.getListing()
fm = prog.getFunctionManager()

def find_jals():
    hits = []
    for ins in listing.getInstructions(True):
        if ins.getMnemonicString() != "jal":
            continue
        ops = ins.getDefaultOperandRepresentation(0).lower()
        if "80023070" in ops:
            f = fm.getFunctionContaining(ins.getAddress())
            hits.append((str(ins.getAddress()), f.getName() if f else "(none)"))
    return hits

hits = find_jals()
print("[{}] {} jal callers:".format(prog_name, len(hits)))
for a, n in hits:
    print("  {} {}".format(a, n))
