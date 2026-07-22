# @category Legaia
# @runtime Jython
#
# Wave-16 dump-worklist drainer: produces per-function dumps for the
# port-catalog "cited but NOT dumped" addresses plus the ported-but-not-
# dumped provenance gaps. One script, run sequentially against each
# program that owns a subset of the targets:
#
#   -process SCUS_942.54                  -noanalysis -postScript /scripts/dump_wave16.py
#   -process overlay_0897_xxx_dat.bin     -noanalysis -postScript /scripts/dump_wave16.py
#   -process overlay_0899_xxx_dat.bin     ...
#   -process overlay_0896_bat_back_dat.bin ...
#   -process overlay_0977_other_game.bin  ...
#   -process overlay_0971.bin             ...
#   -process overlay_battle_action_0898.bin ...
#   -process 0967_xxx_dat.BIN             ...
#   -process overlay_0900_xxx_dat.bin     ...
#
# Targets are keyed per program (slot-A overlays VA-alias each other, so a
# blanket in_program() guard would dump aliased garbage from sibling
# programs); the handful of genuinely ambiguous VAs are listed under every
# candidate program and disambiguated after the fact by which dump carries
# a real function body.
#
# When an address is in-program but has NO analyzed function, the script
# does NOT mutate the DB: it emits a "[nofunc]" report with the defined-
# data unit at the address plus a pseudo-disassembly window, which is the
# classification evidence (data vs unanalyzed code vs mid-function label).

import os

from ghidra.app.cmd.disassemble import DisassembleCommand
from ghidra.app.cmd.function import CreateFunctionCmd
from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.app.util import PseudoDisassembler
from ghidra.util.task import ConsoleTaskMonitor

PROGRAM_TARGETS = {
    "SCUS_942.54": [
        # cited from 80046a20 (spine refs)
        "8004dc68", "80050120", "80051078", "80056208",
        # cited from main 80015e90
        "8001d230", "800265e8", "8002666c", "80026c20", "8002b92c",
        "8002b934", "8003f024", "8003f084", "80057edc", "80062310",
        "800644c0",
        # provenance gap: ported but never dumped
        "80036d80", "80066b00",
        # second-level worklist: callees cited by the wave-16 dumps above
        "80026234", "80050bb8", "80050f30", "80056618", "80056638",
        "80056668", "80056778", "8005bc28", "80062228", "80062d58",
        "80064698", "800654d8", "800655ac", "800693b8", "8006c9e4",
        "8006e2b4", "8006ee8c", "8006eee0",
        # third-level worklist (PsyQ sound-driver band interiors, mostly)
        "8006aae0", "8005bcb8", "8005bce0", "8005bd08", "8005bd80",
        "8005eb50", "80060a1c", "80060a94", "80060ebc", "80060f8c",
        "80061054", "8006113c", "800611e4", "8006126c", "8006139c",
        "800614d0", "80061540", "800615b0", "8006166c", "8006171c",
        "80061954", "80061b24", "80061bf8", "80062af0", "80065fe8",
        "800693d8", "8006cf9c", "8006d1e0", "8006e46c", "8006e600",
        "8006e8d4", "8006ef48", "8006ef58", "8006efd0",
        # fourth-level worklist (same PsyQ band, converging)
        "8005bd30", "8006861c", "8005d504", "8005d5f8", "8005d648",
        "80062d98", "80062dd8", "80062e28", "80062f60", "80064cf0",
        "80064df8", "8006558c", "800655cc", "80065b98", "80065bac",
        "80066308", "8006688c", "80067c1c", "80067d0c", "800694d0",
        "8006954c", "8006a104", "8006cfc8", "8006d030", "8006d358",
        "8006d794", "8006e8f8", "8006e9c0", "8006ec24", "8006ecfc",
        # fifth-level worklist
        "8005dab0", "80066d8c", "80067428", "80067a1c", "80068568",
        "800699ac", "8006a0e0", "8006b684", "8006b854", "8006c9a8",
        "8006d2f0", "8006d470", "8006d768", "8006d7d0", "8006d854",
        "8006e06c", "8006ed34",
        # sixth-level worklist (tail of the SPU DMA cluster)
        "8006d9a0", "8006d9d8", "8006e08c", "8006e0a0", "8006e0e0",
        "8006daac", "8006e0c0", "8006e100",
    ],
    "overlay_0897_xxx_dat.bin": [
        "801c1634", "801c36ac", "801c4520", "801c46a4", "801c6248",
        "801cff3c", "801d0094", "801d0170", "801d1eec", "801d207c",
        "801d20d4", "801d21ac", "801d227c", "801d2e90", "801d4c30",
        "801d873c", "801dfb10", "801e015c", "801e08c4", "801e0df0",
        "801e4af0", "801e5134", "801e7504", "801ee4b8", "801f2098",
        # ambiguous slot-A VA cited from SCUS FUN_80025b30
        "801ce844",
        # second-level: cited by the 801ee4b8 fragment (its j target)
        "801d808c",
    ],
    "overlay_0899_xxx_dat.bin": [
        "801c2704", "801c3594",
        "801ce844",
    ],
    "overlay_0896_bat_back_dat.bin": [
        "801c0d1c", "801c2720", "801db0f8", "801e5ae8", "801e6548",
        "801e65f8", "801ffba4", "802097bc", "8020e504",
        "801ecc00",
    ],
    "overlay_0977_other_game.bin": [
        "801c085c", "801c614c", "801c6804", "801d08ec", "801d1288",
        "801d1308", "801d14b0",
    ],
    "overlay_0971.bin": [
        "801c0f18",
    ],
    "overlay_battle_action_0898.bin": [
        "801ce844", "801ecc00", "801f6b24",
    ],
    "0967_xxx_dat.BIN": [
        "801f6b24",
        # provenance gap: battle-tutorial waiter, ported but never dumped
        "801f6b70",
    ],
    "overlay_0900_xxx_dat.bin": [
        "801f6b24",
    ],
    # Fresh correctly-based imports (the older overlay_0977/overlay_0978
    # programs are mis-based at the 0x801C0000 capture-window address):
    #   0902_xxx_dat.BIN  @ 0x801CE818 (slot A; GAME OVER INIT loads
    #                        FUN_8003EBE4(7) and calls 0x801ce844 = +0x2C)
    #   0977_other_game.BIN @ 0x801CE818 (slot A; muscle-dome door/init,
    #                        script-vm.md pins 0x801D0FF8 = file+0x27E0)
    #   0978_other_game.BIN @ 0x801F69D8 (slot B via overlay_loader_b(0x53);
    #                        tick entry 0x801f6b24 = +0x14C)
    "0902_xxx_dat.BIN": [
        "801ce844",
    ],
    "0977_other_game.BIN": [
        "801d08ec", "801d1288", "801d1308", "801d14b0",
        # second-level: cited by 801d1308
        "801d050c",
    ],
    "0978_other_game.BIN": [
        "801f6b24",
    ],
    "overlay_battle_action.bin": [
        "801ecc00",
    ],
}

# Addresses that are real unanalyzed code (pseudo-disassembly shows a
# function head) in programs where auto-analysis never reached them.
# For these the script force-disassembles + creates the function first.
FORCE_CREATE = {
    # The last four are 2-instruction `jr ra` stubs - genuine jal targets
    # from main (retail-stubbed hooks), so real entries despite the size.
    "SCUS_942.54": ["80036d80", "800265e8",
                    "80026c20", "8002b92c", "8002b934", "8003f084"],
    "0967_xxx_dat.BIN": ["801f6b70"],
    "0902_xxx_dat.BIN": ["801ce844"],
    "0977_other_game.BIN": ["801d08ec", "801d1288", "801d1308", "801d14b0"],
    "0978_other_game.BIN": ["801f6b24"],
}

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

pseudo = PseudoDisassembler(prog)


def out_path_for(addr_str):
    if prog_name.startswith("SCUS"):
        return os.path.join(OUT_DIR, addr_str + ".txt")
    label = prog_name.replace(".bin", "").replace(".BIN", "").replace(".", "_")
    if not label.startswith("overlay_"):
        label = "overlay_" + label
    return os.path.join(OUT_DIR, label + "_" + addr_str + ".txt")


def in_program(addr):
    return mem.getBlock(addr) is not None


def pseudo_window(fh, addr, count):
    fh.write("--- PSEUDO-DISASSEMBLY WINDOW (no DB mutation) ---\n")
    cur = addr
    for _ in range(count):
        try:
            ins = pseudo.disassemble(cur)
        except Exception as e:
            fh.write("{}  (pseudo-disasm failed: {})\n".format(cur, e))
            break
        if ins is None:
            fh.write("{}  (undecodable)\n".format(cur))
            break
        fh.write("{}  {}\n".format(cur, ins.toString()))
        cur = cur.add(ins.getLength())


def force_create(addr):
    if fm.getFunctionAt(addr) is not None:
        return
    cmd = DisassembleCommand(addr, None, True)
    cmd.applyTo(prog, monitor)
    fcmd = CreateFunctionCmd(addr)
    if fcmd.applyTo(prog, monitor):
        print("[force-create] function created at {}".format(addr))
    else:
        print("[force-create] FAILED at {}".format(addr))


def dump(addr_str):
    addr = af.getAddress(addr_str)
    if addr is None:
        print("[skip] {} not an address".format(addr_str))
        return
    if not in_program(addr):
        print("[skip] {} not in {}".format(addr_str, prog_name))
        return
    if addr_str in FORCE_CREATE.get(prog_name, []):
        force_create(addr)
    func = fm.getFunctionAt(addr) or fm.getFunctionContaining(addr)
    out_path = out_path_for(addr_str)

    if func is None:
        fh = open(out_path, "w")
        try:
            fh.write("== NOFUNC {} [{}] ==\n".format(addr_str, prog_name))
            fh.write("no analyzed function at or containing this address\n")
            cu = listing.getCodeUnitAt(addr)
            fh.write("code unit at addr: {}\n\n".format(cu))
            pseudo_window(fh, addr, 48)
        finally:
            fh.close()
        print("wrote {} (NOFUNC)".format(out_path))
        return

    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))

    fh = open(out_path, "w")
    try:
        fh.write("== {} {} (entry={}) [{}] ==\n".format(
            func.getName(), addr_str, func.getEntryPoint(), prog_name))
        if func.getEntryPoint() != addr:
            fh.write("NOTE: requested address is INTERIOR to this function\n")
        fh.write("size={} bytes, {} instructions\n\n".format(
            body.getNumAddresses(), len(instrs)))
        fh.write("--- DISASSEMBLY ---\n")
        for ins in instrs:
            fh.write("{}  {}\n".format(ins.getAddress(), ins.toString()))
        if len(instrs) == 0:
            fh.write("(no analyzed instructions in body)\n")
            pseudo_window(fh, addr, 48)
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


targets = PROGRAM_TARGETS.get(prog_name)
if targets is None:
    print("[skip-all] no target list for program {}".format(prog_name))
else:
    for t in targets:
        dump(t)

print("done [{}]".format(prog_name))
