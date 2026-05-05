# @category Legaia
# @runtime Jython
#
# Find consumers of the effect-bundle data structure in the battle overlay.
#
# Background: PROT entries that use the 0x02018B0C / 28-entry-schema effect
# bundle format have an overlay-resident consumer — zero references to the
# magic word or the buffer pointers exist in SCUS_942.54 or in non-battle
# overlays. Once a battle overlay is imported (see scripts/import_overlay.sh
# pointed at /tmp/legaia_overlay_battle.bin), this script locates:
#
#   1. The init/registration function (entry 0x801DE914 — called by SCUS
#      FUN_800520F0 case 0xe).
#   2. Every other function that reads the registered-effects table at
#      _DAT_8007BD30 — these are the per-frame walkers / triggers.
#
# Output: prints each containing function with its size and a brief disasm
# of the LUI+LW pair that loads the address.
#
# Usage (after importing /tmp/legaia_overlay_battle.bin into the project):
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects/legaia legaia \
#       -process overlay.bin \
#       -postScript find_effect_bundle_consumers.py

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

# Memory addresses we care about.
TARGETS = {
    0x8007BD30: "_DAT_8007BD30 (registered effects table)",
    0x8007BD5C: "_DAT_8007BD5C (efect.dat buffer pointer)",
    0x8007BD58: "_DAT_8007BD58 (init flag byte)",
    0x8007BD24: "_DAT_8007BD24 (generic asset buffer pointer)",
}

# Per-register state: when we see `lui R, hi`, remember it; if a subsequent
# load/store uses base R with imm = lo, we resolved the combined address.
def scan():
    inst_iter = listing.getInstructions(True)
    last_lui = {}
    current_func = None
    hits = {addr: [] for addr in TARGETS}
    for insn in inst_iter:
        f = fm.getFunctionContaining(insn.getAddress())
        f_addr = f.getEntryPoint().getOffset() if f else None
        if f_addr != current_func:
            last_lui = {}
            current_func = f_addr
        mn = insn.getMnemonicString()
        if mn == "lui":
            try:
                rt = insn.getDefaultOperandRepresentation(0)
                hi = insn.getOpObjects(1)[0].getValue() & 0xFFFF
                last_lui[rt] = (insn.getAddress(), hi)
            except:
                pass
            continue
        # LW / LBU / SW / ADDIU on a previously-LUI'd register
        if mn in ("lw", "lh", "lbu", "lhu", "sw", "sh", "sb", "addiu", "lb"):
            try:
                rs_str = insn.getDefaultOperandRepresentation(1)
                # rs is an "imm(reg)" form for lw/sw, separate operand for addiu
                # Try both shapes.
                imm_obj = insn.getOpObjects(1)
                if len(imm_obj) >= 2:
                    # lw/sw: addressing mode "imm(reg)"
                    imm = imm_obj[0].getValue() & 0xFFFF
                    base_reg = str(imm_obj[1])
                else:
                    # addiu rt, rs, imm — operand 2 is the imm
                    imm = insn.getOpObjects(2)[0].getValue() & 0xFFFF
                    base_reg = insn.getDefaultOperandRepresentation(1)
                if base_reg in last_lui:
                    lui_addr, hi = last_lui[base_reg]
                    # Combine: hi<<16 + sign_ext(imm)
                    if imm & 0x8000:
                        combined = (hi << 16) - 0x10000 + imm
                    else:
                        combined = (hi << 16) + imm
                    combined &= 0xFFFFFFFF
                    if combined in TARGETS:
                        hits[combined].append((lui_addr, insn.getAddress(), mn))
            except:
                pass
    return hits

print("\n=== Effect-bundle consumer discovery ===\n")
results = scan()
for addr, label in TARGETS.items():
    refs = results[addr]
    print("0x%08X  %s" % (addr, label))
    print("  %d hits" % len(refs))
    for lui_addr, ld_addr, mn in refs[:20]:
        f = fm.getFunctionContaining(ld_addr)
        f_label = f.getName() if f else "(no function)"
        print("    LUI %s + %s %s  in  %s" % (lui_addr, mn, ld_addr, f_label))
    print("")

# Identify the unique containing functions per target.
print("\n=== Unique containing functions ===\n")
for addr, label in TARGETS.items():
    refs = results[addr]
    funcs = set()
    for _, ld_addr, _ in refs:
        f = fm.getFunctionContaining(ld_addr)
        if f:
            funcs.add(f.getEntryPoint().getOffset())
    print("0x%08X  %s  ->  %d unique function(s):" % (addr, label, len(funcs)))
    for fa in sorted(funcs):
        print("    0x%X" % fa)
