# @category Legaia
# @runtime Jython
# Probe what's at a list of SCUS addresses to figure out why
# they aren't appearing as functions.

prog = currentProgram
listing = prog.getListing()
fm = prog.getFunctionManager()
af = prog.getAddressFactory()
mem = prog.getMemory()

PROBE_ADDRS = [
    "80019788", "80035334", "80035394", "800357fc", "800358c0",
    "80035978", "80035c10", "800566a8", "800566b8", "800566c8",
    "800566d8", "800566e8", "800566f8", "80056708", "80056718",
    "80056698", "8006ee14", "8006ee24", "8006ee34", "8003c43c",
    "8003c510",
]

for hs in PROBE_ADDRS:
    a = af.getAddress(hs)
    print("--- 0x{} ---".format(hs))
    if not mem.contains(a):
        print("  not in any block")
        continue
    blk = mem.getBlock(a)
    print("  block: {} ({}-{}) perm={}".format(
        blk.getName(), blk.getStart(), blk.getEnd(),
        "R" if blk.isRead() else "" + ("W" if blk.isWrite() else "") + ("X" if blk.isExecute() else "")))
    f = fm.getFunctionAt(a)
    if f:
        print("  IS function entry: {}".format(f.getName()))
    f = fm.getFunctionContaining(a)
    if f:
        print("  is INSIDE function: {} (entry {})".format(f.getName(), f.getEntryPoint()))
    ins = listing.getInstructionAt(a)
    if ins:
        print("  instruction: {}".format(ins))
    dat = listing.getDataAt(a)
    if dat:
        print("  data: {} (size {} bytes)".format(dat, dat.getLength()))
    # Read 16 bytes
    try:
        bs = bytes(mem.getBytes(a, 16))
        print("  bytes: {}".format(" ".join("%02x" % (b & 0xFF) for b in bs)))
    except Exception as e:
        print("  read failed: {}".format(e))
print("done")
