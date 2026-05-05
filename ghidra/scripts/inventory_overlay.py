# @category Legaia
# @runtime Jython
#
# Generic per-overlay (or any program) function inventory dumper.
#
# For every function in the current program emits one CSV row:
#   entry_addr,name,size_bytes,instr_count,outgoing_calls,incoming_refs,top_callees
#
# `top_callees` is a `;`-separated list of up to 5 outgoing-call targets
# (helpful for spotting which SCUS APIs an overlay reaches into).
#
# Output: /scripts/inventory_<programname>.csv
#
# Run with the `-process <program>` flag to pick the target. Examples:
#
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process SCUS_942.54 -noanalysis \
#       -postScript /scripts/inventory_overlay.py
#
#   docker compose exec -T ghidra /ghidra/support/analyzeHeadless \
#       /projects legaia -process 0897_xxx_dat -noanalysis \
#       -postScript /scripts/inventory_overlay.py

import os

from ghidra.program.model.symbol import RefType


prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()

prog_name = prog.getName()
out_path = "/scripts/inventory_" + prog_name + ".csv"


def call_targets(func):
    body = func.getBody()
    targets = []
    seen = set()
    for ins in listing.getInstructions(body, True):
        for ref in ins.getReferencesFrom():
            rt = ref.getReferenceType()
            if rt is None:
                continue
            if not (rt.isCall() or rt.isJump()):
                continue
            tgt = ref.getToAddress()
            if tgt is None:
                continue
            tgt_func = fm.getFunctionContaining(tgt) or fm.getFunctionAt(tgt)
            if tgt_func is None or tgt_func.getEntryPoint() == func.getEntryPoint():
                continue
            key = str(tgt_func.getEntryPoint())
            if key in seen:
                continue
            seen.add(key)
            targets.append(tgt_func)
    return targets


def incoming_count(func):
    n = 0
    for ref in ref_mgr.getReferencesTo(func.getEntryPoint()):
        rt = ref.getReferenceType()
        if rt is not None and (rt.isCall() or rt.isJump()):
            n += 1
    return n


funcs = []
it = fm.getFunctions(True)
while it.hasNext():
    funcs.append(it.next())
funcs.sort(key=lambda f: int(str(f.getEntryPoint()), 16))

with open(out_path, "w") as fh:
    fh.write("entry,name,size,instructions,outgoing,incoming,top_callees\n")
    for func in funcs:
        body = func.getBody()
        size = body.getNumAddresses()
        instrs = sum(1 for _ in listing.getInstructions(body, True))
        callees = call_targets(func)
        outgoing = len(callees)
        incoming = incoming_count(func)
        callees.sort(key=lambda f: -int(incoming_count(f)))
        top = ";".join("{}@{}".format(f.getName(), f.getEntryPoint())
                       for f in callees[:5])
        fh.write("{},{},{},{},{},{},{}\n".format(
            func.getEntryPoint(), func.getName(), size, instrs,
            outgoing, incoming, top))

print("wrote {} ({} functions)".format(out_path, len(funcs)))
