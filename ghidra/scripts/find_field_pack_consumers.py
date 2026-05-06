# @category Legaia
# @runtime Jython
#
# Hunt for runtime consumers of the field-pack 97-slot schema. The
# schema has a static offset layout — see crates/asset/src/field_pack.rs
# for the canonical values. A consumer is any function that loads from
# `[reg + slot_off]` for one of the schema's interior slot offsets.
#
# Approach: walk every instruction in the current program, collect
# `lw / lh / lhu / lb / lbu / sb / sh / sw` instructions whose immediate
# matches one of the schema offsets, and report (function, instruction
# address, slot offset) tuples. Hits clustered into one function are
# strong evidence that function reads field-pack data at a fixed base.
#
# Run with -process for each captured program: SCUS_942.54, the 0897
# town overlay, the menu overlay, the battle action overlay. Output goes
# to ghidra/scripts/funcs/field_pack_consumers_<prog>.txt for diffing.

import os

# Subset of the 97 schema slot offsets — picked to skip ones likely to
# collide with regular MIPS struct accesses (small offsets like 0x60 are
# common for any struct base). These large interior offsets are
# field-pack-specific.
SLOT_OFFSETS = [
    0x0E68,
    0x1230,
    0x16FC,
    0x1B68,
    0x1F38,
    0x2108,
    0x2890,
    0x3010,
    0x3790,
    0x3F10,
    0x4690,
    0x4E10,
    0x5590,
    0x5D10,
    0x6490,
    0x6C10,
    0x7390,
    0x7B10,
    0x8290,
    0x8A10,
    0x9190,
    0x9910,
    0xA090,
    0xA810,
    0xAF90,
    0xB710,
    0xBE90,
    0xC610,
    0xCD90,
    0xD510,
    0xDC90,
    0xE410,
    0xEB90,
    0xF310,
    0xFA90,
    0x10210,
    0x10990,
    0x11110,
    0x11890,
    0x12010,
    0x12790,
    0x12F10,
    0x13690,
    0x13E10,
    0x14590,
    0x14D10,
    0x15490,
    0x15C10,
    0x16390,
    0x16651,
]
SLOTS = set(SLOT_OFFSETS)

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
prog_name = prog.getName()
listing = prog.getListing()
fm = prog.getFunctionManager()


def label_for(prog_name):
    return prog_name.replace(".bin", "").replace(".", "_")


def signed_imm(ins):
    # Walk the operand list looking for an scalar with one of the SLOT
    # values. Ghidra surfaces these as `Scalar` operand objects.
    for n in range(ins.getNumOperands()):
        ops = ins.getOpObjects(n)
        for op in ops:
            try:
                v = op.getValue()
            except AttributeError:
                continue
            if v in SLOTS:
                return v
    return None


hits = []
for ins in listing.getInstructions(True):
    mnem = ins.getMnemonicString().lower()
    # Only memory-touching instructions are relevant.
    if mnem not in (
        "lw",
        "lh",
        "lhu",
        "lb",
        "lbu",
        "sw",
        "sh",
        "sb",
    ):
        continue
    imm = signed_imm(ins)
    if imm is None:
        continue
    addr = ins.getAddress()
    func = fm.getFunctionContaining(addr)
    func_addr = func.getEntryPoint() if func else None
    hits.append((str(addr), mnem, imm, str(func_addr) if func_addr else "(no func)"))


out_path = os.path.join(OUT_DIR, "field_pack_consumers_" + label_for(prog_name) + ".txt")
fh = open(out_path, "w")
try:
    fh.write("# field-pack consumer search [{}]\n".format(prog_name))
    fh.write("# total hits: {}\n".format(len(hits)))
    fh.write("# format: ins_addr  mnem  imm(hex)  containing_func\n\n")
    # Group by containing function for readability.
    by_func = {}
    for addr, mnem, imm, func in hits:
        by_func.setdefault(func, []).append((addr, mnem, imm))
    for func in sorted(by_func.keys()):
        rows = by_func[func]
        fh.write("== func={}, hits={} ==\n".format(func, len(rows)))
        for addr, mnem, imm in rows:
            fh.write("  {}  {:5}  0x{:X}\n".format(addr, mnem, imm))
        fh.write("\n")
finally:
    fh.close()

print("[{}] wrote {} hits across {} functions to {}".format(
    prog_name, len(hits), len(set(h[3] for h in hits)), out_path))
