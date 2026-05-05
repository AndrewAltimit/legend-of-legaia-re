# @category Legaia
# @runtime Jython
# Quick inventory: every function known in the overlay program.
prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()

funcs = list(fm.getFunctions(True))
print("total functions: {}".format(len(funcs)))
# Sort by entry, show first 40 + the function at 0x801dd35c specifically
addrs = sorted(f.getEntryPoint().getOffset() for f in funcs)
print("first 20 entries:")
for a in addrs[:20]:
    print("  0x{:08X}".format(a))
print("entries around 0x801dd35c:")
for a in addrs:
    if 0x801dd000 <= a <= 0x801ddfff:
        print("  0x{:08X}".format(a))

print("\nis 0x801dd35c a function? {}".format(
    fm.getFunctionAt(prog.getAddressFactory().getAddress("801dd35c")) is not None))
