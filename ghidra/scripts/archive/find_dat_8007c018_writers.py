# @category Legaia
# @runtime Jython
#
# LUI+ADDIU writers/readers/addressors of DAT_8007C018 - the per-kingdom-
# per-kind data pointer table consumed by FUN_801F69D8 in
# overlay_world_map_top_ext.bin at PC 0x801F71F8:
#
#   iVar7 = *(int *)(&DAT_8007C018 + (iRam8007B6F8 + psVar12[8]) * 4);
#   ...
#   func_0x80043390(iVar7 + 0xC, color_flags, fog);
#
# The address 0x8007C018 sits 0x818 bytes past the end of SCUS's text
# segment (t_addr=0x80010000, t_size=0x6B800 -> end 0x8007B800), so the
# table is zero-initialized BSS and must be runtime-filled.
#
# Run this script against SCUS_942.54 and every overlay binary to catch
# both static SCUS-resident installers and overlay-side fixups:
#
#   for p in SCUS_942.54 overlay_world_map.bin overlay_world_map_top.bin \
#            overlay_world_map_top_ext.bin overlay_world_map_walk.bin; do
#       docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#           /projects legaia -process "$p" -noanalysis \
#           -postScript /scripts/find_dat_8007c018_writers.py
#   done

prog = currentProgram
prog_name = prog.getName()
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

# Window: the table itself + ~256 4-byte entries (1 KiB) of headroom.
LO = 0x8007C000
HI = 0x8007C400

hits = {}
last_lui = {}
current_func = None

inst_iter = listing.getInstructions(True)
total = 0
while inst_iter.hasNext():
    ins = inst_iter.next()
    total += 1
    func = fm.getFunctionContaining(ins.getAddress())
    fa = func.getEntryPoint().getOffset() if func else None
    if fa != current_func:
        last_lui = {}
        current_func = fa
    mnem = ins.getMnemonicString()
    ops = ins.getNumOperands()
    if mnem == "lui" and ops == 2:
        try:
            reg = ins.getDefaultOperandRepresentation(0)
            imm = ins.getOpObjects(1)[0].getValue() & 0xFFFF
            last_lui[reg] = imm << 16
        except:
            pass
        continue
    if mnem == "addiu" and ops == 3:
        try:
            dst = ins.getDefaultOperandRepresentation(0)
            src = ins.getDefaultOperandRepresentation(1)
            imm = ins.getOpObjects(2)[0].getValue()
            if src in last_lui:
                base = last_lui[src]
                combined = (base + imm) & 0xFFFFFFFF
                if LO <= combined < HI:
                    hits.setdefault(fa, []).append(
                        (str(ins.getAddress()), "lui+addiu",
                         "0x{:08X}".format(combined)))
                last_lui[dst] = combined
        except:
            pass
        continue
    if mnem in ("sw", "sh", "sb", "lw", "lh", "lhu", "lb", "lbu") and ops == 2:
        try:
            base_op = ins.getDefaultOperandRepresentation(1)
            if "(" in base_op and base_op.endswith(")"):
                offstr, basestr = base_op.split("(")
                basestr = basestr.rstrip(")")
                if offstr.startswith("-0x"):
                    off = -int(offstr[3:], 16)
                elif offstr.startswith("0x"):
                    off = int(offstr[2:], 16)
                else:
                    off = int(offstr)
                if basestr in last_lui:
                    base = last_lui[basestr]
                    combined = (base + off) & 0xFFFFFFFF
                    if LO <= combined < HI:
                        kind = "STORE" if mnem.startswith("s") else "load"
                        hits.setdefault(fa, []).append(
                            (str(ins.getAddress()), "{} ({})".format(mnem, kind),
                             "0x{:08X}".format(combined)))
        except:
            pass

print("=== {} ===".format(prog_name))
print("scanned {} instructions".format(total))
print("functions touching 0x{:08X}-0x{:08X}: {}".format(LO, HI, len(hits)))
for fa, refs in sorted(hits.items(), key=lambda kv: -len(kv[1])):
    func = fm.getFunctionAt(af.getAddress("{:x}".format(fa))) if fa else None
    fname = func.getName() if func else "?"
    stores = [r for r in refs if "STORE" in r[1]]
    loads = [r for r in refs if "load" in r[1]]
    addrs = [r for r in refs if "lui+addiu" in r[1]]
    print("\n  {} @ 0x{:08X}  stores={} loads={} addrs={}".format(
        fname, fa or 0, len(stores), len(loads), len(addrs)))
    for ia, kind, tgt in stores[:16]:
        print("    [W] {}  {}  {}".format(ia, kind, tgt))
    for ia, kind, tgt in loads[:8]:
        print("    [r] {}  {}  {}".format(ia, kind, tgt))
    for ia, kind, tgt in addrs[:4]:
        print("    [a] {}  {}  {}".format(ia, kind, tgt))
