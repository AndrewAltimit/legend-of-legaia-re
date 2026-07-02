# @category Legaia
# @runtime Jython
#
# Dumps functions from the slot machine overlay (overlay_slot_machine.bin).
# Captured from Duckstation save state save 2 (slot machine / Wild Card) via extract-duckstation-overlay.py.
#
# Slot machine minigame (Wild Card). Shares 138/155 prologues with fishing, confirming all are part of the same minigame-hub overlay family. Unique functions handle coin input, reel-stop, and bonus-game logic.
#
# Run against the named overlay program:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_slot_machine.bin -noanalysis \
#       -postScript /scripts/dump_slot_machine_overlay.py
#
# Output files land in /scripts/funcs/overlay_slot_machine_<addr>.txt

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801cec94",  # FUN_801cec94 overlay init entry: LCG seed + balance from coin bank
    "801de4c8",  # FUN_801de4c8, inc=16, sz=384
    "801de648",  # FUN_801de648, inc=16, sz=80
    "801d63b0",  # FUN_801d63b0, inc=14, sz=1036
    "801d30cc",  # FUN_801d30cc, inc=13, sz=44
    "801d3230",  # FUN_801d3230, inc=10, sz=152
    "801d7a5c",  # FUN_801d7a5c, inc=8, sz=348
    "801e9b3c",  # FUN_801e9b3c, inc=8, sz=208
    "801d2aa4",  # FUN_801d2aa4, inc=4, sz=540
    "801d2440",  # FUN_801d2440, inc=4, sz=332
    "801e3764",  # FUN_801e3764, inc=4, sz=304
    "801e3658",  # FUN_801e3658, inc=4, sz=268
    "801e3894",  # FUN_801e3894, inc=4, sz=240
    "801ed710",  # FUN_801ed710, inc=3, sz=2032
    "801d2cc0",  # FUN_801d2cc0, inc=3, sz=1036
    "801d2114",  # FUN_801d2114, inc=3, sz=812
    "801db8ec",  # FUN_801db8ec, inc=3, sz=308
    "801eca08",  # FUN_801eca08, inc=3, sz=256
    "801dab90",  # FUN_801dab90, inc=2, sz=2432
    "801cfccc",  # caseD_5a, inc=2, sz=700
    "801dbc20",  # FUN_801dbc20, inc=2, sz=636
    "801d5a24",  # FUN_801d5a24, inc=2, sz=520
    "801dba20",  # FUN_801dba20, inc=2, sz=512
    "801e9dc8",  # FUN_801e9dc8, inc=2, sz=412
    "801d069c",  # FUN_801d069c, inc=2, sz=256
    "801d32c8",  # FUN_801d32c8, inc=2, sz=184
    "801d7964",  # FUN_801d7964, inc=2, sz=124
    "801d79e0",  # FUN_801d79e0, inc=2, sz=124
    "801d7bb8",  # FUN_801d7bb8, inc=2, sz=120
    "801d8de8",  # FUN_801d8de8, inc=2, sz=16
    "801e5b4c",  # FUN_801e5b4c, inc=1, sz=2228
    "801d13e8",  # FUN_801d13e8, inc=1, sz=1804
    "801e3e00",  # FUN_801e3e00, inc=1, sz=1648
    "801d1af4",  # FUN_801d1af4, inc=1, sz=1568
    "801d9e1c",  # FUN_801d9e1c, inc=1, sz=1396
    "801cfff0",  # FUN_801cfff0, inc=1, sz=1380
    "801d08e4",  # FUN_801d08e4, inc=1, sz=1336
    "801e3984",  # FUN_801e3984, inc=1, sz=1148
    "801d0fa8",  # FUN_801d0fa8, inc=1, sz=1052
    "801d67bc",  # FUN_801d67bc, inc=1, sz=1024
    "801d258c",  # FUN_801d258c, inc=1, sz=904
    "801d6028",  # FUN_801d6028, inc=1, sz=904
    "801f69ec",  # FUN_801f69ec, inc=1, sz=860
    "801f6d48",  # FUN_801f6d48, inc=1, sz=832
    "801f1278",  # FUN_801f1278, inc=1, sz=804
    "801d76e0",  # FUN_801d76e0, inc=1, sz=480
    "801dd310",  # FUN_801dd310, inc=1, sz=436
    "801e7448",  # FUN_801e7448, inc=1, sz=404
    "801d2914",  # FUN_801d2914, inc=1, sz=400
    "801d0e1c",  # FUN_801d0e1c, inc=1, sz=396
    "801da390",  # FUN_801da390, inc=1, sz=396
    "801d0554",  # FUN_801d0554, inc=1, sz=328
    "801daa50",  # FUN_801daa50, inc=1, sz=320
    "801d58f0",  # FUN_801d58f0, inc=1, sz=308
    "801ead98",  # FUN_801ead98, inc=1, sz=300
    "801d3380",  # FUN_801d3380, inc=1, sz=252
    "801e75dc",  # FUN_801e75dc, inc=1, sz=248
    "801d317c",  # FUN_801d317c, inc=1, sz=180
    "801de190",  # FUN_801de190, inc=1, sz=164
    "801d73b8",  # FUN_801d73b8, inc=1, sz=152
    "801d7d44",  # FUN_801d7d44, inc=1, sz=148
    "801d30f8",  # FUN_801d30f8, inc=1, sz=132
    "801de004",  # FUN_801de004, inc=1, sz=128
    "801d7dd8",  # FUN_801d7dd8, inc=1, sz=88
    "801d84b4",  # FUN_801d84b4, inc=1, sz=68
    "801d13c4",  # FUN_801d13c4, inc=1, sz=36
    "801e76d4",  # FUN_801e76d4, inc=0, sz=9320
    "801dc0bc",  # FUN_801dc0bc, inc=0, sz=4692
    "801cf0d8",  # FUN_801cf0d8, inc=0, sz=3060
    "801f7088",  # FUN_801f7088, inc=0, sz=2580
    "801d4948",  # FUN_801d4948, inc=0, sz=2384
    "801ef2b0",  # FUN_801ef2b0, inc=0, sz=1456
    "801f7a9c",  # FUN_801f7a9c, inc=0, sz=1384
    "801e4794",  # FUN_801e4794, inc=0, sz=1220
    "801f849c",  # FUN_801f849c, inc=0, sz=1120
    "801d5298",  # FUN_801d5298, inc=0, sz=1100
    "801e6b34",  # FUN_801e6b34, inc=0, sz=1084
    "801d5c2c",  # FUN_801d5c2c, inc=0, sz=1020
    "801db510",  # FUN_801db510, inc=0, sz=988
    "801e4d8c",  # FUN_801e4d8c, inc=0, sz=968
    "801f811c",  # FUN_801f811c, inc=0, sz=896
    "801d6bbc",  # FUN_801d6bbc, inc=0, sz=852
    "801e5338",  # FUN_801e5338, inc=0, sz=804
    "801f8a34",  # FUN_801f8a34, inc=0, sz=792
    "801ee328",  # FUN_801ee328, inc=0, sz=684
    "801ef014",  # FUN_801ef014, inc=0, sz=668
    "801ee094",  # FUN_801ee094, inc=0, sz=660
    "801e9f64",  # FUN_801e9f64, inc=0, sz=644
    "801e6f70",  # FUN_801e6f70, inc=0, sz=608
    "801e6400",  # FUN_801e6400, inc=0, sz=556
    "801de840",  # FUN_801de840, inc=0, sz=552
    "801dbe9c",  # FUN_801dbe9c, inc=0, sz=544
    "801ddc20",  # FUN_801ddc20, inc=0, sz=532
    "801e6778",  # FUN_801e6778, inc=0, sz=524
    "801f3d3c",  # FUN_801f3d3c, inc=0, sz=524
    "801e6984",  # FUN_801e6984, inc=0, sz=432
    "801edf00",  # FUN_801edf00, inc=0, sz=404
    "801ed590",  # FUN_801ed590, inc=0, sz=384
    "801e71d0",  # FUN_801e71d0, inc=0, sz=364
    "801e4470",  # FUN_801e4470, inc=0, sz=332
    "801e662c",  # FUN_801e662c, inc=0, sz=332
    "801d079c",  # FUN_801d079c, inc=0, sz=328
    "801f90dc",  # FUN_801f90dc, inc=0, sz=328
    "801e5a08",  # FUN_801e5a08, inc=0, sz=324
    "801da7f0",  # FUN_801da7f0, inc=0, sz=320
    "801f1138",  # FUN_801f1138, inc=0, sz=320
    "801f88fc",  # FUN_801f88fc, inc=0, sz=312
    "801e4c58",  # FUN_801e4c58, inc=0, sz=308
    "801f3990",  # FUN_801f3990, inc=0, sz=308
    "801f159c",  # FUN_801f159c, inc=0, sz=292
    "801f8d4c",  # FUN_801f8d4c, inc=0, sz=288
    "801d72a0",  # FUN_801d72a0, inc=0, sz=280
    "801f16c0",  # FUN_801f16c0, inc=0, sz=280
    "801f8004",  # FUN_801f8004, inc=0, sz=280
    "801dd9d4",  # FUN_801dd9d4, inc=0, sz=276
    "801dde34",  # FUN_801dde34, inc=0, sz=276
    "801de084",  # FUN_801de084, inc=0, sz=268
    "801e733c",  # FUN_801e733c, inc=0, sz=268
    "801e58a8",  # FUN_801e58a8, inc=0, sz=264
    "801f3c34",  # FUN_801f3c34, inc=0, sz=264
    "801da51c",  # FUN_801da51c, inc=0, sz=260
    "801f1e48",  # FUN_801f1e48, inc=0, sz=260
    "801d9c3c",  # FUN_801d9c3c, inc=0, sz=244
    "801d9d30",  # FUN_801d9d30, inc=0, sz=236
    "801e5668",  # FUN_801e5668, inc=0, sz=212
    "801f1fdc",  # FUN_801f1fdc, inc=0, sz=212
    "801d71d4",  # FUN_801d71d4, inc=0, sz=204
    "801de2b0",  # FUN_801de2b0, inc=0, sz=204
    "801f1950",  # FUN_801f1950, inc=0, sz=204
    "801d7c84",  # FUN_801d7c84, inc=0, sz=192
    "801f1890",  # FUN_801f1890, inc=0, sz=192
    "801de698",  # FUN_801de698, inc=0, sz=188
    "801f8e6c",  # FUN_801f8e6c, inc=0, sz=188
    "801f0adc",  # FUN_801f0adc, inc=0, sz=184
    "801f17d8",  # FUN_801f17d8, inc=0, sz=184
    "801f1d90",  # FUN_801f1d90, inc=0, sz=184
    "801f8f28",  # FUN_801f8f28, inc=0, sz=184
    "801d7528",  # FUN_801d7528, inc=0, sz=180
    "801e573c",  # FUN_801e573c, inc=0, sz=180
    "801f1ab0",  # FUN_801f1ab0, inc=0, sz=180
    "801ecd0c",  # FUN_801ecd0c, inc=0, sz=168
    "801d6f90",  # FUN_801d6f90, inc=0, sz=160
    "801ddf48",  # FUN_801ddf48, inc=0, sz=156
    "801de3e0",  # FUN_801de3e0, inc=0, sz=152
    "801f45a4",  # FUN_801f45a4, inc=0, sz=152
    "801d712c",  # FUN_801d712c, inc=0, sz=148
    "801f1a1c",  # FUN_801f1a1c, inc=0, sz=148
    "801f44a0",  # FUN_801f44a0, inc=0, sz=140
    "801de7bc",  # FUN_801de7bc, inc=0, sz=132
    "801f20b0",  # FUN_801f20b0, inc=0, sz=132
    "801d6f10",  # FUN_801d6f10, inc=0, sz=128
    "801d75dc",  # FUN_801d75dc, inc=0, sz=128
    "801ee90c",  # FUN_801ee90c, inc=0, sz=128
    "801f03f0",  # FUN_801f03f0, inc=0, sz=128
    "801f2134",  # FUN_801f2134, inc=0, sz=128
    "801de234",  # FUN_801de234, inc=0, sz=124
    "801d74b0",  # FUN_801d74b0, inc=0, sz=120
    "801d78ec",  # FUN_801d78ec, inc=0, sz=120
    "801f452c",  # FUN_801f452c, inc=0, sz=120
    "801e5834",  # FUN_801e5834, inc=0, sz=116
    "801f1b64",  # FUN_801f1b64, inc=0, sz=116
    "801de754",  # FUN_801de754, inc=0, sz=104
    "801ea9b0",  # FUN_801ea9b0, inc=0, sz=100
    "801ee5d4",  # FUN_801ee5d4, inc=0, sz=100
    "801ed308",  # FUN_801ed308, inc=0, sz=88
    "801eed58",  # FUN_801eed58, inc=0, sz=88
    "801d7c30",  # FUN_801d7c30, inc=0, sz=84
    "801de478",  # FUN_801de478, inc=0, sz=80
    "801f1cd8",  # FUN_801f1cd8, inc=0, sz=72
    "801f1d48",  # FUN_801f1d48, inc=0, sz=72
    "801e57f0",  # FUN_801e57f0, inc=0, sz=68
    "801d70ec",  # FUN_801d70ec, inc=0, sz=64
    "801f1c88",  # FUN_801f1c88, inc=0, sz=40
    "801f1cb0",  # FUN_801f1cb0, inc=0, sz=40
    "801f1d20",  # FUN_801f1d20, inc=0, sz=40
    "801ddfe4",  # FUN_801ddfe4, inc=0, sz=32
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
