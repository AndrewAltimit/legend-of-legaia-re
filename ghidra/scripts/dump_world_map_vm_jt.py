# @category Legaia
# @runtime Jython
#
# Dump the FUN_801D362C world-map drawing-script VM jump table at
# 0x801D1E94 (= 0x801D362C - 0x1798). The table has 0x3D u32 entries
# (case-start addresses). For each case-start, walk forward up to ~40
# instructions and capture the prelude so we can identify the opcode's
# bytecode args and side effects.
#
# FUN_801D362C dispatches via:
#   801d3660  lui v0,0x801d
#   801d3664  addiu v0,v0,-0x1798     ; v0 = 0x801D0000 - 0x1798 = 0x801CE868
#   801d3668  sll v1,v1,0x2           ; v1 = opcode * 4
#   801d366c  addu v1,v1,v0
#   801d3670  lw v0,0x0(v1)
#   801d3678  jr v0
#
# Output: /scripts/funcs/world_map_vm_jt_<program>.txt
#
# This is run against overlay_world_map.bin (or any of its variants).

import os

from ghidra.app.decompiler import DecompInterface, DecompileOptions
from ghidra.util.task import ConsoleTaskMonitor

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()
memory = prog.getMemory()

PROGRAM_NAME = prog.getName()
OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

JT_BASE = 0x801CE868
JT_COUNT = 0x3D
PRELUDE_INSTRS = 16

out_path = os.path.join(
    OUT_DIR, "world_map_vm_jt_%s.txt" % PROGRAM_NAME.replace("/", "_"))


def read_u32(addr_int):
    a = af.getAddress("%x" % addr_int)
    if a is None:
        return None
    try:
        return memory.getInt(a) & 0xFFFFFFFF
    except Exception:
        return None


def disasm_at(addr_int, n=PRELUDE_INSTRS):
    a = af.getAddress("%x" % addr_int)
    if a is None:
        return ["(address %08x not in program)" % addr_int]
    out = []
    ins = listing.getInstructionAt(a)
    seen = 0
    while ins is not None and seen < n:
        out.append("  %s  %s" % (ins.getAddress(), ins.toString()))
        seen += 1
        ins = ins.getNext()
    return out


lines = []
lines.append("program: %s" % PROGRAM_NAME)
lines.append("jump table at 0x%08x (%d entries)" % (JT_BASE, JT_COUNT))
lines.append("")

unique = {}

for i in range(JT_COUNT):
    entry_addr = JT_BASE + i * 4
    target = read_u32(entry_addr)
    if target is None:
        lines.append("[%2d / 0x%02x] @ 0x%08x  (unreadable)" % (i, i, entry_addr))
        continue
    lines.append("[%2d / 0x%02x] @ 0x%08x  -> 0x%08x" % (i, i, entry_addr, target))
    unique.setdefault(target, []).append(i)

lines.append("")
lines.append("--- unique case-start preludes (first %d instrs) ---" % PRELUDE_INSTRS)
for target, ops in sorted(unique.items()):
    op_str = ", ".join("0x%02x" % o for o in ops)
    lines.append("")
    lines.append("=== target 0x%08x (opcodes %s) ===" % (target, op_str))
    for ln in disasm_at(target):
        lines.append(ln)

with open(out_path, "w") as f:
    f.write("\n".join(lines))

print("wrote %s" % out_path)
print("%d entries, %d unique targets" % (JT_COUNT, len(unique)))
