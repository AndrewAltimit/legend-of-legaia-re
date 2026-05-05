# @category Legaia
# @runtime Jython
#
# Hunt for the mode dispatcher: anything that calls a per-mode handler indirectly
# from the table at 0x8007078C, or reads handler_ptr (offset +0x10) from any
# entry in that table. Also: list all callers of FUN_80025eec (the default
# handler shared by 13 modes).

prog = currentProgram
af = prog.getAddressFactory()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()
fm = prog.getFunctionManager()
mem = prog.getMemory()

TABLE_BASE = 0x8007078C
ENTRY_SIZE = 0x18
N = 28

# Step 1: Print direct callers of FUN_80025eec
print("=== Callers of FUN_80025eec (default per-mode handler) ===")
addr = af.getAddress("80025eec")
refs = list(ref_mgr.getReferencesTo(addr))
by_func = {}
for r in refs:
    fa = r.getFromAddress()
    rt = r.getReferenceType()
    f = fm.getFunctionContaining(fa)
    fn = f.getName() if f else "?"
    fe = str(f.getEntryPoint()) if f else "?"
    by_func.setdefault((fe, fn), []).append((str(fa), str(rt)))
for (fe, fn), items in sorted(by_func.items()):
    print("  {} @ {}".format(fn, fe))
    for ia, rt in items[:5]:
        print("    {}  {}".format(ia, rt))

# Step 2: Find all callers of *any* function whose address appears as a handler
# in the mode table.
handler_set = set()
handler_names = {}
for i in range(N):
    a = af.getAddress("{:x}".format(TABLE_BASE + i * ENTRY_SIZE))
    bs = bytearray(ENTRY_SIZE)
    for j in range(ENTRY_SIZE):
        bs[j] = mem.getByte(a.add(j)) & 0xFF
    handler_ptr = bs[0x10] | (bs[0x11] << 8) | (bs[0x12] << 16) | (bs[0x13] << 24)
    handler_set.add(handler_ptr)
    f = fm.getFunctionAt(af.getAddress("{:x}".format(handler_ptr)))
    handler_names[handler_ptr] = f.getName() if f else "?"

print("\n=== unique handler addresses in mode table ===")
for h in sorted(handler_set):
    print("  0x{:08X}  {}".format(h, handler_names[h]))

# Step 3: For each unique handler, find unique calling-function set;
# the intersection (functions that call MANY of these handlers) is the dispatcher.
print("\n=== caller sets per handler ===")
caller_to_handlers = {}  # caller_func_entry -> set of handler addrs they call
for h in sorted(handler_set):
    h_addr = af.getAddress("{:x}".format(h))
    refs = list(ref_mgr.getReferencesTo(h_addr))
    caller_funcs = set()
    for r in refs:
        rt = str(r.getReferenceType())
        if "CALL" not in rt and "DATA" not in rt and "READ" not in rt:
            continue
        fa = r.getFromAddress()
        f = fm.getFunctionContaining(fa)
        if not f:
            continue
        caller_funcs.add(str(f.getEntryPoint()))
    print("  0x{:08X} ({}): {} caller funcs: {}".format(
        h, handler_names[h], len(caller_funcs), sorted(caller_funcs)[:5]))
    for cf in caller_funcs:
        caller_to_handlers.setdefault(cf, set()).add(h)

print("\n=== functions that call multiple handlers (dispatcher candidates) ===")
for cf, hs in sorted(caller_to_handlers.items(), key=lambda kv: -len(kv[1])):
    if len(hs) >= 2:
        f = fm.getFunctionAt(af.getAddress(cf))
        fn = f.getName() if f else "?"
        print("  {} @ {}  calls {} handler(s)".format(fn, cf, len(hs)))

# Step 4: Find anyone who reads from an address that looks like
# "TABLE_BASE + (mode * 0x18) + 0x10"; this is the dispatcher signature.
# Easier: scan for `lw ?, 0x10(...)` immediately after a multiply by 0x18.
print("\n=== anyone referencing 0x{:08X} (mode table base) ===".format(TABLE_BASE))
tab_addr = af.getAddress("{:x}".format(TABLE_BASE))
refs = list(ref_mgr.getReferencesTo(tab_addr))
for r in refs:
    fa = r.getFromAddress()
    rt = r.getReferenceType()
    f = fm.getFunctionContaining(fa)
    fn = f.getName() if f else "?"
    fe = str(f.getEntryPoint()) if f else "?"
    print("  {}  {}  in {} @ {}".format(fa, rt, fn, fe))

print("\ndone")
