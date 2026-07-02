# @category Legaia
# @runtime Jython
#
# Resolve the S5-battle "render-tail" trace hits (the addresses the containment
# attribution left unresolved because no dumped battle_action function covers
# them) to their ENCLOSING function in the currently-open program, using the
# program's own analysis (getFunctionContaining). Prints, per hit address, the
# containing function entry + name + whether a dump already exists on disk.
#
# Run against the battle overlay program:
#   docker compose exec ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process overlay_battle_action.bin -noanalysis \
#       -postScript /scripts/resolve_render_tail.py

import os

# The unresolved S5 overlay hits (addr, hit-count) from
# attribute_overlay_hits.py on captures/trace/s5_tetsu_battle/union.csv.
HITS = [
    (0x801e0080, 120), (0x801f71e0, 62), (0x801e0598, 18), (0x801f0740, 16),
    (0x801f7624, 8), (0x801e0418, 6), (0x801e02a4, 6), (0x801f02d0, 4),
    (0x801f0adc, 3), (0x801f1950, 2), (0x801f1890, 2), (0x801f6d48, 1),
    (0x801f6c70, 1), (0x801f07ac, 1), (0x801f04b0, 1), (0x801f0450, 1),
    (0x801f03c0, 1), (0x801f03b0, 1),
]

FUNCS_DIR = "/scripts/funcs"

fm = currentProgram.getFunctionManager()
af = currentProgram.getAddressFactory()


def addr(a):
    return af.getDefaultAddressSpace().getAddress(a)


seen = {}
print("== render-tail hit -> containing function ==")
for a, hits in HITS:
    f = fm.getFunctionContaining(addr(a))
    if f is None:
        print("  0x%08x (%dx)  -> NO FUNCTION (undefined code / not analyzed)" % (a, hits))
        continue
    entry = f.getEntryPoint().getOffset()
    off = a - entry
    stem = "%08x" % entry
    dumped = os.path.exists(os.path.join(FUNCS_DIR, "overlay_battle_action_%s.txt" % stem))
    tag = "DUMPED" if dumped else "** NEW **"
    print("  0x%08x (%dx)  -> FUN_%08x +0x%x  [%s]  %s" % (a, hits, entry, off, f.getName(), tag))
    seen[entry] = seen.get(entry, 0) + hits

print("\n== distinct enclosing functions (total hits) ==")
for e in sorted(seen, key=lambda k: -seen[k]):
    stem = "%08x" % e
    dumped = os.path.exists(os.path.join(FUNCS_DIR, "overlay_battle_action_%s.txt" % stem))
    print("  FUN_%08x  %5d  %s" % (e, seen[e], "DUMPED" if dumped else "** NEW - dump target **"))
