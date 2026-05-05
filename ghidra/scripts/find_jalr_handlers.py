# @category Legaia
# @runtime Jython
#
# Find all jalr instructions whose target register was loaded via `lw R,
# +0x10(...)` somewhere just before. This is the signature of the mode
# dispatcher: load handler ptr from table[mode].handler, then jalr.
#
# Also flag any jalr immediately preceded by a multiply by 0x18 (the table
# stride). And: print the function context for each match.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

inst_iter = listing.getInstructions(True)

# Walk all instructions; for each jalr, look at the previous ~6 instructions
# in the same function for an `lw <reg>, ...(...)` whose offset is the +0x10
# we expect.

# Buffer last N instructions per function.
LOOKBACK = 8
buf = []
current_func = None

# Group jalrs by source function
hits = {}

count = 0
while inst_iter.hasNext():
    insn = inst_iter.next()
    count += 1
    f = fm.getFunctionContaining(insn.getAddress())
    if not f:
        buf = []
        continue
    if current_func != f.getEntryPoint().getOffset():
        buf = []
        current_func = f.getEntryPoint().getOffset()
    buf.append(insn)
    if len(buf) > LOOKBACK:
        buf.pop(0)
    mnem = insn.getMnemonicString()
    if mnem != "jalr":
        continue
    # Found jalr. Get target register.
    try:
        target_reg = insn.getDefaultOperandRepresentation(0 if insn.getNumOperands() == 1 else 1)
    except:
        target_reg = None
    # Look backwards for an `lw target_reg, OFFSET(BASE)`
    handler_load = None
    stride_mul = None
    table_lui = None
    for prev in reversed(buf[:-1]):
        pm = prev.getMnemonicString()
        if pm == "lw" and prev.getNumOperands() == 2:
            try:
                dst = prev.getDefaultOperandRepresentation(0)
                op2 = prev.getDefaultOperandRepresentation(1)
            except:
                continue
            if dst != target_reg:
                continue
            handler_load = (str(prev.getAddress()), op2)
            break
        # Detect "sll x,y,4" (stride 0x10) or "addiu/sub" patterns near
        if pm in ("mult",):
            stride_mul = (str(prev.getAddress()), prev.toString())
    if handler_load:
        # Check if the offset looks like +0x10 or +0x14 (handler / param offsets)
        op2 = handler_load[1]
        if "0x10" in op2 or "0x14" in op2:
            f_entry = str(f.getEntryPoint())
            f_name = f.getName()
            hits.setdefault((f_entry, f_name), []).append({
                "jalr_at": str(insn.getAddress()),
                "handler_load": handler_load,
                "stride_mul": stride_mul,
            })

print("=== jalr instructions whose target was loaded from offset +0x10 (handler ptr) ===")
for (fe, fn), items in sorted(hits.items()):
    print("  {} @ {}  ({} hit(s))".format(fn, fe, len(items)))
    for h in items[:5]:
        print("    jalr {}   load {}: {}   stride_mul {}".format(
            h["jalr_at"], h["handler_load"][0], h["handler_load"][1], h["stride_mul"]))

print("\nscanned {} insns".format(count))
print("done")
