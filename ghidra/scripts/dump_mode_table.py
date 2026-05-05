# @category Legaia
# @runtime Jython
#
# Dump the mode-handler table at 0x8007078C (24 bytes per entry) and find
# which function calls handlers from it -- the actual game-mode dispatcher.

prog = currentProgram
af = prog.getAddressFactory()
listing = prog.getListing()
fm = prog.getFunctionManager()
mem = prog.getMemory()

TABLE_BASE = 0x8007078C
ENTRY_SIZE = 0x18

print("=== Mode-handler table at 0x{:08X} (24-byte entries) ===".format(TABLE_BASE))
print("(reading until first all-zero entry or 32 entries)")
n_modes = 0
for i in range(40):
    a = af.getAddress("{:x}".format(TABLE_BASE + i * ENTRY_SIZE))
    bs = []
    try:
        for j in range(ENTRY_SIZE):
            bs.append(mem.getByte(a.add(j)) & 0xFF)
    except:
        break
    if all(b == 0 for b in bs):
        print("  mode[{:2d}]  ALL ZERO  (likely sentinel)".format(i))
        break
    # interpret per FUN_800179c0:
    #   +0xa  i16 next_mode (-1 means "no transition")
    next_mode = bs[0xa] | (bs[0xb] << 8)
    if next_mode > 0x7FFF:
        next_mode -= 0x10000
    # try to interpret various u32 fields as function pointers
    def u32(off):
        return bs[off] | (bs[off+1] << 8) | (bs[off+2] << 16) | (bs[off+3] << 24)
    p_00 = u32(0x00)
    p_04 = u32(0x04)
    p_08 = u32(0x08)
    p_0c = u32(0x0c)
    p_10 = u32(0x10)
    p_14 = u32(0x14)
    print("  mode[{:2d}] @ {}: next={:5d}  u32@0={:08x}  @4={:08x}  @8={:08x}  @c={:08x}  @10={:08x}  @14={:08x}".format(
        i, a, next_mode, p_00, p_04, p_08, p_0c, p_10, p_14))
    n_modes += 1

# For each potential function pointer field, check if it points to actual code
print("\n=== Verify which u32 field is actually a function pointer ===")
print("(checking if value is a known function entry in 0x80010000-0x80060000 range)")
for offset_in_entry in [0x00, 0x04, 0x08, 0x0c, 0x10, 0x14]:
    is_code_count = 0
    for i in range(n_modes):
        a = af.getAddress("{:x}".format(TABLE_BASE + i * ENTRY_SIZE + offset_in_entry))
        try:
            v = mem.getInt(a) & 0xFFFFFFFF
        except:
            continue
        if 0x80010000 <= v <= 0x80060000:
            try:
                target_addr = af.getAddress("{:x}".format(v))
                func = fm.getFunctionContaining(target_addr)
                if func and func.getEntryPoint().getOffset() == v:
                    is_code_count += 1
            except:
                pass
    print("  offset +0x{:02X}:  {}/{} entries point at known function entries".format(
        offset_in_entry, is_code_count, n_modes))

print("\n=== Find the actual dispatcher: function that does jalr to *(table_base + idx*24 + handler_offset) ===")
# We need to find: a function that loads a value from the table (at a specific offset
# within the entry) and calls it via jalr.
# Pattern: lui+addiu to load TABLE_BASE, then sll/addu chain for *24 indexing, then
# lw with some offset, then jalr.

# Quicker: find any function that uses the table base AND has a jalr instruction
# within ~20 instructions of the table load.
inst_list = list(listing.getInstructions(True))
print("scanning {} instructions for table-load + jalr pattern...".format(len(inst_list)))

# Build a per-function set of "uses the table"
fn_using_table = set()
last_lui = {}
cur_func = None
fn_table_uses = {}  # func_entry -> [(insn_addr, computed_addr, mnem)]
for insn in inst_list:
    func = fm.getFunctionContaining(insn.getAddress())
    fa = func.getEntryPoint().getOffset() if func else None
    if fa != cur_func:
        last_lui = {}
        cur_func = fa
    mnem = insn.getMnemonicString()
    ops = insn.getNumOperands()
    if mnem == "lui" and ops == 2:
        try:
            reg = insn.getDefaultOperandRepresentation(0)
            imm = insn.getOpObjects(1)[0].getValue() & 0xFFFF
            last_lui[reg] = imm << 16
        except: pass
        continue
    if mnem == "addiu" and ops == 3:
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            src = insn.getDefaultOperandRepresentation(1)
            imm = insn.getOpObjects(2)[0].getValue()
            if src in last_lui:
                combined = (last_lui[src] + imm) & 0xFFFFFFFF
                last_lui[dst] = combined
                if combined == TABLE_BASE:
                    fn_table_uses.setdefault(fa, []).append((str(insn.getAddress()), combined, mnem))
        except: pass

# Now find which of those functions has jalr nearby
print("\nFunctions that load TABLE_BASE (0x{:08X}):".format(TABLE_BASE))
for fa in sorted(fn_table_uses.keys()):
    func = fm.getFunctionAt(af.getAddress("{:x}".format(fa)))
    name = func.getName() if func else "?"
    body = func.getBody() if func else None
    has_jalr = False
    if body:
        for insn in listing.getInstructions(body, True):
            if insn.getMnemonicString() == "jalr":
                has_jalr = True
                break
    marker = " <-- HAS JALR" if has_jalr else ""
    print("  {} @ 0x{:08X}{}".format(name, fa, marker))

print("\ndone")
