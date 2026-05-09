# @category Legaia
# @runtime Jython
#
# Dumps functions from the baka fighter overlay (overlay_baka_fighter.bin).
# Captured from Duckstation save state save 3 (Baka Fighter / fighting minigame) via extract-duckstation-overlay.py.
#
# Baka Fighter minigame (other5.lzs). FUN_801d5ed0 (49 callers, 1072 bytes) is the main battle-round dispatcher. FUN_801d3b18 (1068 bytes) and FUN_801d4c50 handle AI and player input respectively.
#
# Run against the named overlay program:
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_baka_fighter.bin -noanalysis \
#       -postScript /scripts/dump_baka_fighter_overlay.py
#
# Output files land in /scripts/funcs/overlay_baka_fighter_<addr>.txt

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

TARGETS = [
    "801d5ed0",  # FUN_801d5ed0, inc=49, sz=1072
    "801de4c8",  # FUN_801de4c8, inc=16, sz=384
    "801de648",  # FUN_801de648, inc=16, sz=80
    "801d6e04",  # FUN_801d6e04, inc=15, sz=88
    "801e9b3c",  # FUN_801e9b3c, inc=8, sz=208
    "801d6a18",  # FUN_801d6a18, inc=7, sz=416
    "801d6480",  # FUN_801d6480, inc=7, sz=252
    "801d4c50",  # FUN_801d4c50, inc=6, sz=424
    "801d657c",  # FUN_801d657c, inc=5, sz=124
    "801d3b18",  # FUN_801d3b18, inc=4, sz=1068
    "801d5c7c",  # FUN_801d5c7c, inc=4, sz=596
    "801d4df8",  # FUN_801d4df8, inc=4, sz=464
    "801d21fc",  # FUN_801d21fc, inc=4, sz=416
    "801e3764",  # FUN_801e3764, inc=4, sz=304
    "801e3658",  # FUN_801e3658, inc=4, sz=268
    "801e3894",  # FUN_801e3894, inc=4, sz=240
    "801d6d60",  # FUN_801d6d60, inc=4, sz=164
    "801d6710",  # FUN_801d6710, inc=4, sz=96
    "801d69a8",  # FUN_801d69a8, inc=4, sz=60
    "801ed710",  # FUN_801ed710, inc=3, sz=2032
    "801d6bb8",  # FUN_801d6bb8, inc=3, sz=260
    "801eca08",  # FUN_801eca08, inc=3, sz=256
    "801d6770",  # FUN_801d6770, inc=3, sz=128
    "801d6910",  # FUN_801d6910, inc=3, sz=44
    "801d239c",  # FUN_801d239c, inc=2, sz=1676
    "801d5a24",  # FUN_801d5a24, inc=2, sz=600
    "801e9dc8",  # FUN_801e9dc8, inc=2, sz=412
    "801d6e5c",  # FUN_801d6e5c, inc=2, sz=188
    "801d6660",  # FUN_801d6660, inc=2, sz=176
    "801d65f8",  # FUN_801d65f8, inc=2, sz=104
    "801d59d4",  # FUN_801d59d4, inc=2, sz=80
    "801d69e4",  # FUN_801d69e4, inc=2, sz=52
    "801d693c",  # FUN_801d693c, inc=2, sz=44
    "801d6968",  # FUN_801d6968, inc=2, sz=44
    "801d6994",  # FUN_801d6994, inc=2, sz=20
    "801d8de8",  # FUN_801d8de8, inc=2, sz=12
    "801e5b4c",  # FUN_801e5b4c, inc=1, sz=2228
    "801d2afc",  # FUN_801d2afc, inc=1, sz=2196
    "801e3e00",  # FUN_801e3e00, inc=1, sz=1648
    "801ceb84",  # FUN_801ceb84, inc=1, sz=1160
    "801e3984",  # FUN_801e3984, inc=1, sz=1148
    "801f69ec",  # FUN_801f69ec, inc=1, sz=860
    "801f6d48",  # FUN_801f6d48, inc=1, sz=832
    "801f1278",  # FUN_801f1278, inc=1, sz=804
    "801d553c",  # FUN_801d553c, inc=1, sz=640
    "801dd310",  # FUN_801dd310, inc=1, sz=436
    "801e7448",  # FUN_801e7448, inc=1, sz=404
    "801d6f44",  # FUN_801d6f44, inc=1, sz=384
    "801d487c",  # FUN_801d487c, inc=1, sz=364
    "801ead98",  # FUN_801ead98, inc=1, sz=300
    "801d57bc",  # FUN_801d57bc, inc=1, sz=292
    "801d3a14",  # FUN_801d3a14, inc=1, sz=260
    "801e75dc",  # FUN_801e75dc, inc=1, sz=248
    "801d58f0",  # FUN_801d58f0, inc=1, sz=228
    "801d2a28",  # FUN_801d2a28, inc=1, sz=212
    "801d6cbc",  # FUN_801d6cbc, inc=1, sz=164
    "801de190",  # FUN_801de190, inc=1, sz=164
    "801d84b4",  # FUN_801d84b4, inc=1, sz=128
    "801de004",  # FUN_801de004, inc=1, sz=128
    "801daa50",  # FUN_801daa50, inc=1, sz=56
    "801dba20",  # FUN_801dba20, inc=1, sz=36
    "801d58e0",  # FUN_801d58e0, inc=1, sz=16
    "801d6300",  # FUN_801d6300, inc=1, sz=16
    "801db8ec",  # FUN_801db8ec, inc=1, sz=16
    "801d9e1c",  # FUN_801d9e1c, inc=1, sz=12
    "801dbc20",  # FUN_801dbc20, inc=1, sz=1
    "801cf388",  # FUN_801cf388, inc=0, sz=11892
    "801e76d4",  # FUN_801e76d4, inc=0, sz=9320
    "801f7088",  # FUN_801f7088, inc=0, sz=2580
    "801d3f44",  # FUN_801d3f44, inc=0, sz=2360
    "801ef2b0",  # FUN_801ef2b0, inc=0, sz=1456
    "801d3468",  # FUN_801d3468, inc=0, sz=1452
    "801d4fc8",  # FUN_801d4fc8, inc=0, sz=1396
    "801f7a9c",  # FUN_801f7a9c, inc=0, sz=1384
    "801e4794",  # FUN_801e4794, inc=0, sz=1220
    "801f849c",  # FUN_801f849c, inc=0, sz=1120
    "801e6b34",  # FUN_801e6b34, inc=0, sz=1084
    "801e4d8c",  # FUN_801e4d8c, inc=0, sz=968
    "801f811c",  # FUN_801f811c, inc=0, sz=896
    "801cf00c",  # FUN_801cf00c, inc=0, sz=892
    "801e5338",  # FUN_801e5338, inc=0, sz=804
    "801f8a34",  # FUN_801f8a34, inc=0, sz=792
    "801ee328",  # FUN_801ee328, inc=0, sz=684
    "801ef014",  # FUN_801ef014, inc=0, sz=668
    "801ee094",  # FUN_801ee094, inc=0, sz=660
    "801e9f64",  # FUN_801e9f64, inc=0, sz=644
    "801d49e8",  # FUN_801d49e8, inc=0, sz=616
    "801e6f70",  # FUN_801e6f70, inc=0, sz=608
    "801e6400",  # FUN_801e6400, inc=0, sz=556
    "801de840",  # FUN_801de840, inc=0, sz=552
    "801ddc20",  # FUN_801ddc20, inc=0, sz=532
    "801e6778",  # FUN_801e6778, inc=0, sz=524
    "801f3d3c",  # FUN_801f3d3c, inc=0, sz=524
    "801e6984",  # FUN_801e6984, inc=0, sz=432
    "801edf00",  # FUN_801edf00, inc=0, sz=404
    "801ed590",  # FUN_801ed590, inc=0, sz=384
    "801d6310",  # FUN_801d6310, inc=0, sz=368
    "801e71d0",  # FUN_801e71d0, inc=0, sz=364
    "801e4470",  # FUN_801e4470, inc=0, sz=332
    "801e662c",  # FUN_801e662c, inc=0, sz=332
    "801f90dc",  # FUN_801f90dc, inc=0, sz=328
    "801e5a08",  # FUN_801e5a08, inc=0, sz=324
    "801f1138",  # FUN_801f1138, inc=0, sz=320
    "801f88fc",  # FUN_801f88fc, inc=0, sz=312
    "801e4c58",  # FUN_801e4c58, inc=0, sz=308
    "801f3990",  # FUN_801f3990, inc=0, sz=308
    "801f159c",  # FUN_801f159c, inc=0, sz=292
    "801d67f0",  # FUN_801d67f0, inc=0, sz=288
    "801f8d4c",  # FUN_801f8d4c, inc=0, sz=288
    "801f16c0",  # FUN_801f16c0, inc=0, sz=280
    "801f8004",  # FUN_801f8004, inc=0, sz=280
    "801dd9d4",  # FUN_801dd9d4, inc=0, sz=276
    "801dde34",  # FUN_801dde34, inc=0, sz=276
    "801de084",  # FUN_801de084, inc=0, sz=268
    "801e733c",  # FUN_801e733c, inc=0, sz=268
    "801e58a8",  # FUN_801e58a8, inc=0, sz=264
    "801f3c34",  # FUN_801f3c34, inc=0, sz=264
    "801f1e48",  # FUN_801f1e48, inc=0, sz=260
    "801d3390",  # FUN_801d3390, inc=0, sz=216
    "801e5668",  # FUN_801e5668, inc=0, sz=212
    "801f1fdc",  # FUN_801f1fdc, inc=0, sz=212
    "801de2b0",  # FUN_801de2b0, inc=0, sz=204
    "801f1950",  # FUN_801f1950, inc=0, sz=204
    "801f1890",  # FUN_801f1890, inc=0, sz=192
    "801de698",  # FUN_801de698, inc=0, sz=188
    "801f8e6c",  # FUN_801f8e6c, inc=0, sz=188
    "801f0adc",  # FUN_801f0adc, inc=0, sz=184
    "801f17d8",  # FUN_801f17d8, inc=0, sz=184
    "801f1d90",  # FUN_801f1d90, inc=0, sz=184
    "801f8f28",  # FUN_801f8f28, inc=0, sz=184
    "801e573c",  # FUN_801e573c, inc=0, sz=180
    "801f1ab0",  # FUN_801f1ab0, inc=0, sz=180
    "801ecd0c",  # FUN_801ecd0c, inc=0, sz=168
    "801ddf48",  # FUN_801ddf48, inc=0, sz=156
    "801de3e0",  # FUN_801de3e0, inc=0, sz=152
    "801f45a4",  # FUN_801f45a4, inc=0, sz=152
    "801f1a1c",  # FUN_801f1a1c, inc=0, sz=148
    "801f44a0",  # FUN_801f44a0, inc=0, sz=140
    "801de7bc",  # FUN_801de7bc, inc=0, sz=132
    "801f20b0",  # FUN_801f20b0, inc=0, sz=132
    "801ee90c",  # FUN_801ee90c, inc=0, sz=128
    "801f03f0",  # FUN_801f03f0, inc=0, sz=128
    "801f2134",  # FUN_801f2134, inc=0, sz=128
    "801de234",  # FUN_801de234, inc=0, sz=124
    "801f452c",  # FUN_801f452c, inc=0, sz=120
    "801e5834",  # FUN_801e5834, inc=0, sz=116
    "801f1b64",  # FUN_801f1b64, inc=0, sz=116
    "801de754",  # FUN_801de754, inc=0, sz=104
    "801ea9b0",  # FUN_801ea9b0, inc=0, sz=100
    "801ee5d4",  # FUN_801ee5d4, inc=0, sz=100
    "801ed308",  # FUN_801ed308, inc=0, sz=88
    "801eed58",  # FUN_801eed58, inc=0, sz=88
    "801de478",  # FUN_801de478, inc=0, sz=80
    "801f1cd8",  # FUN_801f1cd8, inc=0, sz=72
    "801f1d48",  # FUN_801f1d48, inc=0, sz=72
    "801e57f0",  # FUN_801e57f0, inc=0, sz=68
    "801d6f18",  # FUN_801d6f18, inc=0, sz=44
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
