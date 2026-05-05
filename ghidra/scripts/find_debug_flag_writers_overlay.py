# @category Legaia
# @runtime Jython
#
# Scan the current Ghidra program (run on overlay programs) for stores to
# either of the two retail debug flags:
#   - _DAT_8007B8C2 (dev/retail mode flag)
#   - _DAT_8007B98F (debug-menu select flag)
#
# Generic version of find_lui_writers.py: any `lui+sw` or `lui+addiu+sw`
# pair whose effective address lands on either target byte counts as a
# write site. Reports per-function. Critical because TCRF GameShark codes
# prove both flags are runtime-writable but no SCUS_942.54 writer exists.

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()

TARGETS = {0x8007B8C2: "DEV_FLAG_8007B8C2",
           0x8007B98F: "DEBUG_MENU_8007B98F",
           # Also check the surrounding word -- DEBUG_MENU is read as the
           # high byte of word 0x8007B98C, so word-aligned writes to
           # 0x8007B98C touch the same byte.
           0x8007B98C: "DEBUG_MENU_WORD_8007B98C"}

# Generous slack: writes anywhere in the +-3 byte range around target
SLACK = 3
hits = []  # list of (func_entry, insn_addr, mnemonic, operand_text, target)

# Per-function lui state map: reg -> hi_imm
inst_iter = listing.getInstructions(True)
last_lui = {}
last_addiu = {}  # reg -> (base_reg, full_addr)
current_func_addr = None

def reg_str(insn, idx):
    return insn.getDefaultOperandRepresentation(idx)

while inst_iter.hasNext():
    insn = inst_iter.next()
    func = fm.getFunctionContaining(insn.getAddress())
    fa = func.getEntryPoint().getOffset() if func else None
    if fa != current_func_addr:
        last_lui = {}
        last_addiu = {}
        current_func_addr = fa
    mnem = insn.getMnemonicString()
    if mnem == "lui":
        try:
            reg = reg_str(insn, 0)
            imm = insn.getOpObjects(1)[0].getValue() & 0xFFFF
            last_lui[reg] = imm << 16
            if reg in last_addiu:
                del last_addiu[reg]
        except Exception:
            pass
    elif mnem == "addiu" and insn.getNumOperands() == 3:
        try:
            dst = reg_str(insn, 0)
            src = reg_str(insn, 1)
            imm = insn.getOpObjects(2)[0].getValue()
            # signed immediate
            if imm >= 0x8000:
                imm -= 0x10000
            if src in last_lui:
                last_addiu[dst] = last_lui[src] + imm
        except Exception:
            pass
    elif mnem in ("sw", "sh", "sb"):
        try:
            ops = insn.getOpObjects(1)
            # Operand 1 typically: imm(reg)
            if len(ops) >= 2:
                imm_obj = ops[0]
                base_reg_obj = ops[1]
                imm = imm_obj.getValue()
                if imm >= 0x8000:
                    imm -= 0x10000
                base = str(base_reg_obj)
                if base in last_lui:
                    eff = last_lui[base] + imm
                elif base in last_addiu:
                    eff = last_addiu[base] + imm
                else:
                    continue
                for tgt, name in TARGETS.items():
                    if abs(eff - tgt) <= SLACK:
                        hits.append((fa, insn.getAddress().getOffset(), mnem, str(insn), name, eff))
                        break
        except Exception:
            pass

print("found {} writes in {} program".format(len(hits), prog.getName()))
seen = set()
for fa, addr, mnem, txt, name, eff in hits:
    key = (fa, addr)
    if key in seen:
        continue
    seen.add(key)
    fa_str = "func@0x{:08X}".format(fa) if fa else "(no func)"
    print("  {}  0x{:08X}  {:<35s}  -> {} (eff 0x{:08X})".format(
        fa_str, addr, txt, name, eff))
