# @category Legaia
# @runtime Jython
#
# Walk the continent-terrain emitter call chain rooted at FUN_8002C69C.
# Dumps the function + its callers and direct callees, plus the static
# tile-atlas table at DAT_80073A00 (32 bytes per tile entry) and the
# DAT_800732A5 / DAT_800732A7 byte-indexed tables that hold per-record
# (tile_id, flag) pairs.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()
mem = prog.getMemory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
decomp.setOptions(DecompileOptions())
decomp.openProgram(prog)


def dump_func(addr_str, label=""):
    addr = af.getAddress(addr_str)
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function for %s" % addr_str)
        return
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_path = os.path.join(OUT_DIR, addr_str + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== %s %s (entry=%s) %s ==\n" % (
            func.getName(), addr_str, func.getEntryPoint(), label))
        fh.write("size=%d bytes, %d instructions\n\n" % (
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("%s  %s\n" % (ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 120, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: %s)\n" % res.getErrorMessage())
        except Exception as e:
            fh.write("(decompile exception: %s)\n" % e)
    print("wrote %s" % out_path)


def list_callers(addr_str, label=""):
    addr = af.getAddress(addr_str)
    refs = list(ref_mgr.getReferencesTo(addr))
    target = fm.getFunctionAt(addr)
    name = target.getName() if target else "?"
    print("\n=== callers of %s (%s) %s -- %d refs ===" % (
        addr_str, name, label, len(refs)))
    for r in refs:
        from_a = r.getFromAddress()
        from_func = fm.getFunctionContaining(from_a)
        from_fn_name = from_func.getName() if from_func else "?"
        from_fn_entry = str(from_func.getEntryPoint()) if from_func else "?"
        ins = listing.getInstructionAt(from_a)
        print("  from %s in %s @ %s: %s" % (
            from_a, from_fn_name, from_fn_entry,
            ins.toString() if ins else "?"))


def dump_data(addr_str, length, label):
    addr = af.getAddress(addr_str)
    bs = bytearray()
    try:
        for i in range(length):
            b = mem.getByte(addr.add(i)) & 0xFF
            bs.append(b)
    except Exception as e:
        print("[skip] %s read failed: %s" % (addr_str, e))
        return
    out_path = os.path.join(OUT_DIR, "data_%s.txt" % addr_str)
    with open(out_path, "w") as fh:
        fh.write("== %s @ %s (%d bytes) ==\n\n" % (label, addr_str, length))
        # hex dump 16 bytes per line.
        for off in range(0, length, 16):
            row = bs[off:off + 16]
            hexpart = " ".join("%02x" % b for b in row)
            ascpart = "".join(chr(b) if 32 <= b < 127 else "." for b in row)
            fh.write("%08x  %-47s  %s\n" % (
                addr.getOffset() + off, hexpart, ascpart))
    print("wrote %s" % out_path)


def find_callers_at_constants(target_int):
    """Find every instruction that loads target_int as a 32-bit constant
    (via lui+addiu/ori), so we can locate everything that touches the
    tile-atlas table address."""
    print("\n=== lui+addiu sites loading 0x%08X ===" % target_int)
    hi = (target_int >> 16) & 0xFFFF
    lo16 = target_int & 0xFFFF
    if lo16 & 0x8000:
        hi = (hi + 1) & 0xFFFF
    inst_iter = listing.getInstructions(True)
    last_lui = {}  # reg -> (addr, hi_value)
    current_func = None
    while inst_iter.hasNext():
        insn = inst_iter.next()
        func = fm.getFunctionContaining(insn.getAddress())
        fa = func.getEntryPoint().getOffset() if func else None
        if fa != current_func:
            last_lui = {}
            current_func = fa
        mnem = insn.getMnemonicString()
        if mnem == "lui":
            try:
                dst = insn.getDefaultOperandRepresentation(0)
                imm = insn.getOpObjects(1)[0].getValue() & 0xFFFF
                last_lui[dst] = (str(insn.getAddress()), imm << 16)
            except Exception:
                pass
            continue
        if mnem in ("addiu", "ori") and insn.getNumOperands() == 3:
            try:
                dst = insn.getDefaultOperandRepresentation(0)
                src = insn.getDefaultOperandRepresentation(1)
                imm = insn.getOpObjects(2)[0].getValue()
                if src in last_lui:
                    base = last_lui[src][1]
                    combined = (base + (imm if imm >= 0 else (imm & 0xFFFFFFFF))) & 0xFFFFFFFF
                    # Treat addiu as signed: sign-extend.
                    if mnem == "addiu" and imm < 0:
                        combined = (last_lui[src][1] + imm) & 0xFFFFFFFF
                    if combined == target_int:
                        fname = (
                            fm.getFunctionContaining(insn.getAddress()).getName()
                            if fm.getFunctionContaining(insn.getAddress())
                            else "?"
                        )
                        fentry = (
                            str(fm.getFunctionContaining(insn.getAddress()).getEntryPoint())
                            if fm.getFunctionContaining(insn.getAddress())
                            else "?"
                        )
                        print("  %s in %s @ %s: %s" % (
                            insn.getAddress(), fname, fentry,
                            insn.toString()))
                    last_lui[dst] = (last_lui[src][0], combined)
            except Exception:
                pass


# --- targets ---
# The emitter + its caller.
dump_func("8002c69c", "POLY_FT4 / SPRT emitter -- 10 cmd=0x2C sites")
dump_func("80031d00", "sole static caller of FUN_8002C69C")

# Callees observed in the emitter that look like helpers worth dumping.
dump_func("8003d2c4", "OT linker helper (called per emitted prim)")

# Caller chain.
list_callers("80031d00", "(callers of the emitter dispatcher)")
list_callers("8002c69c", "(direct callers of the emitter)")
list_callers("8003d2c4", "(direct callers of the OT linker)")

# Static data tables.
dump_data("80073a00", 32 * 64, "Tile atlas table (32-byte entries)")
dump_data("80073280", 256, "Adjacent table containing DAT_800732A5 / DAT_800732A7")

# Find every site that loads the tile-atlas table address. This catches
# the per-mode call sites, including any uninspected siblings.
find_callers_at_constants(0x80073A00)
find_callers_at_constants(0x800732A5)
find_callers_at_constants(0x800732A7)

print("\ndone")
