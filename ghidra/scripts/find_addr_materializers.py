# @category Legaia
# @runtime Jython
#
# Generic LUI+ADDIU materializer finder. For each target address, walks
# every instruction in the current program, tracks per-register `lui` +
# `addiu` pairs, and reports each site where the combined value lands on
# a target address. Reports the next 6 instructions so the caller can
# classify the use (store base = writer, load base = reader, function
# argument = jal/jalr follows).
#
# Why this exists: Ghidra's reference manager bails on the LUI+ADDIU
# combination when an `addu` mixes the propagated constant with a value
# loaded from memory. The reference-database scan thus reports zero
# writers/readers to addresses materialized this way - even though they
# DO exist statically.
#
# Pattern this catches (the install store at FUN_80026B4C):
#   lui   v0, 0x8008
#   lui   v1, 0x8008
#   lw    v1, -0x488c(v1)     ; v1 = *DAT_8007B774 (index counter)
#   addiu v0, v0, -0x3fe8     ; v0 = 0x8007C018 (base) -- THIS LANDS THE TARGET
#   sll   v1, v1, 0x2         ; v1 = idx * 4
#   addu  v1, v1, v0          ; v1 = idx*4 + 0x8007C018
#   sw    a0, 0(v1)           ; store to table
#
# Usage (headless):
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process SCUS_942.54 -noanalysis \
#       -postScript /scripts/find_addr_materializers.py \
#       0x8007C018 0x8007BB38 0x8007B7DC
#
# Args may be decimal or hex (0x... prefix). If no args are passed,
# falls back to the GHIDRA_FIND_ADDRS env var (comma- or space-
# separated). If neither is set, exits with a usage message.

import os


def parse_addr(s):
    s = s.strip()
    if not s:
        return None
    base = 16 if s.lower().startswith("0x") else 10
    try:
        return int(s, base) & 0xFFFFFFFF
    except ValueError:
        return None


def collect_targets():
    args = list(getScriptArgs())
    if not args:
        env = os.environ.get("GHIDRA_FIND_ADDRS", "")
        # Allow both commas and whitespace as separators.
        args = [tok for chunk in env.split(",") for tok in chunk.split()]
    out = []
    for a in args:
        v = parse_addr(a)
        if v is not None:
            out.append(v)
        else:
            print("[warn] skipping unparseable arg: {}".format(a))
    return out


TARGETS = collect_targets()
if not TARGETS:
    print("usage: -postScript find_addr_materializers.py <addr> [<addr> ...]")
    print("       or set GHIDRA_FIND_ADDRS='0x8007c018,0x8007bb38'")
    raise SystemExit(0)

prog = currentProgram
prog_name = prog.getName()
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

inst_iter = listing.getInstructions(True)
last_lui = {}
current_func = None
out = []

# DEF-clearing set: any mnemonic that overwrites a register without
# extending the lui+addiu chain. If one of these defs a register we are
# tracking as part of a lui pair, we drop the tracked state for that
# register.
DEF_CLEAR_MNEM = set([
    "lw", "lhu", "lh", "lbu", "lb", "li", "move",
    "or", "and", "subu", "addu",
    "andi", "ori", "xori",
    "sll", "srl", "sra",
    "mflo", "mfhi", "lwc1", "lwl", "lwr",
])

while inst_iter.hasNext():
    ins = inst_iter.next()
    func = fm.getFunctionContaining(ins.getAddress())
    fa = func.getEntryPoint().getOffset() if func else None
    if fa != current_func:
        last_lui = {}
        current_func = fa
    mnem = ins.getMnemonicString()
    if mnem == "lui" and ins.getNumOperands() == 2:
        try:
            reg = ins.getDefaultOperandRepresentation(0)
            imm = ins.getOpObjects(1)[0].getValue() & 0xFFFF
            last_lui[reg] = imm << 16
        except:
            pass
        continue
    if mnem == "addiu" and ins.getNumOperands() == 3:
        try:
            dst = ins.getDefaultOperandRepresentation(0)
            src = ins.getDefaultOperandRepresentation(1)
            imm = ins.getOpObjects(2)[0].getValue()
            if src in last_lui:
                base = last_lui[src]
                combined = (base + imm) & 0xFFFFFFFF
                if combined in TARGETS:
                    fname = func.getName() if func else "?"
                    look_ahead = []
                    nxt = ins.getAddress().add(4)
                    for _ in range(6):
                        nins = listing.getInstructionAt(nxt)
                        if nins is None:
                            break
                        look_ahead.append("%s  %s" % (nxt, nins.toString()))
                        nxt = nxt.add(4)
                    out.append((str(ins.getAddress()), "0x%08X" % combined,
                                fname, dst, look_ahead))
                last_lui[dst] = combined
        except:
            pass
        continue
    if mnem in DEF_CLEAR_MNEM:
        try:
            if ins.getNumOperands() >= 1:
                d = ins.getDefaultOperandRepresentation(0)
                if d in last_lui:
                    del last_lui[d]
        except:
            pass

print("=== %s : %d materializations across %d targets ===" % (
    prog_name, len(out), len(TARGETS)))
print("targets: %s" % ", ".join("0x%08X" % t for t in TARGETS))
for site, tgt, fname, reg, ahead in out:
    print("\n%s  in %s  (reg %s = %s)" % (site, fname, reg, tgt))
    for line in ahead:
        print("    %s" % line)
