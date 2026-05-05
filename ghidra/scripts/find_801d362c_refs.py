# @category Legaia
# @runtime Jython
#
# Hunt for every reference to 0x801D362C - the 61-case actor command
# dispatcher in the 0897 town overlay. It has zero static jal callers in
# the dumps we've taken so far, so the call site is either:
#   (a) in another overlay we haven't searched (battle, 0971, 0978, etc.)
#   (b) reached via a function-pointer table (data ref, not code ref)
#   (c) jalr on a register loaded from such a table.
#
# Strategy: across every program currently in the project, walk every
# instruction looking for:
#   - direct jal to 0x801D362C
#   - lui+addiu pair that materializes 0x801D362C (function-pointer loads)
#   - data 4-byte words equal to 0x801D362C in any defined memory block
#
# Run with:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#     /projects legaia -process <PROGRAM> -noanalysis \
#     -prescript /scripts/find_801d362c_refs.py
#
# Or use a -process pattern that covers all programs (e.g. ".*").

from ghidra.program.model.address import AddressFactory

TARGET = 0x801D362C

prog = currentProgram
prog_name = prog.getName()
mem = prog.getMemory()
listing = prog.getListing()
af = prog.getAddressFactory()
fm = prog.getFunctionManager()


def find_jal_callers():
    hits = []
    instrs = listing.getInstructions(True)
    for ins in instrs:
        mnem = ins.getMnemonicString()
        if mnem != "jal":
            continue
        ops = ins.getDefaultOperandRepresentation(0)
        # operand text usually looks like "0x801d362c"
        if "801d362c" in ops.lower() or "0x801d362c" == ops.lower():
            func = fm.getFunctionContaining(ins.getAddress())
            fname = func.getName() if func else "(no-func)"
            hits.append((str(ins.getAddress()), fname, ops))
    return hits


def find_lui_addiu_pairs():
    """Track per-register LUI immediates and find addiu pairs that yield TARGET."""
    hits = []
    # state: reg -> last-loaded high half word (left-shifted), tracked per func
    instrs = listing.getInstructions(True)
    reg_hi = {}  # reg-name -> (signed hi << 16, addr-of-lui)
    func_at_start = None

    for ins in instrs:
        cur_func = fm.getFunctionContaining(ins.getAddress())
        if cur_func != func_at_start:
            reg_hi = {}
            func_at_start = cur_func

        mnem = ins.getMnemonicString()
        if mnem == "lui":
            try:
                rd = ins.getDefaultOperandRepresentation(0)  # e.g. "v0"
                imm = ins.getScalar(1)
                if imm is None:
                    continue
                hi = (imm.getValue() & 0xFFFF) << 16
                reg_hi[rd] = (hi, ins.getAddress())
            except Exception:
                continue
        elif mnem in ("addiu", "ori", "addi"):
            try:
                rd = ins.getDefaultOperandRepresentation(0)
                rs = ins.getDefaultOperandRepresentation(1)
                if rs not in reg_hi:
                    continue
                imm = ins.getScalar(2)
                if imm is None:
                    continue
                lo_signed = imm.getValue()
                # MIPS: addiu sign-extends; ori zero-extends.
                if mnem == "ori":
                    lo = lo_signed & 0xFFFF
                else:
                    lo = lo_signed  # already signed
                full = (reg_hi[rs][0] + lo) & 0xFFFFFFFF
                if full == TARGET:
                    func = fm.getFunctionContaining(ins.getAddress())
                    fname = func.getName() if func else "(no-func)"
                    hits.append((
                        "{}/{}".format(reg_hi[rs][1], ins.getAddress()),
                        fname,
                        "{} {} = {} + {:#x} -> 0x{:08x}".format(mnem, rd, rs, lo_signed, full),
                    ))
                # Update if rd changes (helpful for follow-on chains)
                reg_hi[rd] = (full & 0xFFFF0000, ins.getAddress())
            except Exception:
                continue
    return hits


def find_data_refs():
    """Scan memory for u32 little-endian words equal to TARGET."""
    hits = []
    for block in mem.getBlocks():
        if not block.isInitialized():
            continue
        # Skip code blocks for speed (TARGET would already be caught by jal/lui).
        # Ghidra blocks: name, start, end. Heuristic: skip blocks that match the
        # main program's executable region only if we know they're code; but to
        # be thorough, do scan all initialized blocks.
        start = block.getStart()
        end = block.getEnd()
        cur = start
        # Read in chunks to keep things sane.
        size = block.getSize()
        if size > 0x800000:  # >8 MB? skip
            continue
        try:
            data = bytearray(size)
            for i in range(size):
                data[i] = mem.getByte(start.add(i)) & 0xFF
        except Exception:
            continue
        # Scan 4-byte aligned LE u32s.
        for i in range(0, size - 3, 4):
            word = data[i] | (data[i+1] << 8) | (data[i+2] << 16) | (data[i+3] << 24)
            if word == TARGET:
                addr = start.add(i)
                hits.append((
                    str(addr),
                    block.getName(),
                    "u32 LE = 0x{:08x}".format(TARGET),
                ))
    return hits


print("=" * 72)
print("program: {}".format(prog_name))
print("target : 0x{:08x}".format(TARGET))
print("=" * 72)

print("\n[1/3] Scanning for direct jal callers...")
jals = find_jal_callers()
print("    found {} jal sites".format(len(jals)))
for addr, fname, ops in jals:
    print("      {} {} -> {}".format(addr, fname, ops))

print("\n[2/3] Scanning for lui+addiu pointer materialization...")
luis = find_lui_addiu_pairs()
print("    found {} lui+lo pairs that compute target".format(len(luis)))
for addrs, fname, expr in luis[:50]:
    print("      {} {} | {}".format(addrs, fname, expr))
if len(luis) > 50:
    print("      ...({} more)".format(len(luis) - 50))

print("\n[3/3] Scanning for data refs (u32 LE = target)...")
datas = find_data_refs()
print("    found {} data words".format(len(datas)))
for addr, blk, expr in datas[:50]:
    print("      {} [{}] {}".format(addr, blk, expr))
if len(datas) > 50:
    print("      ...({} more)".format(len(datas) - 50))

print("\ndone")
