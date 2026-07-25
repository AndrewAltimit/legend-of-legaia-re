#!/usr/bin/env python3
"""
Verify that each Ghidra function dump's printed addresses agree with where
its bytes actually live in the extracted images.

A dump prints instruction addresses derived from the load base Ghidra was
given. If that base is wrong, every address in the dump is wrong by a
constant while the instruction text stays perfectly plausible - so the dump
reads as authoritative and cites a function that does not exist at that VA.
This sweep detects exactly that failure, by ignoring the printed addresses
and asking the bytes where they live.

Method: canonicalise the dump's first N instructions into a base-independent
token sequence (branch displacements dropped, since those DO shift with the
base; registers and non-zero immediates kept), then look that sequence up in
an index built the same way over every extracted overlay + SCUS image. A
single hit resolves the dump to a real file offset, which the overlay map
turns back into a VA. The delta between that VA and the dump's printed VA is
the base error.

Classes reported:

  MATCH      printed VA == resolved VA. Trustworthy.
  SHIFTED    resolved at a constant non-zero delta. The dump was produced at
             the wrong load base; its addresses are all off by that delta.
  NOT_FOUND  bytes are in no extracted image. Usually a RAM-capture-derived
             dump whose source was a live save state, not a static
             extraction. UNVERIFIABLE, not known-bad.
  SHORT      fewer than the minimum signable instructions. No verdict.

See docs/tooling/dump-corpus-integrity.md for the standing results and what
each class is usable for.

--- The second axis: SHAPE ---

Base correctness is not the only way a dump can be defective, and the other
ways are not visible to the check above. A dump can print the right addresses
for the right bytes and still be unusable, because the bytes it carries are
not the whole routine. `--shape` classifies that axis:

  SOUND        header agrees with the stream and the body ends in a `jr`.
  NO_RETURN    header agrees with its own stream, every printed address is
               right, every instruction is real - and the stream still stops
               mid-body, because Ghidra computed a short function body
               (usually a second `FUN_` entry minted inside the routine cut
               it). INTERNALLY CONSISTENT, so neither the base check nor
               disc-coverage.py can see it. Repair with
               ghidra/scripts/repair_truncated_dumps.py.
  TAIL_J       ends in `j <target>` rather than `jr`. Ambiguous: either a
               real tail call or a label-call slice of a larger function.
               Bodies under SHORT_BODY_INSNS instructions are almost always
               the latter. Needs eyes.
  SIZE_MISMATCH  the `size=` header and the instruction count disagree.
  HEADERLESS_WITH_DISASM  no parseable `size=` header, but a complete
               disassembly is present. The dump is usable evidence that
               every header-driven counter silently discards - it makes
               coverage look worse than it is, not the corpus thinner.
  HEADERLESS_C_ONLY  no header and no disassembly. Only a C rendering, which
               this repo's own rules say is not evidence. Needs a re-dump.

The shape axis reads only the dump text, so unlike the base axis it needs no
`extracted/` tree: `--shape` runs anywhere.

Usage:
  scripts/ghidra-analysis/check-dump-base-integrity.py
  scripts/ghidra-analysis/check-dump-base-integrity.py --min-insns 10
  scripts/ghidra-analysis/check-dump-base-integrity.py --list-shifted
  scripts/ghidra-analysis/check-dump-base-integrity.py --shape
  scripts/ghidra-analysis/check-dump-base-integrity.py --shape --cited-only
  scripts/ghidra-analysis/check-dump-base-integrity.py --shape --emit-csv out.csv
"""

import argparse
import glob
import os
import re
import sys
from collections import Counter, defaultdict

try:
    import capstone
except ImportError:
    print("error: capstone not installed (`pip install capstone`)", file=sys.stderr)
    sys.exit(2)

try:
    import tomllib
except ImportError:  # Python < 3.11
    import tomli as tomllib

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
FUNCS = os.path.join(ROOT, "ghidra", "scripts", "funcs")
OVERLAYS = os.path.join(ROOT, "extracted", "overlays")
SCUS = os.path.join(ROOT, "extracted", "SCUS_942.54")
OVERLAY_MAP = os.path.join(ROOT, "crates", "asset", "data", "static-overlays.toml")

SCUS_BASE = 0x80010000
SCUS_HEADER = 0x800

# Mnemonic aliases capstone and Ghidra spell differently. Folding them keeps
# the token sequence stable across the two disassemblers.
MCLASS = {
    "li": "IMM", "addiu": "IMM", "ori": "IMM", "addi": "IMM",
    "move": "MOVE", "addu": "MOVE", "or": "MOVE", "add": "MOVE",
    "clear": "MOVE",
    "nop": "SHIFT", "sll": "SHIFT",
    "b": "BR", "beq": "BR", "beqz": "BR", "bnez": "BR", "bne": "BR",
    "bal": "BAL", "bgezal": "BAL",
    "negu": "SUBU", "subu": "SUBU", "not": "NOR", "nor": "NOR",
    "neg": "SUBU",
}

# Register aliases. Ghidra and capstone disagree on two ABI spellings, and an
# unfolded disagreement is invisible: the token differs, the dump silently
# fails to resolve, and it lands in NOT_FOUND looking like a capture of an
# un-extracted image. r30 is the one that bites - every function that saves a
# frame pointer touches it.
RCLASS = {"s8": "fp", "r30": "fp", "s9": "fp"}

# Branch/jump operands are PC-relative or absolute and therefore move with
# the load base - they must not enter the signature.
BRANCH = set(
    "b beq bne beqz bnez blez bgtz bltz bgez bltzal bgezal bal bc1t bc1f"
    " bgt blt bge ble j jal".split()
)
REGS = set(
    "zero at v0 v1 a0 a1 a2 a3 t0 t1 t2 t3 t4 t5 t6 t7 t8 t9 s0 s1 s2 s3"
    " s4 s5 s6 s7 k0 k1 gp sp fp s8 ra pc hi lo".split()
)
NUM = re.compile(r"-?0x[0-9a-fA-F]+|-?\d+")
TOK = re.compile(r"[a-zA-Z_][a-zA-Z0-9_]*")

_md = capstone.Cs(capstone.CS_ARCH_MIPS, capstone.CS_MODE_MIPS32 + capstone.CS_MODE_LITTLE_ENDIAN)
_md.skipdata = True


def canon(mnem, ops):
    """Base-independent token for one instruction."""
    mnem = mnem.lower().lstrip("_")
    ops = ops.replace("$", "").replace(" ", "").lower()
    cls = MCLASS.get(mnem, mnem.upper())
    regs = [RCLASS.get(t, t) for t in TOK.findall(ops) if t in REGS and t != "zero"]
    # Strip register names before reading immediates: `s8` and `a1` carry
    # digits that NUM would otherwise pick up as operand values, so a register
    # spelled two ways would perturb the immediate list as well as the
    # register list.
    imm_src = TOK.sub(lambda m: "" if m.group(0) in REGS else m.group(0), ops)
    imms = []
    if mnem not in BRANCH:
        for m in NUM.findall(imm_src):
            if m.startswith("-0x"):
                v = -int(m[3:], 16)
            elif m.lower().startswith("0x"):
                v = int(m, 16)
            else:
                v = int(m)
            if v != 0:
                imms.append(v)
    return "%s|%s|%s" % (cls, ",".join(regs), ",".join(map(str, imms)))


def canon_bytes(data, n_insns):
    out = []
    for ins in _md.disasm(data, 0x80000000):
        out.append(canon(ins.mnemonic, ins.op_str))
        if len(out) >= n_insns:
            break
    return out


def parse_dump(path, n):
    """(header_line, [(printed_va, canon_token)]) from a dump's disassembly."""
    hdr, rows, in_dis = None, [], False
    with open(path, "r", errors="replace") as f:
        for line in f:
            if hdr is None and line.startswith("=="):
                hdr = line.strip()
            if "--- DISASSEMBLY ---" in line:
                in_dis = True
                continue
            if not in_dis:
                continue
            s = line.rstrip("\n").strip()
            if not s:
                if rows:
                    break
                continue
            if s.startswith("---"):
                break
            m = re.match(r"^([0-9a-fA-F]{8})\s+(\S+)\s*(.*)$", s)
            if not m:
                continue
            rows.append((int(m.group(1), 16), canon(m.group(2), m.group(3) or "")))
            if len(rows) >= n:
                break
    return hdr, rows


# A body this short that exits through `j` is a label-call slice rather than
# a tail call - the standing example is the 4-instruction "function" whose
# only exit is `j 0x801ea7ac`.
SHORT_BODY_INSNS = 8

HDR_SIZE_RE = re.compile(r"^size=(\d+) bytes,\s*(\d+) instructions")
DIS_ROW_RE = re.compile(r"^([0-9a-fA-F]{8})\s+(\S+)\s*(.*)$")

SHAPES = ("SOUND", "TAIL_J", "NO_RETURN", "SIZE_MISMATCH",
          "HEADERLESS_WITH_DISASM", "HEADERLESS_C_ONLY", "EMPTY",
          "INTERIOR_SLICE")

# `funcs/` also holds the output of the non-dumping analysis scripts
# (`addprim_emitters_*`, `refs_to_*`, `inventory_*`). Those are not evidence
# about a function and must stay out of the denominator, or the defect rate
# is diluted by files that were never dumps. A dump is recognised by its
# name - `<addr>.txt` or `<label>_<addr>.txt` - or by carrying the standard
# `== name addr (entry=...) ==` header.
DUMP_NAME_RE = re.compile(r"^(?:.*_)?[0-9a-fA-F]{8}\.txt$")
DUMP_HDR_RE = re.compile(r"^==\s+\S+\s+[0-9a-fA-F]{8}\s+\(entry=")

# Worst first: what a re-dump pass should work through in order.
SHAPE_SEVERITY = {
    "INTERIOR_SLICE": 0,
    "HEADERLESS_C_ONLY": 1,
    "NO_RETURN": 2,
    "SIZE_MISMATCH": 3,
    "TAIL_J": 4,
    "HEADERLESS_WITH_DISASM": 5,
    "EMPTY": 6,
    "SOUND": 9,
}


def mark_interior_slices(infos):
    """Re-grade dumps whose entry lies inside another dump's function body.

    Ghidra mints a `FUN_` at intra-function labels - a jump-table arm, or a
    branch target it could not tie back - and a dump taken at one of those is
    not a short function, it is a SLICE of a larger one. It reads as
    authoritative (right bytes, right addresses) and cites an entry point
    that does not exist, so the fix is to retire the dump and the citation,
    NOT to re-dump the address.

    Detected structurally: an entry strictly inside a same-program sibling's
    [entry, entry + size) interval. Grouping is by the dump's filename prefix,
    which is the program label, so two overlays that alias in VA space are
    never compared against each other.
    """
    by_prog = defaultdict(list)
    for i in infos:
        if i["first_va"] is None:
            continue
        m = re.match(r"^(.*)_[0-9a-fA-F]{8}\.txt$", i["file"])
        by_prog[m.group(1) if m else ""].append(i)

    marked = 0
    for _prog, group in by_prog.items():
        spans = [(i["first_va"], i["first_va"] + i["declared_size"], i)
                 for i in group
                 if i["declared_size"] and i["shape"] in ("SOUND", "TAIL_J")]
        for i in group:
            va = i["first_va"]
            for lo, hi, owner in spans:
                if owner is i:
                    continue
                if lo < va < hi:
                    i["shape"] = "INTERIOR_SLICE"
                    i["owner"] = owner["file"]
                    marked += 1
                    break
    return marked


def scan_shape(path):
    """Classify one dump on the shape axis. Reads only the dump text."""
    declared_size = declared_insns = None
    rows = 0
    first_va = last_va = None
    last_mnems = []
    saw_c = False
    saw_dump_hdr = False
    body_chars = 0
    section = None

    with open(path, "r", errors="replace") as f:
        for line in f:
            if section is None:
                if DUMP_HDR_RE.match(line):
                    saw_dump_hdr = True
                m = HDR_SIZE_RE.match(line)
                if m:
                    declared_size = int(m.group(1))
                    declared_insns = int(m.group(2))
            if "--- DISASSEMBLY ---" in line:
                section = "dis"
                continue
            if "--- DECOMPILED ---" in line:
                section = "c"
                continue
            if section == "c":
                if line.strip():
                    saw_c = True
                continue
            if section != "dis":
                # Some dumps are bare decompiler output with no section
                # markers at all. Track that there is *content* so they are
                # graded C-only rather than empty.
                if line.strip():
                    body_chars += len(line)
                continue
            m = DIS_ROW_RE.match(line.rstrip("\n").strip())
            if not m:
                continue
            rows += 1
            if first_va is None:
                first_va = int(m.group(1), 16)
            last_va = int(m.group(1), 16)
            last_mnems.append(m.group(2).lower().lstrip("_"))
            if len(last_mnems) > 2:
                last_mnems.pop(0)

    base = os.path.basename(path)
    if not (saw_dump_hdr or DUMP_NAME_RE.match(base)):
        return None

    info = {
        "file": base,
        "declared_size": declared_size,
        "declared_insns": declared_insns,
        "rows": rows,
        "first_va": first_va,
        "last_va": last_va,
        "last_mnem": last_mnems[-1] if last_mnems else "",
        "has_c": saw_c,
    }

    # A `size=1 bytes, 0 instructions` header is Ghidra reporting that it
    # decoded nothing - the same defect as no header at all, so it is graded
    # by what the file actually carries, not by the header being parseable.
    headerless = declared_size is None or rows == 0
    if headerless:
        if rows > 0:
            info["shape"] = "HEADERLESS_WITH_DISASM"
        elif saw_c or body_chars:
            info["shape"] = "HEADERLESS_C_ONLY"
        else:
            info["shape"] = "EMPTY"
        return info

    if declared_size != rows * 4 or (declared_insns is not None
                                     and declared_insns != rows):
        info["shape"] = "SIZE_MISMATCH"
        return info

    # `jr` anywhere in the closing pair means the routine returns (the delay
    # slot legitimately prints last).
    if any(m == "jr" for m in last_mnems):
        info["shape"] = "SOUND"
    elif any(m in ("j", "b") for m in last_mnems):
        info["shape"] = "TAIL_J"
    else:
        info["shape"] = "NO_RETURN"
    return info


def cited_dump_names(docs_dir):
    """Dump basenames the committed docs currently cite as evidence.

    Two citation forms are in use: an explicit `funcs/<name>.txt` path, and a
    bare `FUN_<addr>` / backticked address that resolves to `<addr>.txt`. Both
    are collected, because a claim resting on either is a claim resting on
    that dump file.
    """
    names = set()
    path_re = re.compile(r"funcs/([A-Za-z0-9_.]+)\.txt")
    addr_re = re.compile(r"(?:FUN_|`)([0-9a-fA-F]{8})(?:`|\b)")
    for root, _dirs, files in os.walk(docs_dir):
        for fn in files:
            if not fn.endswith(".md"):
                continue
            with open(os.path.join(root, fn), "r", errors="replace") as f:
                text = f.read()
            for m in path_re.finditer(text):
                names.add(m.group(1) + ".txt")
            for m in addr_re.finditer(text):
                names.add(m.group(1).lower() + ".txt")
    return names


def run_shape(args):
    files = sorted(glob.glob(os.path.join(args.funcs_dir, "*.txt")))
    cited = None
    if args.cited_only:
        cited = cited_dump_names(os.path.join(ROOT, "docs"))
        print("[dump-shape] docs cite %d distinct dump name(s)" % len(cited))

    infos, skipped, not_a_dump = [], 0, 0
    for path in files:
        base = os.path.basename(path)
        if cited is not None:
            # An overlay dump is cited by its bare address too, so match on
            # the trailing `<addr>.txt` as well as the whole filename.
            tail = base.split("_")[-1]
            if base not in cited and tail not in cited:
                skipped += 1
                continue
        try:
            info = scan_shape(path)
        except Exception as e:
            print("  [parse-err] %s: %s" % (base, e))
            continue
        if info is None:
            not_a_dump += 1
            continue
        infos.append(info)

    # Orthogonal to shape: does the FILENAME's address agree with the address
    # the content actually starts at? The dumpers resolve a target with
    # `getFunctionContaining(addr)` but name the file after the *requested*
    # address, so asking for an interior address silently yields a file named
    # `<interior>.txt` holding the enclosing function. Every citation of that
    # filename then asserts a function entry that does not exist.
    name_addr_re = re.compile(r"(?:^|_)([0-9a-fA-F]{8})\.txt$")
    mismatched = []
    for i in infos:
        m = name_addr_re.search(i["file"])
        if not m or i["first_va"] is None:
            continue
        want = int(m.group(1), 16)
        if want != i["first_va"]:
            i["name_va"] = want
            mismatched.append(i)

    n_interior = mark_interior_slices(infos)

    cat = Counter(i["shape"] for i in infos)
    scope = "cited" if cited is not None else "all"
    print("\n=== shape classification (%d %s dump(s)%s) ===" % (
        len(infos), scope, ", %d skipped" % skipped if skipped else ""))
    for k in SHAPES:
        if cat[k]:
            print("  %-24s %5d" % (k, cat[k]))
    if not_a_dump:
        print("  (%d non-dump file(s) in funcs/ excluded from the denominator)"
              % not_a_dump)

    if n_interior:
        print("\n%d dump(s) are a SLICE of a larger sibling function, not an "
              "entry point. Retire the dump and the citation - do not "
              "re-dump the address." % n_interior)

    if mismatched:
        print("\n%d dump(s) are NAME-MISMATCHED: the filename's address is not "
              "where the content starts, so the file asserts a function entry "
              "that does not exist." % len(mismatched))
        for i in sorted(mismatched, key=lambda i: i["file"])[:args.limit]:
            print("  %-48s named %08x  starts %08x  (%+d)" % (
                i["file"], i["name_va"], i["first_va"],
                i["first_va"] - i["name_va"]))

    recoverable = [i for i in infos if i["shape"] == "HEADERLESS_WITH_DISASM"]
    if recoverable:
        insns = sum(i["rows"] for i in recoverable)
        print("\n%d headerless dump(s) carry a complete disassembly "
              "(%d instructions, ~%d bytes) that every header-driven counter "
              "discards." % (len(recoverable), insns, insns * 4))

    worst = sorted(
        (i for i in infos if i["shape"] != "SOUND"),
        key=lambda i: (SHAPE_SEVERITY.get(i["shape"], 9), -i["rows"], i["file"]))
    if args.list_defects:
        print("\n=== defective dumps, worst first ===")
        for i in worst[:args.limit]:
            flag = ""
            if i["shape"] == "TAIL_J" and i["rows"] < SHORT_BODY_INSNS:
                flag = "  (short body - likely label-call slice)"
            elif i["shape"] == "INTERIOR_SLICE":
                flag = "  inside %s" % i.get("owner", "?")
            print("  %-24s %-44s rows=%-5d last=%-6s%s" % (
                i["shape"], i["file"], i["rows"], i["last_mnem"], flag))

    if args.emit_csv:
        with open(args.emit_csv, "w") as f:
            f.write("shape,file,declared_size,rows,first_va,last_va,last_mnem\n")
            for i in sorted(infos, key=lambda i: (
                    SHAPE_SEVERITY.get(i["shape"], 9), i["file"])):
                f.write("%s,%s,%s,%d,%s,%s,%s\n" % (
                    i["shape"], i["file"],
                    "" if i["declared_size"] is None else i["declared_size"],
                    i["rows"],
                    "" if i["first_va"] is None else "%08x" % i["first_va"],
                    "" if i["last_va"] is None else "%08x" % i["last_va"],
                    i["last_mnem"]))
        print("\nwrote %s (verdicts only - no dump text)" % args.emit_csv)

    return 0


def load_images():
    """{name: (bytes, base_va, header_len)} for every extracted image."""
    images = {}
    if os.path.exists(SCUS):
        images["SCUS_942.54"] = (open(SCUS, "rb").read(), SCUS_BASE, SCUS_HEADER)
    bases = {}
    if os.path.exists(OVERLAY_MAP):
        with open(OVERLAY_MAP, "rb") as f:
            for o in tomllib.load(f).get("overlays", []):
                bases[o["label"]] = o.get("base_va")
    for path in sorted(glob.glob(os.path.join(OVERLAYS, "*.bin"))):
        name = os.path.basename(path)
        # overlay_<label>_<prot>.bin
        m = re.match(r"overlay_(.+)_(\d+)\.bin$", name)
        label = m.group(1) if m else None
        images[name] = (open(path, "rb").read(), bases.get(label), 0)
    return images


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--min-insns", type=int, default=10,
                    help="signature length; shorter dumps are reported SHORT (default 10)")
    ap.add_argument("--list-shifted", action="store_true",
                    help="print every SHIFTED dump, not just the delta histogram")
    ap.add_argument("--funcs-dir", default=FUNCS)
    ap.add_argument("--shape", action="store_true",
                    help="run the SHAPE axis (truncation / headerless) instead "
                         "of the base axis; needs no extracted/ tree")
    ap.add_argument("--cited-only", action="store_true",
                    help="shape axis: restrict to dumps the docs cite as evidence")
    ap.add_argument("--list-defects", action="store_true",
                    help="shape axis: list every non-SOUND dump, worst first")
    ap.add_argument("--limit", type=int, default=60,
                    help="shape axis: cap --list-defects output (default 60)")
    ap.add_argument("--emit-csv", metavar="PATH",
                    help="shape axis: write per-dump verdicts to CSV")
    args = ap.parse_args()

    if args.shape:
        return run_shape(args)

    images = load_images()
    if not images:
        print("error: no extracted images found - run the extraction first "
              "(see docs/tooling/extraction.md)", file=sys.stderr)
        return 2
    print("[dump-base-integrity] indexing %d image(s)" % len(images))

    n = args.min_insns
    index = defaultdict(list)
    for name, (data, base, hdrlen) in images.items():
        toks = canon_bytes(data, len(data) // 4)
        for i in range(len(toks) - n):
            index["\x00".join(toks[i:i + n])].append((name, i * 4))

    cat = Counter()
    deltas = Counter()
    shifted = []
    files = sorted(glob.glob(os.path.join(args.funcs_dir, "*.txt")))
    for path in files:
        try:
            hdr, rows = parse_dump(path, n)
        except Exception:
            cat["PARSE_ERR"] += 1
            continue
        if hdr is None or len(rows) < n:
            cat["SHORT"] += 1
            continue
        va0 = rows[0][0]
        hits = index.get("\x00".join(r[1] for r in rows), [])
        ivas = []
        for name, off in hits:
            _, base, hdrlen = images[name]
            if base is not None:
                ivas.append((name, base + off - hdrlen))
        if not hits:
            cat["NOT_FOUND"] += 1
            continue
        if not ivas:
            cat["FOUND_NO_BASE"] += 1
            continue
        if va0 in [v for _, v in ivas]:
            cat["MATCH"] += 1
            continue
        name, iva = min(ivas, key=lambda t: abs(t[1] - va0))
        d = iva - va0
        deltas[d] += 1
        cat["SHIFTED"] += 1
        shifted.append((os.path.basename(path), va0, d, name, iva, len(hits)))

    print("\n=== classification (%d dumps) ===" % len(files))
    for k in ("MATCH", "SHIFTED", "NOT_FOUND", "SHORT", "FOUND_NO_BASE", "PARSE_ERR"):
        if cat[k]:
            print("  %-14s %5d" % (k, cat[k]))

    print("\n=== base-error histogram ===")
    for d, c in deltas.most_common(20):
        print("  %+#010x  %4d" % (d, c))

    if args.list_shifted:
        print("\n=== SHIFTED dumps ===")
        for nm, va0, d, img, iva, nh in sorted(shifted, key=lambda r: (-abs(r[2]), r[0])):
            print("  %-44s printed %08x  real %08x  %+#x  %s%s"
                  % (nm, va0, iva, d, img, "" if nh == 1 else "  (%d hits)" % nh))

    # A single dominant delta means one mis-based batch run, not scattered
    # one-offs; that is the finding worth acting on.
    return 1 if cat["SHIFTED"] else 0


if __name__ == "__main__":
    sys.exit(main())
