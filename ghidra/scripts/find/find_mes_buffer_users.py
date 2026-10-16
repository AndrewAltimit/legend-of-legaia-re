# @category Legaia
# @runtime Jython
#
# Find every load (lw/lh/lb/lhu/lbu) and store (sw/sh/sb) that touches the
# MES buffer pointer at _DAT_8007b8a8 (read by anyone who consumes the
# decoded dialog blob loaded by FUN_8001f05c case 4).
#
# Tracks LUI+ADDIU/ORI register pairs so it catches the combined 32-bit
# address even when Ghidra's reference manager misses the LUI/ADDIU pair.
#
# Output: each function that touches the buffer with op + context.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

TARGET_ADDR = 0x8007b8a8
LOAD_OPS = ("lw", "lh", "lb", "lhu", "lbu", "lwl", "lwr")
STORE_OPS = ("sw", "sh", "sb", "swl", "swr")

lui = {}  # reg_name -> u32 (zero-extended << 16)


def context_around(addr, n_before=4):
    out = []
    cursor = listing.getInstructionBefore(addr)
    while cursor is not None and len(out) < n_before:
        out.append(cursor)
        cursor = listing.getInstructionBefore(cursor.getAddress())
    out.reverse()
    out.append(listing.getInstructionAt(addr))
    return out


def fmt(ins):
    return "{}  {}".format(ins.getAddress(), ins.toString())


hits_load = {}
hits_store = {}

it = listing.getInstructions(True)
while it.hasNext():
    ins = it.next()
    mnem = ins.getMnemonicString().lower()

    if mnem == "lui":
        try:
            dst = str(ins.getRegister(0))
            imm = ins.getOpObjects(1)[0].getValue() & 0xFFFF
            lui[dst] = (imm << 16) & 0xFFFFFFFF
        except Exception:
            pass
        continue

    if mnem == "addiu":
        try:
            rt = str(ins.getRegister(0))
            rs = str(ins.getRegister(1))
            imm = ins.getOpObjects(2)[0].getValue()
            if imm & 0x8000:
                imm -= 0x10000
            if rs in lui:
                lui[rt] = (lui[rs] + imm) & 0xFFFFFFFF
            elif rt in lui:
                del lui[rt]
        except Exception:
            pass
        continue

    if mnem == "ori":
        try:
            rt = str(ins.getRegister(0))
            rs = str(ins.getRegister(1))
            imm = ins.getOpObjects(2)[0].getValue() & 0xFFFF
            if rs in lui:
                lui[rt] = (lui[rs] | imm) & 0xFFFFFFFF
        except Exception:
            pass
        continue

    if mnem == "move":
        try:
            rt = str(ins.getRegister(0))
            rs = str(ins.getRegister(1))
            if rs in lui:
                lui[rt] = lui[rs]
            elif rt in lui:
                del lui[rt]
        except Exception:
            pass
        continue

    bucket = None
    if mnem in LOAD_OPS:
        bucket = hits_load
    elif mnem in STORE_OPS:
        bucket = hits_store
    else:
        continue

    try:
        ops = ins.getOpObjects(1)
        imm = 0
        base = None
        for op in ops:
            if hasattr(op, "getValue"):
                imm = op.getValue()
                if imm & 0x8000:
                    imm -= 0x10000
            else:
                base = str(op)
        if base in lui:
            addr = (lui[base] + imm) & 0xFFFFFFFF
            if addr == TARGET_ADDR:
                func = fm.getFunctionContaining(ins.getAddress())
                fentry = func.getEntryPoint().getOffset() if func else 0
                fname = func.getName() if func else "(none)"
                bucket.setdefault((fentry, fname), []).append(ins.getAddress())
    except Exception:
        pass


def report(label, bucket):
    print("\n== {} of 0x{:08X} ==".format(label, TARGET_ADDR))
    print("hits across {} function(s)".format(len(bucket)))
    for (fentry, fname), sites in sorted(bucket.items()):
        print("\n  {} @ 0x{:08X}  ({} sites)".format(fname, fentry, len(sites)))
        for s in sites[:4]:
            print("    site: {}".format(s))
            for i in context_around(s, 3):
                print("      {}".format(fmt(i)))


report("LOADS", hits_load)
report("STORES", hits_store)
