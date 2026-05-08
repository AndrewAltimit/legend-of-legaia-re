# @category Legaia
# @runtime Jython
#
# Dumps disassembly + decompiled C for a hardcoded list of function entry
# points. Output goes to /scripts/funcs/<addr>.txt.

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

PROGRAM = "SCUS_942.54"  # default; override with OVERLAY_PROGRAM=overlay.bin per-run

TARGETS = [
    # Asset dispatcher + sub-asset loader chain
    "8003e8a8",  # sub-asset address resolver (PROT[id], sub_id)
    "8003e800",  # sub-asset loader (dst, addr, sub_id)
    "8003eb98",  # by-index loader (on-disc -> in-RAM)
    "8003f128",  # disc read kickoff
    "8005c328",  # CD position setup
    "8005c42c",  # CD base lookup
    "80020224",  # descriptor-pair walker (caller #1 of FUN_8001f05c)
    "8002541c",  # streaming-asset driver (handles types 10/0xF/0x14)
    "800255b8",  # called from FUN_8002541c right before table use
    "80058104",  # init/teardown helper called by FUN_8002541c
    "80017888",  # malloc helper used by FUN_8002541c
    "800198e0",  # per-entry cleanup helper

    # Battle archive loaders
    "80052fa0",
    "800542c8",

    # Asset-table init
    "8001e1b4",  # writes _DAT_8007b85c (in-RAM asset table base)

    # TMD pipeline
    "80026b4c",  # TMD handler dispatched from FUN_8001f05c
    "800268dc",  # TMD primitive iterator

    # Boot-time TOC loader cluster (PROT.DAT first 3 sectors -> 0x801C70F0)
    "8003e4e8",  # multi-region init: 0x801C70F0 + 0x801C88F0 + 0x801C8970
    "8003dda0",  # single-region init: 0x801C70F0
    "8003e68c",  # short stub referencing 0x801C70F0
    "8003e360",  # sibling allocator at 0x801C88F0
    "8003e6bc",
    "8003e9c0",
    "8003efe8",  # caller of FUN_8003e4e8
    "8003f08c",  # caller of FUN_8003e4e8
    "8003d3c4",  # file-into-buffer reader, sibling of FUN_8003e4e8

    # Debug-flag region readers (0x8007B98C word that holds byte 0x8007B98F)
    "80016230",  # writes 0x8007BC3C/4C; reads 0x8007B98C, B9A8, B9AC, B9DC
    "80016444",  # reads 0x8007B98C, B924, BC00, BC7C, B9CC
    "800173bc",  # reads 0x8007B98C; writes 0x8007BCAC
    "800179c0",
    "80016b6c",  # heavy debug-area consumer
    "8003aeb0",  # debug-state init/reset
    "80018db0",  # 0x8007BC70 R/W
    "800188c8",  # debug-area reads (B852, B87C, B6D0)
    "80030628",  # consecutive byte reads in BB88-BB91 (packed debug flags)
    "8001822c",  # reader of 0x8007B7C0 (debug dispatch trigger)

    # Move-table consumer (Tactical Arts)
    "800204f8",  # reads _DAT_8007b888 (MOVE) and _DAT_8007b840 (MOVE2)
    "80020740",  # pre-tick helper invoked when actor flag 0x1000 set

    # Game-mode state-machine handlers (table at 0x8007078C, 28 entries)
    "80025eec",  # default per-frame handler shared by 13 modes
    "80025c68",  # mode 0 (CONFIG init)
    "80025b64",  # mode 2 (MAIN init)
    "80025da0",  # mode 12 (MAPDSIP MODE INIT)
    "80025f2c",  # mode 13 (MAPDSIP MODE = field display)
    "80025e68",  # mode 8 (EFECT init)
    "80025f74",  # mode 23 (CARD MODE)
    "80025980",  # mode 24 (OTHER) init
    "80025fb4",  # mode 26 (STR) init
    "8001dcf8",  # boot-time mode initializer
    "8001c93c",  # mode-table reader / per-frame dispatcher candidate

    # Default-handler call chain (FUN_80025eec)
    "8001698c",  # first call; returns nonzero to skip frame
    "800172c0",  # called from FUN_80016444 mid-pipeline

    # TMD renderer + adjacent table consumers (read 0x8007C018 + idx*4)
    "80021b04",
    "8001ebec",
    "8001e890",
    "80024d78",
    "80032a44",  # adjacent region 0x8007C338
    "80020424",  # adjacent region 0x8007C370
    "800204a4",

    # GTE-heavy SCUS functions (renderer + transform candidates)
    "8002735c",  # 60 GTE ops -- the Legaia TMD renderer
    "80048a08",  # 40 ops (ctc2-heavy: GTE setup)
    "80029dd8",  # 32 ops
    "8004638c",  # 29 ops (triplet)
    "8004629c",  # 29 ops (triplet)
    "800461a4",  # 29 ops (triplet)
    "8001b73c",  # 27 ops
    "80027c6c",  # 22 ops
    "8001ada4",  # 18 ops
    "80029888",  # 16 ops
    "8003d344",  # 5 GTE ops; called from FUN_80021b04

    # DATA_FIELD trailer-consumer chain (readers of _DAT_8007b85c)
    "8001c604",
    "8001e3b8",
    "8001eef0",
    "80020118",
    "800243f0",
    "8002574c",
    "80052770",

    # Asset co-load chain (multi-PROT loaders surfaced by find_prot_consumers.py)
    "800520f0",  # loads PROT 0x369 + 0x36B (battle scene loader)
    "8001ed60",  # loads PROT 0x36C
    "8003ec70",  # loads PROT 0x381
    "8001fa88",  # loads PROT 0x5

    # Dev-path xrefs (string literals in SCUS_942.54)
    "8001f7c0",  # "h:\\PROT\\FIELD\\" - field/town scene loader
    "8001e890",  # "player.lzs" - player asset entry point
    "80019098",  # "h:\\prot\\all\\mapname" - map lookup
    "8001d8fc",  # "cdname.txt" - CDNAME parser at boot

    # ANM container consumer (asset type 6)
    "80024cfc",  # ANM buffer reader (lw at 0x80024D30)
    "80020de0",  # actor-state setup helper called by FUN_80024cfc and siblings
    "80024c88",  # sibling of FUN_80024cfc
    "80024e80",  # sibling of FUN_80024cfc
    "8001fc00",  # SCUS asset loader called from overlay with type=6
    "8001e54c",  # second SCUS function called with type=6

    # World map overlay - new functions from captures mc4/mc7/mc8 (2026-05-08)
    "801cfc40",  # world_map_top: first code entry; 5 callers (top-view dispatch?)
    "801cf8ac",  # 3 callers (small helper ~328B)
    "801da51c",  # 3 callees (~724B)
    "801eca08",  # 3 callees (~772B)
    "801ee90c",  # 2 callees (~1100B)
    "801ef014",  # 2 callees (~668B)
    "801dbe9c",  # 3 callees, world_map_top only (~544B)
    "801ee5d4",  # 1 callee (~824B)
    "801eed58",  # 1 callee (~700B)
    "801dba20",  # 2 callers (~512B)
    "801d5e20",  # 1 caller (~568B)
    "801da390",  # 1 caller (~396B)
    "801e4794",  # largest new fn ~1220B
    "801e6b34",  # ~1084B
    "801e4d8c",  # ~968B
    "801e5338",  # ~804B
    "801d2ebc",  # ~756B
    "801e6f70",  # ~608B
    "801ed308",  # 1 callee (~648B)
    "801ee094",  # 1 callee (~660B)
    "801ee328",  # 1 callee (~684B)
    "801ed590",  # 3 callees (~384B)
    "801edf00",  # 2 callees (~404B)
    "801d2298",  # 2 callees (~364B)
    "801d5c08",  # 1 callee (~344B)
    "801d5d60",  # 2 callees (~192B)
    "801e6400",  # ~556B
    "801e6778",  # ~524B
    "801e6984",  # ~432B
    "801d6058",  # 1 callee (~580B, world_map_top)

    # Missing helpers surfaced by world_map dumps (coverage tracker 2026-05-08)
    "80017714",  # cited by 801edf00
    "80039b7c",  # cited by 801da51c (world map entity tick)
    "8003d038",  # cited by 801cfc40 (world_map_top sprite batcher)
    "801d84b4",  # cited by 801ed590 (overlay address)
    "8002035c",  # cited by 80017714
    "80038050",  # cited by 80039b7c
    "80056608",  # cited by 80017714
    "80056628",  # cited by 80017714
    "8005fe7c",  # cited by 80017714
    "80062164",  # cited by 80017714
    "8006ca04",  # cited by 80017714
    "8003d038",  # cited by 801cfc40 (world_map_top)
    "80056648",  # cited by 8002035c
    "8005fdb8",  # cited by 80062164
    "8006d2ac",  # cited by 8006ca04
    "8006ef18",  # cited by 8002035c
]

OUT_DIR = "/scripts/funcs"
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
        print("[skip] {} not an address".format(addr_str))
        return
    func = fm.getFunctionContaining(addr)
    if func is None:
        # try the entry directly
        func = fm.getFunctionAt(addr)
    if func is None:
        print("[skip] no function for {}".format(addr_str))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    out_path = os.path.join(OUT_DIR, addr_str + ".txt")
    with open(out_path, "w") as fh:
        fh.write("== {} {} (entry={}) ==\n".format(
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
            fh.write("(decompile exception: {})\n".format(e))
    print("wrote {}".format(out_path))


for t in TARGETS:
    dump(t)

print("done")
