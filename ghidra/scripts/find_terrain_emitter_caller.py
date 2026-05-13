# @category Legaia
# @runtime Jython
#
# Find the outer caller(s) of FUN_801D7EA0 - the world-map continent
# terrain emitter. The function has Ghidra-incoming=0 because it's
# called via function pointer (the address is computed by LUI+ADDIU
# and stored to a dispatch table, which Ghidra's reference manager
# misses, per the cross-cutting fact in CLAUDE.md).
#
# Two passes:
#
#   1. Reference manager sweep on 0x801D7EA0 itself. Catches anything
#      Ghidra DID resolve.
#   2. LUI+ADDIU pair sweep across all instructions in the loaded
#      program. Reports every (lui R, 0x801E ; addiu R, R, -0x8160)
#      pair (sign-extension form) or (lui R, 0x801D ; addiu R, R,
#      0x7EA0). Both forms compute 0x801D7EA0.
#   3. Same LUI+ADDIU sweep on the gate flag 0x801F351C. Whoever
#      writes nonzero to this address is what TRIGGERS terrain
#      emission - which gives us the second half of the picture
#      (the scene-init code that arms the emitter).
#
# Run against every overlay that might contain code at 0x801C0000+:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_world_map.bin -noanalysis \
#       -postScript /scripts/find_terrain_emitter_caller.py
#
# Each invocation processes one overlay. Outputs to stdout (the
# launcher's analyzer log).

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()

PROGRAM_NAME = prog.getName()
print("=== find_terrain_emitter_caller against %s ===" % PROGRAM_NAME)

# --------------------------------------------------------------
# Pass 1: Ghidra reference manager (cheap, often misses indirect)

TARGETS_HEX = [
    "801D7EA0",  # the terrain emitter itself
    "801F351C",  # the one-shot gate flag
    "801D1344",  # the trigger-caller of FUN_801D8258
    "801D8258",  # the gate setter
    "80016444",  # SCUS-resident terrain tick
    "8007BC3C",  # game_mode submode that gates the call
]

for t in TARGETS_HEX:
    addr = af.getAddress(t)
    if addr is None:
        print("  [skip pass1] address %s not in %s" % (t, PROGRAM_NAME))
        continue
    refs = list(ref_mgr.getReferencesTo(addr))
    print("\n--- pass1: refs to 0x%s in %s : %d ---" % (t, PROGRAM_NAME, len(refs)))
    for r in refs:
        from_a = r.getFromAddress()
        from_func = fm.getFunctionContaining(from_a)
        fn = from_func.getName() if from_func else "?"
        fe = str(from_func.getEntryPoint()) if from_func else "?"
        ins = listing.getInstructionAt(from_a)
        print("  %s in %s @ %s: %s" % (
            from_a, fn, fe, ins.toString() if ins else "?"))


# --------------------------------------------------------------
# Pass 2 + 3: LUI+ADDIU sweep
#
# We're looking for any pair that computes either:
#   - 0x801D7EA0  (function pointer to the emitter)
#   - 0x801F351C  (gate flag address - to set/clear/test)
# Anyone computing 0x801F351C in a context that writes it (sw/sh/sb
# with that as the effective address) is the trigger. Anyone writing
# 0x801D7EA0 into a memory cell is installing the function pointer.

TARGET_ADDRS = {
    0x801D7EA0: "emitter_ptr",
    0x801F351C: "gate_flag",
    0x801D1344: "trigger_caller",
    0x801D8258: "gate_setter",
    0x80016444: "scus_terrain_tick",
    0x8007BC3C: "game_mode_var",
}

# Walk all instructions, track most-recent LUI per register, reset at
# function boundaries.
last_lui = {}
current_func_addr = None

# Report tables
# hit_func[fa] = list of (instr_addr_str, kind, target_int, dst_reg, store_op?, store_dst?)
hits = {}

inst_iter = listing.getInstructions(True)
scanned = 0
addiu_hits = 0
mem_hits = 0

while inst_iter.hasNext():
    insn = inst_iter.next()
    scanned += 1
    func = fm.getFunctionContaining(insn.getAddress())
    fa = func.getEntryPoint().getOffset() if func else None
    if fa != current_func_addr:
        last_lui = {}
        current_func_addr = fa

    mnem = insn.getMnemonicString()
    ops = insn.getNumOperands()

    if mnem == "lui" and ops == 2:
        try:
            reg = insn.getDefaultOperandRepresentation(0)
            imm = insn.getOpObjects(1)[0].getValue() & 0xFFFF
            last_lui[reg] = imm << 16
        except:
            pass
        continue

    # addiu reg, src, imm  -> if combined matches a TARGET_ADDR, that's
    # the function-pointer load or the flag-address compute.
    if mnem == "addiu" and ops == 3:
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            src = insn.getDefaultOperandRepresentation(1)
            imm = insn.getOpObjects(2)[0].getValue()
            if src in last_lui:
                base = last_lui[src]
                combined = (base + imm) & 0xFFFFFFFF
                if combined in TARGET_ADDRS:
                    addiu_hits += 1
                    hits.setdefault(fa, []).append(
                        (str(insn.getAddress()), "lui+addiu",
                         combined, dst, None))
                last_lui[dst] = combined
        except:
            pass
        continue

    # ori reg, src, imm -- some compilers use lui+ori to build constants.
    if mnem == "ori" and ops == 3:
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            src = insn.getDefaultOperandRepresentation(1)
            imm = insn.getOpObjects(2)[0].getValue() & 0xFFFF
            if src in last_lui:
                base = last_lui[src]
                combined = (base | imm) & 0xFFFFFFFF
                if combined in TARGET_ADDRS:
                    addiu_hits += 1
                    hits.setdefault(fa, []).append(
                        (str(insn.getAddress()), "lui+ori",
                         combined, dst, None))
                last_lui[dst] = combined
        except:
            pass
        continue

    # jal / jalr target check. A direct `jal 0x801D7EA0` from any
    # overlay's code would show up here, regardless of whether Ghidra's
    # reference manager has indexed it. Ghidra's getOpObjects() returns
    # an Address object for `jal` whose .getOffset() gives the target.
    if mnem in ("jal", "j") and ops >= 1:
        try:
            target_op = insn.getOpObjects(0)
            if target_op and hasattr(target_op[0], "getOffset"):
                tgt = target_op[0].getOffset() & 0xFFFFFFFF
                if tgt in TARGET_ADDRS:
                    addiu_hits += 1
                    hits.setdefault(fa, []).append(
                        (str(insn.getAddress()), "%s_DIRECT" % mnem,
                         tgt, None, None))
        except:
            pass
        continue

    # sw/sh/sb/lw/lh/lb with off(base) form.  If the effective address
    # (base_reg's last_lui + off) matches a TARGET_ADDR, this is a
    # direct store/load to the flag or pointer.
    if mnem in ("sw", "sh", "sb", "lw", "lh", "lhu", "lb", "lbu") and ops == 2:
        try:
            base_rep = insn.getDefaultOperandRepresentation(1)
            if "(" in base_rep and base_rep.endswith(")"):
                offstr, basestr = base_rep.split("(")
                basestr = basestr.rstrip(")")
                off = None
                if offstr.startswith("0x") or offstr.startswith("-0x"):
                    off = int(offstr, 16)
                elif offstr and (offstr[0] in "-+0123456789"):
                    off = int(offstr, 0)
                if off is not None and basestr in last_lui:
                    base = last_lui[basestr]
                    combined = (base + off) & 0xFFFFFFFF
                    if combined in TARGET_ADDRS:
                        mem_hits += 1
                        kind = "STORE" if mnem.startswith("s") else "load"
                        # capture the value being stored (the first
                        # operand of the store)
                        store_src = insn.getDefaultOperandRepresentation(0)
                        hits.setdefault(fa, []).append(
                            (str(insn.getAddress()),
                             "%s_%s" % (mnem, kind),
                             combined, store_src, None))
        except:
            pass

print("\n=== sweep done : scanned=%d addiu_hits=%d mem_hits=%d funcs=%d ===" % (
    scanned, addiu_hits, mem_hits, len(hits)))

# Sort by number of hits, descending
for fa, refs in sorted(hits.items(), key=lambda kv: -len(kv[1])):
    if fa is None:
        print("\n  <not in any function>")
        fname = "<orphan>"
    else:
        func = fm.getFunctionAt(af.getAddress("%x" % fa))
        fname = func.getName() if func else "<orphan>"
        print("\n  %s @ 0x%08X  (%d hits)" % (fname, fa, len(refs)))
    for ia, kind, tgt, reg, _ in refs:
        tgt_label = TARGET_ADDRS.get(tgt, "?")
        print("    %s  %-14s  0x%08X (%s)  reg=%s" % (
            ia, kind, tgt, tgt_label, reg))

print("\n=== end ===")
