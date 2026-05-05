# @category Legaia
# @runtime Jython
#
# Trace register state through FUN_80026b4c instruction-by-instruction
# so we can confirm whether the SW at 80026ba8 should be detected.

from ghidra.program.model.lang import Register

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()


def regs_written(insn):
    out = []
    for o in insn.getResultObjects():
        if isinstance(o, Register):
            out.append(o.getName())
    return out


func = fm.getFunctionAt(af.getAddress("80026b4c"))
print("Tracing {}".format(func.getName()))
for ins in listing.getInstructions(func.getBody(), True):
    rs = list(insn_iter for insn_iter in [ins.getDefaultOperandRepresentation(i) for i in range(ins.getNumOperands())])
    written = regs_written(ins)
    print("  {}  mnem={:6s}  ops={!r}  written={!r}".format(
        ins.getAddress(), ins.getMnemonicString(), rs, written
    ))
