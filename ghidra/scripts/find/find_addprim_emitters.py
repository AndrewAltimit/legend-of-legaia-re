# @category Legaia
# @runtime Jython
#
# Sweep for PSX-GPU POLY_FT4 / POLY_GT4 emitter sites in the currently-loaded
# program. The PSX prim-pool slots all have a "code byte" at packet+7 (byte
# offset of POLY_FT4's `code` field). Textured-quad codes are 0x2C-0x2F:
#
#     0x2C  POLY_FT4 (textured, flat, opaque)
#     0x2D  POLY_FT4 (textured, flat, semi-transparent)
#     0x2E  POLY_FT4 (textured, Gouraud, opaque)
#     0x2F  POLY_FT4 (textured, Gouraud, semi-transparent)
#
# Two compiler-emitted shapes dominate:
#
#   Pattern A  (libgs setcode macro):
#       addiu / ori  $r, $zero, 0xKK         ; KK in 0x2C..0x2F
#       sb           $r, +off($base)
#
#   Pattern B  (inline assembled code word, written as a u32):
#       lui          $r, 0xKK00              ; KK in 0x2C..0x2F
#       ...optional or / ori with RGB color in the low 24 bits...
#       sw           $r, +off($base)
#
# Per-function aggregation: report functions sorted by total emitter hits,
# highlight unique (cmd_byte, offset) signatures within each function so we
# can tell apart "writes one POLY_FT4 per call" vs "loops over a tile table
# writing many POLY_FT4 packets per call".
#
# Tracker semantics: per-register most-recent constant. Resets on function
# entry, and caller-saved registers get invalidated by `jal` / `jalr`.

prog = currentProgram
af = prog.getAddressFactory()
fm = prog.getFunctionManager()
listing = prog.getListing()

PROGRAM_NAME = prog.getName()

# Output file: one report per program.
import os
out_dir = "/scripts/funcs"
try:
    os.makedirs(out_dir)
except OSError:
    pass
out_path = "%s/addprim_emitters_%s.txt" % (out_dir, PROGRAM_NAME.replace("/", "_"))


def _is_zero_reg(name):
    return name in ("zero", "0", "$zero", "r0")


# MIPS caller-saved registers (clobbered across jal / jalr).
CALLER_SAVED = set(
    [
        "v0",
        "v1",
        "a0",
        "a1",
        "a2",
        "a3",
        "t0",
        "t1",
        "t2",
        "t3",
        "t4",
        "t5",
        "t6",
        "t7",
        "t8",
        "t9",
        "ra",
        "at",
    ]
)

# Track per-register most-recent constant within the active function.
# value tuple: (kind, imm_int, source_insn_addr_str)
#   kind == "cmd_byte"  -> imm_int is an int in 0x2C..0x2F
#   kind == "code_word" -> imm_int is the full 32-bit lui value (0xKK000000)
recent = {}

# func_entry_int -> list of (insn_addr_str, "A"/"B", cmd_byte, mem_str, src_addr_str)
hits = {}

current_func_addr = None
inst_iter = listing.getInstructions(True)
total = 0

while inst_iter.hasNext():
    insn = inst_iter.next()
    total += 1
    func = fm.getFunctionContaining(insn.getAddress())
    fa = func.getEntryPoint().getOffset() if func else None
    if fa != current_func_addr:
        recent = {}
        current_func_addr = fa
    mnem = insn.getMnemonicString()

    # Caller-saved invalidation across function calls. JAL has a 1-instr
    # delay slot but for the conservative tracker that's fine - we only
    # care that the register value isn't trusted past the call.
    if mnem in ("jal", "jalr"):
        for r in list(recent.keys()):
            if r in CALLER_SAVED:
                del recent[r]
        continue

    # addiu $r, $rs, imm  (li imm via $zero)
    if mnem == "addiu":
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            src = insn.getDefaultOperandRepresentation(1)
            imm_obj = insn.getOpObjects(2)[0]
            imm = imm_obj.getValue() if hasattr(imm_obj, "getValue") else int(str(imm_obj), 0)
            if _is_zero_reg(src) and imm in (0x2C, 0x2D, 0x2E, 0x2F):
                recent[dst] = ("cmd_byte", imm, str(insn.getAddress()))
            else:
                if dst in recent:
                    del recent[dst]
        except Exception:
            if dst in recent:
                del recent[dst]
        continue

    # ori $r, $rs, imm  (alt li form)
    if mnem == "ori":
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            src = insn.getDefaultOperandRepresentation(1)
            imm_obj = insn.getOpObjects(2)[0]
            imm = imm_obj.getValue() if hasattr(imm_obj, "getValue") else int(str(imm_obj), 0)
            if _is_zero_reg(src) and imm in (0x2C, 0x2D, 0x2E, 0x2F):
                recent[dst] = ("cmd_byte", imm, str(insn.getAddress()))
            else:
                # ori $r, $r, lo  --  if r is being grown into a code word
                # from an earlier lui, keep tracking it as a code_word.
                if src == dst and dst in recent and recent[dst][0] == "code_word":
                    pass  # keep code_word tracking
                else:
                    if dst in recent:
                        del recent[dst]
        except Exception:
            if dst in recent:
                del recent[dst]
        continue

    # lui $r, imm  -> sets upper-16 bits.
    if mnem == "lui":
        try:
            dst = insn.getDefaultOperandRepresentation(0)
            imm_obj = insn.getOpObjects(1)[0]
            imm = imm_obj.getValue() if hasattr(imm_obj, "getValue") else int(str(imm_obj), 0)
            imm &= 0xFFFF
            hi = (imm >> 8) & 0xFF
            if hi in (0x2C, 0x2D, 0x2E, 0x2F):
                recent[dst] = ("code_word", (imm << 16) & 0xFFFFFFFF, str(insn.getAddress()))
            else:
                if dst in recent:
                    del recent[dst]
        except Exception:
            if dst in recent:
                del recent[dst]
        continue

    # sb $r, off(base) -- Pattern A completion.
    if mnem == "sb":
        try:
            src = insn.getDefaultOperandRepresentation(0)
            mem = insn.getDefaultOperandRepresentation(1)
            if src in recent and recent[src][0] == "cmd_byte":
                kind, val, lui_addr = recent[src]
                hits.setdefault(fa, []).append(
                    (str(insn.getAddress()), "A", val, mem, lui_addr)
                )
        except Exception:
            pass
        continue

    # sw $r, off(base) -- Pattern B completion.
    if mnem == "sw":
        try:
            src = insn.getDefaultOperandRepresentation(0)
            mem = insn.getDefaultOperandRepresentation(1)
            if src in recent and recent[src][0] == "code_word":
                kind, val, lui_addr = recent[src]
                hits.setdefault(fa, []).append(
                    (str(insn.getAddress()), "B", (val >> 24) & 0xFF, mem, lui_addr)
                )
        except Exception:
            pass
        continue

    # Anything else that writes a register clobbers our tracking for the
    # destination. Cheap heuristic: if first operand looks like a known
    # register, drop it.
    try:
        if insn.getNumOperands() >= 1:
            dst = insn.getDefaultOperandRepresentation(0)
            if dst in recent:
                # Exception: keep code_word alive through `or` chains that
                # add the low 24-bit RGB color.
                if mnem == "or":
                    if dst in recent and recent[dst][0] == "code_word":
                        pass
                    else:
                        del recent[dst]
                else:
                    del recent[dst]
    except Exception:
        pass

# ------------------- report -------------------
lines = []
lines.append("program: %s" % PROGRAM_NAME)
lines.append("instructions scanned: %d" % total)
lines.append("functions with emitter candidates: %d" % len(hits))
lines.append("")

# Sort by hit count desc.
ranked = sorted(hits.items(), key=lambda kv: -len(kv[1]))
for fa, refs in ranked:
    addr = af.getAddress("%x" % fa) if fa is not None else None
    func = fm.getFunctionAt(addr) if addr is not None else None
    fname = func.getName() if func else "?"
    a_hits = [r for r in refs if r[1] == "A"]
    b_hits = [r for r in refs if r[1] == "B"]
    # Unique (cmd_byte, mem_offset) signatures - rough indicator that the
    # site writes multiple distinct packets (i.e. loops over a record table).
    sigs = set((r[2], r[3]) for r in refs)
    lines.append(
        "FUN_%08X %-32s A=%-4d B=%-4d total=%-4d unique_sites=%d"
        % (fa, fname, len(a_hits), len(b_hits), len(refs), len(sigs))
    )
    for ia, p, cmd, mem, src_addr in refs[:8]:
        lines.append(
            "    [%s] %s cmd=0x%02X dst=%s lui/li@%s" % (p, ia, cmd, mem, src_addr)
        )
    if len(refs) > 8:
        lines.append("    ... %d more" % (len(refs) - 8))
    lines.append("")

with open(out_path, "w") as f:
    f.write("\n".join(lines))

print("\n".join(lines[: min(120, len(lines))]))
print("---")
print("full report -> %s" % out_path)
