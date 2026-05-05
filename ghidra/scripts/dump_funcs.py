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
    "8003e8a8",  # sub-asset address resolver (PROT[id], sub_id)
    "8003e800",  # sub-asset loader (dst, addr, sub_id)
    "80052fa0",  # battle archive loader 1
    "800542c8",  # battle archive loader 2
    "80020224",  # caller #1 of dispatcher FUN_8001f05c
    "8002541c",  # caller #2 of dispatcher (handles types 10/0xF/0x14)
    "800255b8",  # called from 8002541c right before table use - probably disc reader
    "80058104",  # called twice in 8002541c - init/teardown helper
    "80017888",  # FUN_8002541c calls this for malloc - allocator
    "800198e0",  # called per-entry in cleanup loop - probably free
    "8001e1b4",  # *** the function that writes _DAT_8007b85c (init!) ***
    "8003eb98",  # by-index loader (the on-disc -> in-RAM transformation)
    "8003f128",  # actual disc read kickoff
    "8005c328",  # sets up CD position
    "8005c42c",  # gets base value
    "80026b4c",  # TMD handler from FUN_8001f05c dispatcher
    "800268dc",  # actual TMD primitive iterator (called from 80026b4c)
    # Boot-time TOC loader candidates (Epic 4.1)
    "8003e4e8",  # references 0x801C70F0 + 0x801C88F0 + 0x801C8970 -- multi-region init
    "8003dda0",  # references only 0x801C70F0 -- possible dedicated TOC setup
    "8003e68c",  # short stub referencing 0x801C70F0
    "8003e360",  # references 0x801C88F0 -- sibling allocator
    "8003e6bc",  # references 0x801C88F0
    "8003e9c0",  # references 0x801C88F0
    # Callers of the boot-time TOC loader (FUN_8003e4e8)
    "8003efe8",
    "8003f08c",
    "8003d3c4",  # generic file-into-buffer reader, sibling of FUN_8003e4e8
    # Debug-flag region readers (Epic 4.2 - find consumers of 0x8007b98f).
    # All ten read the word at 0x8007B98C, which contains byte 0x8007B98F.
    "80016230",  # writes 0x8007BC3C/4C; reads 0x8007B98C, 0x8007B9A8, 0x8007B9AC, 0x8007B9DC
    "80016444",  # reads 0x8007B98C, 0x8007B924, 0x8007BC00, 0x8007BC7C, 0x8007B9CC
    "800173bc",  # reads 0x8007B98C; writes 0x8007BCAC
    "800179c0",  # reads 0x8007B98C
    "80016b6c",  # heavy debug-area consumer
    "8003aeb0",  # writes a dozen debug-area addresses - probable debug-state init/reset
    "80018db0",  # writes 0x8007BC70 (back-to-back R/W - probable counter)
    "80016b6c",  # repeat for the dispatcher hint
    "800188c8",  # multiple debug-area reads (B852, B87C, B6D0)
    "80030628",  # consecutive byte reads in BB88-BB91 - packed debug flags
    "8001822c",  # only refmgr-known reader of 0x8007B7C0 (debug dispatch trigger)
    # Move-table consumer (Epic 10.5 - Tactical Arts move definition table)
    "800204f8",  # reads BOTH _DAT_8007b888 (MOVE) and _DAT_8007b840 (MOVE2)
    "80020740",  # pre-tick helper called by FUN_800204f8 when actor flag 0x1000 set
    # Game-mode state machine + script VM recon (Phase 0)
    "80025eec",  # default per-frame handler shared by 13 of 28 modes (incl MAIN MODE)
    "80025c68",  # mode 0 handler (CONFIG init)
    "80025b64",  # mode 2 handler (MAIN init)
    "80025f2c",  # mode 13 handler (MAPDSIP MODE = field display)
    "80025e68",  # mode 8 handler (EFECT init)
    "8001dcf8",  # boot-time mode initializer (1212 bytes)
    "8001c93c",  # mode-table reader; possibly the per-frame dispatcher
    # Script VM hunt - default-handler pipeline calls (FUN_80025eec body)
    "8001698c",  # first call - returns nonzero to skip frame; primary VM candidate
    "800172c0",  # called from FUN_80016444 mid-pipeline; may be VM tick
    # MAIN MODE INIT and MAPDSIP non-default handlers
    "80025da0",  # mode 12 (MAPDSIP MODE INIT)
    "80025f74",  # mode 23 (CARD MODE) non-default
    "80025980",  # mode 24 (OTHER) init
    "80025fb4",  # mode 26 (STR) init
    # TMD renderer hunt (Epic 4.4 / 6.3) - first batch dumped on a wrong
    # premise (TABLE_ADDR was actually 0x8007C018, not 0x80080018). These
    # turned out to be PSX SDK GPU wrappers (MoveImage, DrawOTag, etc.):
    "80058490",
    "80058704",
    "80058778",
    "8005887c",
    "8005feac",
    # TMD renderer hunt (corrected) - functions that READ the first slot
    # of the TMD pointer table at 0x8007C018. These are static-binary
    # consumers of FUN_80026b4c's output.
    "80021b04",
    "8001ebec",
    "8001e890",
    "80024d78",
    # Adjacent table region 0x8007C338 (write-cluster) and 0x8007C370
    # (read+write pair). May be related TMD metadata or unrelated.
    "80032a44",
    "80020424",
    "800204a4",
    # TMD renderer hunt round 2: GTE-heavy functions in SCUS_942.54.
    # If the renderer is in the static binary, it'll be one of these.
    "8002735c",  # 60 GTE ops (top candidate)
    "80048a08",  # 40 ops (mostly ctc2 - GTE setup)
    "80029dd8",  # 32 ops (swc2/lwc2)
    "8004638c",  # 29 ops -- triplet
    "8004629c",  # 29 ops -- triplet
    "800461a4",  # 29 ops -- triplet
    "8001b73c",  # 27 ops
    "80027c6c",  # 22 ops
    "8001ada4",  # 18 ops
    "80029888",  # 16 ops
    "8003d344",  # called from FUN_80021b04 and uses 5 GTE ops
    # DATA_FIELD trailer-consumer hunt (Epic 9.1) - functions that read
    # the streaming buffer pointer _DAT_8007b85c besides the loader/driver.
    "8001c604",  # 2 reads
    "8001e3b8",  # reads buffer
    "8001eef0",  # reads buffer + asset table index
    "80020118",  # 2 reads
    "800243f0",  # reads buffer + asset table index
    "8002574c",  # reads buffer (right after streaming driver)
    "80052770",  # reads buffer
    # Buffer reader at 0x800219e0 lives in an unfunctioned region; need
    # to disassemble surroundings separately.
    # Asset co-load chain hunt (find_prot_consumers.py output)
    "800520f0",  # loads BOTH 0x369 and 0x36B - first confirmed multi-PROT loader
    "8001ed60",  # loads 0x36C; agent says also 0x384 via different resolver
    "8003ec70",  # loads 0x381
    "8002574c",  # loads 0x37E
    "8001fa88",  # loads 0x5
    # Dev-path xref hunt (find_string_xrefs.py output)
    "8001f7c0",  # references "h:\PROT\FIELD\" - field/town scene loader
    "8001e890",  # references "player.lzs" - player asset entry point
    "80019098",  # references "h:\prot\all\mapname" - map lookup
    "8001d8fc",  # references "cdname.txt" - CDNAME parser at boot
    # ANM format hunt (Epic 3.6) - dispatcher case 6 stores the buffer
    # pointer at DAT_8007b7c8; only ONE consumer in SCUS:
    "80024cfc",  # the ANM buffer reader (lw at 0x80024D30)
    "80020de0",  # actor-state setup helper called by 80024cfc and 19 others
    "80024c88",  # sibling of 80024cfc (also calls 80020de0)
    "80024e80",  # sibling of 80024cfc (also calls 80020de0)
    # ANM loader candidates from overlay FUN_801d6704: it calls
    #   func_0x8001fc00(0x36e, 6, _DAT_8007b85c, 0, 0x37000)
    # then func_0x8001e54c(6, _DAT_8007b85c, 0) -- both with type=6 = ANM
    "8001fc00",  # SCUS asset loader called from overlay with type=6 (ANM)
    "8001e54c",  # second SCUS function called with type=6 right after
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
