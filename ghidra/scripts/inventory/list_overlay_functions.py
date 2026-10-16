# @category Legaia
# @runtime Jython
#
# List all functions Ghidra found in the overlay program, sorted by size
# (largest first). Also reports how many functions each one calls (jal-target
# count) and how many call it (incoming refs). Used to spot dispatcher-shaped
# functions: those with high-fan-in OR many internal call sites are likely
# infrastructure (script VM, mode handlers, etc.).

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
rm = prog.getReferenceManager()

rows = []
for f in fm.getFunctions(True):
    body = f.getBody()
    size = body.getNumAddresses()
    addr = f.getEntryPoint()
    # Count outgoing calls (jal/jalr) by walking instructions and looking
    # for FlowType.CALL.
    n_call_out = 0
    for ins in listing.getInstructions(body, True):
        ftype = ins.getFlowType()
        if ftype.isCall():
            n_call_out += 1
    # Count callers: incoming references with reference type CALL.
    n_callers = 0
    for ref in rm.getReferencesTo(addr):
        if ref.getReferenceType().isCall():
            n_callers += 1
    rows.append((addr.getOffset(), size, n_call_out, n_callers, f.getName()))

print("found {} functions".format(len(rows)))

# Sort by size descending.
rows.sort(key=lambda r: -r[1])
print("\n=== TOP 30 BY SIZE ===")
print("  addr        size  out  in   name")
for addr, size, out, inn, name in rows[:30]:
    print("  0x{:08X}  {:>4}  {:>3}  {:>3}  {}".format(addr, size, out, inn, name))

# Sort by outgoing-call count (dispatcher-shaped functions).
rows.sort(key=lambda r: -r[2])
print("\n=== TOP 20 BY OUTGOING CALLS (dispatcher candidates) ===")
print("  addr        size  out  in   name")
for addr, size, out, inn, name in rows[:20]:
    print("  0x{:08X}  {:>4}  {:>3}  {:>3}  {}".format(addr, size, out, inn, name))

# Sort by incoming refs (most-called).
rows.sort(key=lambda r: -r[3])
print("\n=== TOP 20 BY INCOMING REFS (hot-target functions) ===")
print("  addr        size  out  in   name")
for addr, size, out, inn, name in rows[:20]:
    print("  0x{:08X}  {:>4}  {:>3}  {:>3}  {}".format(addr, size, out, inn, name))
