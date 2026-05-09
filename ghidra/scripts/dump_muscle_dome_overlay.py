# @category Legaia
# @runtime Jython
#
# Dumps functions from the muscle dome overlay (overlay_muscle_dome.bin).
# Captured from Duckstation save state save 5 (Muscle Dome / Baka card battle) via extract-duckstation-overlay.py.
#
# Muscle Dome / Baka card battle system. FUN_801d8de8 (77 callers, 3028 bytes) is the top-level round dispatcher; FUN_801d5854 (47 callers, 6500 bytes) is the main game state machine; FUN_801d388c (39 callers, 7820 bytes) handles card resolution logic. Completely distinct from the other-game cluster (only 17 shared prologues with any other overlay).
#
# Run against the named overlay program:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_muscle_dome.bin -noanalysis \
#       -postScript /scripts/dump_muscle_dome_overlay.py
#
# Output files land in /scripts/funcs/overlay_muscle_dome_<addr>.txt

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801d8de8",  # FUN_801d8de8, inc=77, sz=3028
    "801d5854",  # FUN_801d5854, inc=47, sz=6500
    "801d388c",  # FUN_801d388c, inc=39, sz=7820
    "801d829c",  # FUN_801d829c, inc=24, sz=548
    "801d5718",  # FUN_801d5718, inc=18, sz=96
    "801f69d8",  # FUN_801f69d8, inc=14, sz=1
    "801de4c8",  # FUN_801de4c8, inc=13, sz=856
    "801de648",  # FUN_801de648, inc=13, sz=244
    "801f0348",  # FUN_801f0348, inc=11, sz=264
    "801db81c",  # FUN_801db81c, inc=10, sz=152
    "801d8d00",  # FUN_801d8d00, inc=9, sz=232
    "801da6b4",  # FUN_801da6b4, inc=9, sz=204
    "801d99bc",  # FUN_801d99bc, inc=8, sz=300
    "801db8b4",  # FUN_801db8b4, inc=8, sz=64
    "801dc0a0",  # FUN_801dc0a0, inc=7, sz=3596
    "801dfdf0",  # FUN_801dfdf0, inc=6, sz=656
    "801d8a88",  # FUN_801d8a88, inc=6, sz=632
    "801dceac",  # FUN_801dceac, inc=6, sz=512
    "801d32bc",  # FUN_801d32bc, inc=6, sz=392
    "801db8f4",  # FUN_801db8f4, inc=6, sz=208
    "801e2650",  # FUN_801e2650, inc=4, sz=780
    "801dbddc",  # FUN_801dbddc, inc=4, sz=232
    "801f69f0",  # FUN_801f69f0, inc=4, sz=4
    "801ddb30",  # FUN_801ddb30, inc=3, sz=2456
    "801daba4",  # FUN_801daba4, inc=3, sz=1408
    "801e1d98",  # FUN_801e1d98, inc=3, sz=1328
    "801dd864",  # FUN_801dd864, inc=3, sz=716
    "801f1ed4",  # FUN_801f1ed4, inc=3, sz=652
    "801db124",  # FUN_801db124, inc=3, sz=500
    "801dbb8c",  # FUN_801dbb8c, inc=3, sz=164
    "801dba04",  # FUN_801dba04, inc=3, sz=140
    "801f69fc",  # FUN_801f69fc, inc=3, sz=4
    "801f69f4",  # FUN_801f69f4, inc=3, sz=1
    "801eed1c",  # FUN_801eed1c, inc=2, sz=3272
    "801f2410",  # FUN_801f2410, inc=2, sz=2372
    "801efe44",  # FUN_801efe44, inc=2, sz=1284
    "801da780",  # FUN_801da780, inc=2, sz=1060
    "801da34c",  # FUN_801da34c, inc=2, sz=592
    "801f8228",  # FUN_801f8228, inc=2, sz=528
    "801d88cc",  # FUN_801d88cc, inc=2, sz=444
    "801dbc30",  # FUN_801dbc30, inc=2, sz=212
    "801d5778",  # FUN_801d5778, inc=2, sz=112
    "801d57e8",  # FUN_801d57e8, inc=2, sz=108
    "801f6a58",  # FUN_801f6a58, inc=2, sz=8
    "801f69ec",  # FUN_801f69ec, inc=2, sz=4
    "801f6a30",  # FUN_801f6a30, inc=2, sz=1
    "801f6a74",  # FUN_801f6a74, inc=2, sz=1
    "801e9fd4",  # FUN_801e9fd4, inc=1, sz=8456
    "801e805c",  # FUN_801e805c, inc=1, sz=4492
    "801d71b8",  # FUN_801d71b8, inc=1, sz=4324
    "801f0450",  # FUN_801f0450, inc=1, sz=3712
    "801e791c",  # FUN_801e791c, inc=1, sz=1856
    "801d9d3c",  # FUN_801d9d3c, inc=1, sz=1552
    "801f12d0",  # FUN_801f12d0, inc=1, sz=1320
    "801db318",  # FUN_801db318, inc=1, sz=1176
    "801e6968",  # FUN_801e6968, inc=1, sz=1052
    "801d84c0",  # FUN_801d84c0, inc=1, sz=1036
    "801dd0ac",  # FUN_801dd0ac, inc=1, sz=1028
    "801f76f4",  # FUN_801f76f4, inc=1, sz=844
    "801e6d84",  # FUN_801e6d84, inc=1, sz=824
    "801ec0dc",  # FUN_801ec0dc, inc=1, sz=776
    "801d3444",  # FUN_801d3444, inc=1, sz=772
    "801e752c",  # FUN_801e752c, inc=1, sz=760
    "801e1ab0",  # FUN_801e1ab0, inc=1, sz=744
    "801f2160",  # FUN_801f2160, inc=1, sz=688
    "801f3990",  # FUN_801f3990, inc=1, sz=676
    "801efbfc",  # FUN_801efbfc, inc=1, sz=584
    "801ef9e4",  # FUN_801ef9e4, inc=1, sz=536
    "801e7320",  # FUN_801e7320, inc=1, sz=524
    "801f8638",  # FUN_801f8638, inc=1, sz=444
    "801e70bc",  # FUN_801e70bc, inc=1, sz=404
    "801d9bbc",  # FUN_801d9bbc, inc=1, sz=384
    "801df570",  # FUN_801df570, inc=1, sz=328
    "801f7ebc",  # FUN_801f7ebc, inc=1, sz=328
    "801d3748",  # FUN_801d3748, inc=1, sz=324
    "801e93c8",  # FUN_801e93c8, inc=1, sz=316
    "801da59c",  # FUN_801da59c, inc=1, sz=280
    "801f3c34",  # FUN_801f3c34, inc=1, sz=264
    "801f87f4",  # FUN_801f87f4, inc=1, sz=264
    "801dbf9c",  # FUN_801dbf9c, inc=1, sz=260
    "801e7824",  # FUN_801e7824, inc=1, sz=248
    "801e91e8",  # FUN_801e91e8, inc=1, sz=244
    "801e92dc",  # FUN_801e92dc, inc=1, sz=236
    "801dbd04",  # FUN_801dbd04, inc=1, sz=216
    "801dbec4",  # FUN_801dbec4, inc=1, sz=216
    "801d9ae8",  # FUN_801d9ae8, inc=1, sz=212
    "801e7250",  # FUN_801e7250, inc=1, sz=208
    "801f7624",  # FUN_801f7624, inc=1, sz=208
    "801f45a4",  # FUN_801f45a4, inc=1, sz=152
    "801f8190",  # FUN_801f8190, inc=1, sz=152
    "801f80a0",  # FUN_801f80a0, inc=1, sz=124
    "801f452c",  # FUN_801f452c, inc=1, sz=120
    "801f7e4c",  # FUN_801f7e4c, inc=1, sz=112
    "801db7b0",  # FUN_801db7b0, inc=1, sz=108
    "801f8438",  # FUN_801f8438, inc=1, sz=100
    "801f7d38",  # FUN_801f7d38, inc=1, sz=88
    "801f7a54",  # FUN_801f7a54, inc=1, sz=72
    "801f92a4",  # FUN_801f92a4, inc=1, sz=68
    "801f9ba8",  # FUN_801f9ba8, inc=1, sz=68
    "801db9c4",  # FUN_801db9c4, inc=1, sz=64
    "801f7b28",  # FUN_801f7b28, inc=1, sz=64
    "801f8e3c",  # FUN_801f8e3c, inc=1, sz=36
    "801f8080",  # FUN_801f8080, inc=1, sz=32
    "801f7a40",  # FUN_801f7a40, inc=1, sz=20
    "801f7b1c",  # FUN_801f7b1c, inc=1, sz=12
    "801f816c",  # FUN_801f816c, inc=1, sz=12
    "801f8e60",  # FUN_801f8e60, inc=1, sz=12
    "801f6a08",  # FUN_801f6a08, inc=1, sz=4
    "801f6a10",  # thunk_EXT_FUN_8c000000, inc=1, sz=4
    "801f6a34",  # FUN_801f6a34, inc=1, sz=4
    "801f6a40",  # FUN_801f6a40, inc=1, sz=4
    "801f6c70",  # FUN_801f6c70, inc=1, sz=4
    "801f6d48",  # FUN_801f6d48, inc=1, sz=4
    "801f69e8",  # FUN_801f69e8, inc=1, sz=1
    "801f69f8",  # FUN_801f69f8, inc=1, sz=1
    "801f6a00",  # FUN_801f6a00, inc=1, sz=1
    "801f6a18",  # FUN_801f6a18, inc=1, sz=1
    "801f6a3c",  # FUN_801f6a3c, inc=1, sz=1
    "801f6a84",  # FUN_801f6a84, inc=1, sz=1
    "801e295c",  # FUN_801e295c, inc=0, sz=16396
    "801d0748",  # FUN_801d0748, inc=0, sz=11124
    "801ec3e4",  # FUN_801ec3e4, inc=0, sz=10008
    "801e09f8",  # FUN_801e09f8, inc=0, sz=4280
    "801dea50",  # FUN_801dea50, inc=0, sz=2848
    "801e9504",  # FUN_801e9504, inc=0, sz=2768
    "801f30c4",  # FUN_801f30c4, inc=0, sz=2252
    "801f3d3c",  # FUN_801f3d3c, inc=0, sz=1892
    "801df6b8",  # FUN_801df6b8, inc=0, sz=1848
    "801f7088",  # FUN_801f7088, inc=0, sz=1436
    "801f19ec",  # FUN_801f19ec, inc=0, sz=1256
    "801f8a34",  # FUN_801f8a34, inc=0, sz=792
    "801f2e10",  # FUN_801f2e10, inc=0, sz=692
    "801e22c8",  # FUN_801e22c8, inc=0, sz=604
    "801dd4b0",  # FUN_801dd4b0, inc=0, sz=516
    "801f17f8",  # FUN_801f17f8, inc=0, sz=500
    "801dd6b4",  # FUN_801dd6b4, inc=0, sz=432
    "801f849c",  # FUN_801f849c, inc=0, sz=412
    "801f90dc",  # FUN_801f90dc, inc=0, sz=328
    "801f88fc",  # FUN_801f88fc, inc=0, sz=312
    "801e2524",  # FUN_801e2524, inc=0, sz=300
    "801f8d4c",  # FUN_801f8d4c, inc=0, sz=240
    "801f8e6c",  # FUN_801f8e6c, inc=0, sz=188
    "801f8f28",  # FUN_801f8f28, inc=0, sz=184
    "801dba90",  # FUN_801dba90, inc=0, sz=156
    "801f44a0",  # FUN_801f44a0, inc=0, sz=140
    "801f8004",  # FUN_801f8004, inc=0, sz=124
    "801f7a9c",  # FUN_801f7a9c, inc=0, sz=120
    "801f811c",  # FUN_801f811c, inc=0, sz=104
]

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
prog_name = prog.getName()
fm = prog.getFunctionManager()
listing = prog.getListing()
af = prog.getAddressFactory()
mem = prog.getMemory()
monitor = ConsoleTaskMonitor()

decomp = DecompInterface()
opts = DecompileOptions()
decomp.setOptions(opts)
decomp.openProgram(prog)


def out_path_for(addr_str):
    label = prog_name.replace(".bin", "").replace(".", "_")
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    block = mem.getBlock(addr)
    return block is not None


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] {} not an address".format(addr_str))
        return
    if not in_program(addr):
        return
    func = fm.getFunctionContaining(addr) or fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function at {} in {}".format(addr_str, prog_name))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = out_path_for(addr_str)
    fh = open(out_path, "w")
    try:
        fh.write("== {} {} (entry={}) [{}] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint(), prog_name))
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
            fh.write("(decompile exception: {})\n".format(e))
    finally:
        fh.close()
    print("wrote {}".format(out_path))


for t in TARGETS:
    dump(t)

print("done [{}]".format(prog_name))
