# @category Legaia
# @runtime Jython
#
# Hunt for runtime consumers of the scene-v12 cluster (the 97 PROT
# entries with strict header signature `[N+4, 0x12, 0, 0x14, ?, N, 0,
# N+2]` at bytes 0..16). See crates/asset/src/scene_v12_table.rs and
# docs/formats/scene-bundles.md for the on-disc layout.
#
# Approach (heuristic): a consumer reads `u16[1]` and `u16[3]` (the
# constants 0x12 and 0x14) as part of validating the header before
# walking the offset tables. Look for `lh / lhu` instructions whose
# immediate matches `+2` or `+6` from a base register, followed by a
# compare against 0x12 or 0x14.
#
# Pattern matched (simplified):
#   lhu Vx, +2(Vbase)
#   ...
#   ori / addiu Vy, zero, 0x12        ; or `li Vy, 0x12`
#   beq Vx, Vy, ...                    ; or bne
#
# Output groups by containing function so high-frequency matches stand
# out. Run with -process for each captured program.

import os

OUT_DIR = "/scripts/funcs"
try:
    os.makedirs(OUT_DIR)
except OSError:
    pass

prog = currentProgram
prog_name = prog.getName()
listing = prog.getListing()
fm = prog.getFunctionManager()


def label_for(prog_name):
    return prog_name.replace(".bin", "").replace(".", "_")


def all_imm_values(ins):
    out = []
    for n in range(ins.getNumOperands()):
        for op in ins.getOpObjects(n):
            try:
                out.append(op.getValue())
            except AttributeError:
                continue
    return out


hits = []
last_h_load_at_2 = {}  # address -> register name producing the +2 load
last_h_load_at_6 = {}

for ins in listing.getInstructions(True):
    mnem = ins.getMnemonicString().lower()
    if mnem in ("lh", "lhu"):
        imms = all_imm_values(ins)
        if 2 in imms or 6 in imms:
            addr = ins.getAddress()
            func = fm.getFunctionContaining(addr)
            func_addr = func.getEntryPoint() if func else None
            offset = 2 if 2 in imms else 6
            hits.append(
                (
                    str(addr),
                    mnem,
                    offset,
                    str(func_addr) if func_addr else "(no func)",
                )
            )

# Filter to functions that have BOTH a +2 and a +6 lh/lhu load - those
# are much higher-confidence v12 consumers (the header has constants at
# both offsets).
by_func = {}
for addr, mnem, off, func in hits:
    by_func.setdefault(func, {"+2": 0, "+6": 0, "rows": []})
    key = "+" + str(off)
    by_func[func][key] += 1
    by_func[func]["rows"].append((addr, mnem, off))

candidates = {f: v for f, v in by_func.items() if v["+2"] > 0 and v["+6"] > 0}

out_path = os.path.join(OUT_DIR, "scene_v12_consumers_" + label_for(prog_name) + ".txt")
fh = open(out_path, "w")
try:
    fh.write("# scene-v12 consumer search [{}]\n".format(prog_name))
    fh.write(
        "# total +2-or-+6 hits: {}, two-imm-bothlist candidates: {}\n".format(
            len(hits), len(candidates)
        )
    )
    fh.write("# format: ins_addr  mnem  offset  containing_func\n\n")
    fh.write("=== HIGH-CONFIDENCE CANDIDATES (both +2 and +6 lh/lhu) ===\n")
    for func in sorted(candidates.keys()):
        v = candidates[func]
        fh.write(
            "== func={}, +2={}, +6={}, total_rows={} ==\n".format(
                func, v["+2"], v["+6"], len(v["rows"])
            )
        )
        for addr, mnem, off in v["rows"]:
            fh.write("  {}  {:5}  +{}\n".format(addr, mnem, off))
        fh.write("\n")
finally:
    fh.close()

print(
    "[{}] wrote {} hits ({} two-offset candidate functions) to {}".format(
        prog_name, len(hits), len(candidates), out_path
    )
)
