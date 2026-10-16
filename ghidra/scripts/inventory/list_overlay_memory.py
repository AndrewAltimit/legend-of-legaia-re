# @category Legaia
# @runtime Jython
# Lists all memory blocks in the current program to understand address layout.

prog = currentProgram
mem = prog.getMemory()
print("== Memory blocks for {} ==".format(prog.getName()))
for block in mem.getBlocks():
    print("  {:20s}  start=0x{:08x}  end=0x{:08x}  size=0x{:06x}  init={}  exec={}".format(
        block.getName(),
        block.getStart().getOffset(),
        block.getEnd().getOffset(),
        block.getSize(),
        block.isInitialized(),
        block.isExecute()))
print("done")
