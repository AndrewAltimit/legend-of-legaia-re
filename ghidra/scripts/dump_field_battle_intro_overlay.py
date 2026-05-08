# @category Legaia
# @runtime Jython
#
# Dumps field_battle_intro overlay functions by size.
# Output naming: overlay_field_battle_intro_<addr>.txt.
#
# overlay_field_battle_intro.bin (125 functions) was captured during the
# 3D camera spin that plays between the field and the battle load -- a moment
# when the field overlay is still resident but the battle overlay has not yet
# loaded. It is a partial 0897 code image: FUN_801DE840 (main dispatcher)
# and FUN_801EAD98 (dev menu) are absent, but the core world/field helpers
# are present. Unique functions not found in other captures:
#   FUN_801D081C -- (1288 bytes)
#   FUN_801D0370 -- (1196 bytes)
#   FUN_801CF1B0 -- (1036 bytes)
#   FUN_801CFDA0 -- (964 bytes)
#   FUN_801D11D0 -- (916 bytes)
#   FUN_801D0E54 -- (892 bytes)
#   FUN_801D1564 -- (804 bytes)
#   FUN_801CF5BC -- (752 bytes)
#   FUN_801D1A20 -- (692 bytes)
#   FUN_801D0164 -- (524 bytes)
#   FUN_801CFBB4 -- (492 bytes)
#   FUN_801D1888 -- (408 bytes)
#   FUN_801D0D24 -- (304 bytes)
#
# Run against overlay_field_battle_intro.bin:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_field_battle_intro.bin -noanalysis \
#       -postScript /scripts/dump_field_battle_intro_overlay.py

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801e76d4",  # 9320 bytes  -- world map controller
    "801dc0bc",  # 4692 bytes
    "801d6704",  # 3604 bytes  -- MAIN INIT
    "801dab90",  # 2432 bytes
    "801e5b4c",  # 2228 bytes
    "801d84d0",  # 2176 bytes
    "801ed710",  # 2032 bytes  -- MES renderer
    "801e3e00",  # 1648 bytes
    "801d9e1c",  # 1396 bytes
    "801d081c",  # 1288 bytes  -- unique to field_battle_intro
    "801e4794",  # 1220 bytes
    "801d0370",  # 1196 bytes  -- unique to field_battle_intro
    "801e3984",  # 1148 bytes
    "801d31b0",  # 1148 bytes
    "801e6b34",  # 1084 bytes
    "801cf1b0",  # 1036 bytes
    "801db510",  # 988 bytes
    "801e4d8c",  # 968 bytes
    "801cfda0",  # 964 bytes   -- unique to field_battle_intro
    "801d11d0",  # 916 bytes   -- unique to field_battle_intro
    "801d0e54",  # 892 bytes   -- unique to field_battle_intro
    "801d1564",  # 804 bytes   -- unique to field_battle_intro
    "801cf5bc",  # 752 bytes   -- unique to field_battle_intro
    "801d1a20",  # 692 bytes   -- unique to field_battle_intro
    "801d0164",  # 524 bytes   -- unique to field_battle_intro
    "801cfbb4",  # 492 bytes   -- unique to field_battle_intro
    "801d1888",  # 408 bytes   -- unique to field_battle_intro
    "801d0d24",  # 304 bytes   -- unique to field_battle_intro
    "801d1cd4",  # 40 bytes    -- unique to field_battle_intro
]

OUT_DIR = "/scripts/funcs"
PREFIX = "overlay_field_battle_intro_"

try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] not an address: " + addr_str)
        return
    func = fm.getFunctionContaining(addr)
    if func is None:
        func = fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function for " + addr_str)
        return
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    out_path = os.path.join(OUT_DIR, PREFIX + addr_str + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) [overlay_field_battle_intro base=0x801C0000] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint()))
        fh.write("size={} bytes, {} instructions\n\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        fh.write("\n--- DECOMPILED ---\n")
        try:
            res = decomp.decompileFunction(func, 60, monitor)
            if res.decompileCompleted():
                fh.write(res.getDecompiledFunction().getC())
            else:
                fh.write("(decompile failed: {})\n".format(res.getErrorMessage()))
        except Exception as e:
            fh.write("(decompile exception: {})\n".format(str(e)))
    print("wrote " + out_path)


for t in TARGETS:
    dump(t)

print("done")
