# @category Legaia
# @runtime Jython
#
# Dumps a JSON report of the SCUS_942.54 program: every function with an
# LZSS-decoder fingerprint score, plus every defined string with its
# inbound xrefs. Emits /scripts/explore.json (mapped to ghidra/scripts/
# on the host).

import json

from ghidra.program.model.data import StringDataType, TerminatedStringDataType
from ghidra.program.model.symbol import RefType

prog = currentProgram
fm = prog.getFunctionManager()
listing = prog.getListing()
ref_mgr = prog.getReferenceManager()


def function_summary(func):
    body = func.getBody()
    instrs = list(listing.getInstructions(body, True))
    op_counts = {}
    has_andi_f = False
    has_andi_fff = False
    has_andi_1 = False
    has_srl_1 = False
    has_srl_4 = False
    branch_count = 0
    for ins in instrs:
        m = ins.getMnemonicString()
        op_counts[m] = op_counts.get(m, 0) + 1
        if m in ("beq", "bne", "bgtz", "bltz", "bgez", "blez", "j", "jal", "b"):
            branch_count += 1
        if m == "andi":
            try:
                imm = ins.getScalar(2)
                if imm is not None:
                    v = imm.getValue() & 0xFFFF
                    if v == 0xF:
                        has_andi_f = True
                    elif v == 0xFFF:
                        has_andi_fff = True
                    elif v == 0x1:
                        has_andi_1 = True
            except Exception:
                pass
        if m == "srl":
            try:
                imm = ins.getScalar(2)
                if imm is not None:
                    v = imm.getValue() & 0xFF
                    if v == 1:
                        has_srl_1 = True
                    elif v == 4:
                        has_srl_4 = True
            except Exception:
                pass

    score = 0
    if op_counts.get("lbu", 0) >= 2:
        score += 2
    if op_counts.get("sb", 0) >= 1:
        score += 2
    if has_srl_1:
        score += 2  # peeling control bit
    if has_andi_1:
        score += 2  # masking control bit
    if has_andi_f or has_andi_fff:
        score += 2  # masking back-ref length
    if has_srl_4:
        score += 1
    if 30 <= len(instrs) <= 400:
        score += 1
    if branch_count >= 3:
        score += 1

    return {
        "addr": str(func.getEntryPoint()),
        "name": func.getName(),
        "size_bytes": int(body.getNumAddresses()),
        "instrs": len(instrs),
        "branches": branch_count,
        "ops": {k: v for k, v in op_counts.items() if v >= 1},
        "has_andi_f": has_andi_f,
        "has_andi_fff": has_andi_fff,
        "has_andi_1": has_andi_1,
        "has_srl_1": has_srl_1,
        "has_srl_4": has_srl_4,
        "lzss_score": score,
    }


def collect_strings():
    out = []
    for data in listing.getDefinedData(True):
        dt = data.getDataType()
        name = dt.getName().lower() if dt is not None else ""
        if "string" not in name and "char" not in name:
            continue
        try:
            v = data.getValue()
        except Exception:
            v = None
        if v is None:
            continue
        s = str(v)
        if len(s) < 3:
            continue
        addr = data.getAddress()
        refs_to = list(ref_mgr.getReferencesTo(addr))
        if not refs_to:
            continue
        out.append({
            "addr": str(addr),
            "value": s[:160],
            "xrefs": [str(r.getFromAddress()) for r in refs_to[:16]],
        })
    return out


def main():
    funcs = [function_summary(f) for f in fm.getFunctions(True)]
    funcs.sort(key=lambda x: x["lzss_score"], reverse=True)
    strings = collect_strings()

    report = {
        "program": prog.getName(),
        "image_base": str(prog.getImageBase()),
        "function_count": len(funcs),
        "string_count": len(strings),
        "top_lzss_candidates": funcs[:25],
        "all_functions_brief": [
            {"addr": f["addr"], "name": f["name"], "instrs": f["instrs"], "score": f["lzss_score"]}
            for f in funcs
        ],
        "strings": strings,
    }

    out_path = "/scripts/explore.json"
    with open(out_path, "w") as fh:
        json.dump(report, fh, indent=2)
    print("wrote {}: {} functions, {} strings".format(out_path, len(funcs), len(strings)))


main()
