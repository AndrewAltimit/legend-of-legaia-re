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
  SIZE_MISMATCH  the header's instruction count and the stream disagree.
  BODY_WITH_DATA  the counts agree but the byte span is larger: a Ghidra body
               is an address set, so an inline jump table or alignment padding
               makes a routine span more bytes than it has instructions. A
               fact about the routine, not a defect - reported, never counted.
  ADDRLESS_DISASM  a whole, correct instruction stream printed WITHOUT its
               address column, because the dumper wrote `ins` instead of
               `ins.getAddress(), ins`. Nothing is missing except the one
               field every address-keyed instrument reads, so the file is
               complete evidence that reads as empty. Repairable in place
               (a body is contiguous, so address i is `entry + 4*i`).
  HEADERLESS_WITH_DISASM  no parseable `size=` header, but a complete
               disassembly is present. The dump is usable evidence that
               every header-driven counter silently discards - it makes
               coverage look worse than it is, not the corpus thinner.
  HEADERLESS_C_ONLY  no header and no disassembly. Only a C rendering, which
               this repo's own rules say is not evidence. Needs a re-dump.

The shape axis reads only the dump text, so unlike the base axis it needs no
`extracted/` tree: `--shape` runs anywhere.

--- What is not a dump ---

`funcs/` also holds files that answer a question about an address without
being a dump of a function, and counting those as defective dumps inflates the
defect rate with the corpus's own correct answers. Three kinds exist and each
is reported separately, never as a defect:

  POINTER_STUB   `== citation pointer 0x<addr> ==` or `== <addr> (cite of
                 FUN_<addr>) ==`. A mid-function citation, recorded as a file
                 that names the enclosing dump. This is the RIGHT handling of
                 an interior address - the alternative is a file that asserts
                 an entry point which does not exist.
  NOFUNC_RECORD  `== NOFUNC <addr> ==`, or a `--- PSEUDO-DISASSEMBLY WINDOW`
                 section. A recorded negative: Ghidra has no function at or
                 containing the address. The window is a read of the bytes,
                 explicitly not a function body.
  NOT_A_DUMP     an analysis script's output (`jal_allprogs_*`, `jalraw_*`,
                 `addrconst_*`, jump/handler-table dumps) whose filename
                 happens to end `_<addr>.txt`.

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
import json
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
BASE_BASELINE = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                             "dump-base-baseline.json")

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

# `break`'s operand is a 20-bit code the two disassemblers split differently:
# Ghidra prints the whole field (`break 0x1c00`), capstone a sub-field
# (`break 7`). The instruction is the same instruction, so the immediate is
# dropped on both sides rather than compared.
#
# It matters far out of proportion to its size. `div; bne; break 0x1c00` is the
# signed-division overflow guard the compiler emits at every integer divide, so
# an unfolded `break` makes any window containing a division fail to match the
# image it came from - and the failure lands in NOT_FOUND, the class that reads
# as "no extracted image holds these bytes", i.e. as a fact about the corpus
# rather than about the comparison. It was 24 of 25 systematic disagreements
# when the two instruments were cross-checked.
NO_IMM = frozenset(("break", "syscall"))

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
    if mnem not in BRANCH and mnem not in NO_IMM:
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

SHAPES = ("SOUND", "TAIL_J", "NO_RETURN", "SIZE_MISMATCH", "ADDRLESS_DISASM",
          "HEADERLESS_WITH_DISASM", "HEADERLESS_C_ONLY", "EMPTY",
          "INTERIOR_SLICE")

# Classes that are a correct answer stored in `funcs/`, not a defective dump.
# They are reported, never counted as defects. See the module docstring.
RECORD_CLASSES = ("POINTER_STUB", "NOFUNC_RECORD", "NOT_A_DUMP",
                  "BODY_WITH_DATA")

# `funcs/` also holds the output of the non-dumping analysis scripts
# (`addprim_emitters_*`, `refs_to_*`, `inventory_*`). Those are not evidence
# about a function and must stay out of the denominator, or the defect rate
# is diluted by files that were never dumps. A dump is recognised by its
# name - `<addr>.txt` or `<label>_<addr>.txt` - or by carrying the standard
# `== name addr (entry=...) ==` header.
DUMP_NAME_RE = re.compile(r"^(?:.*_)?[0-9a-fA-F]{8}\.txt$")

# Header spellings actually present in the corpus. The `entry=` field is the
# one that identifies a file as a function dump, and three dumpers spell it
# differently: bare hex, `0x`-prefixed, and hex followed by a `, label=` field.
# A stricter pattern silently drops real dumps into HEADERLESS_*, which reads
# as a corpus gap rather than as a regex that is too narrow.
DUMP_HDR_RE = re.compile(r"^==\s+.*\(entry=(?:0x)?[0-9a-fA-F]{8}")

# A mid-function citation recorded as its own file, in either spelling. This is
# the corpus doing the right thing: the alternative is a dump file whose name
# asserts an entry point that does not exist.
POINTER_STUB_RE = re.compile(
    r"^==\s+(?:citation pointer\b"
    r"|[0-9a-fA-F]{8}\s+\(cite of\b)")

# A recorded negative - no analyzed function at or containing the address.
NOFUNC_HDR_RE = re.compile(r"^==\s+NOFUNC\b")

# Section markers. `--- DECOMPILED C ---` is one dumper's spelling of
# `--- DECOMPILED ---`; unrecognised, its C body is read as disassembly rows.
# `--- PSEUDO-DISASSEMBLY WINDOW ---` is deliberately NOT a function body: it
# is a read of the bytes at an address Ghidra has no function for.
DIS_MARK_RE = re.compile(r"^---\s*DISASSEMBLY")
PSEUDO_MARK_RE = re.compile(r"^---\s*PSEUDO-DISASSEMBLY")
C_MARK_RE = re.compile(r"^---\s*DECOMPILED")

# Worst first: what a re-dump pass should work through in order.
SHAPE_SEVERITY = {
    "INTERIOR_SLICE": 0,
    "HEADERLESS_C_ONLY": 1,
    "NO_RETURN": 2,
    "ADDRLESS_DISASM": 3,
    "SIZE_MISMATCH": 4,
    "TAIL_J": 5,
    "HEADERLESS_WITH_DISASM": 6,
    "EMPTY": 7,
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
    rows = bare_rows = 0
    first_va = last_va = None
    last_mnems = []
    saw_c = False
    saw_dump_hdr = False
    record_class = None
    body_chars = 0
    section = None

    with open(path, "r", errors="replace") as f:
        for line in f:
            s = line.rstrip("\n").strip()
            if section is None:
                if DUMP_HDR_RE.match(line):
                    saw_dump_hdr = True
                if record_class is None:
                    if POINTER_STUB_RE.match(line):
                        record_class = "POINTER_STUB"
                    elif NOFUNC_HDR_RE.match(line):
                        record_class = "NOFUNC_RECORD"
                m = HDR_SIZE_RE.match(line)
                if m:
                    declared_size = int(m.group(1))
                    declared_insns = int(m.group(2))
            if PSEUDO_MARK_RE.match(s):
                # Explicitly not a function body - a read of the bytes at an
                # address Ghidra has no function for.
                record_class = "NOFUNC_RECORD"
                section = "pseudo"
                continue
            if DIS_MARK_RE.match(s):
                section = "dis"
                continue
            if C_MARK_RE.match(s):
                section = "c"
                continue
            if section in ("c", "pseudo"):
                if s:
                    saw_c = True
                continue
            if section != "dis":
                # Some dumps are bare decompiler output with no section
                # markers at all. Track that there is *content* so they are
                # graded C-only rather than empty.
                if s:
                    body_chars += len(line)
                continue
            if not s or s.startswith("---"):
                continue
            m = DIS_ROW_RE.match(s)
            if not m:
                # A stream row without its address column. The instruction is
                # there; only the field every address-keyed instrument reads
                # is missing.
                bare_rows += 1
                last_mnems.append(s.split()[0].lower().lstrip("_"))
                if len(last_mnems) > 2:
                    last_mnems.pop(0)
                continue
            rows += 1
            if first_va is None:
                first_va = int(m.group(1), 16)
            last_va = int(m.group(1), 16)
            last_mnems.append(m.group(2).lower().lstrip("_"))
            if len(last_mnems) > 2:
                last_mnems.pop(0)

    base = os.path.basename(path)
    if record_class is not None:
        return {"file": base, "shape": record_class, "record": True,
                "rows": rows, "declared_size": declared_size,
                "declared_insns": declared_insns, "first_va": first_va,
                "last_va": last_va, "last_mnem": "", "has_c": saw_c}
    if not (saw_dump_hdr or DUMP_NAME_RE.match(base)):
        return None
    if not saw_dump_hdr and rows == 0 and bare_rows == 0:
        # Filename ends `_<addr>.txt` but the file carries neither a dump
        # header nor an instruction stream: an analysis script's output.
        return {"file": base, "shape": "NOT_A_DUMP", "record": True,
                "rows": 0, "declared_size": None, "declared_insns": None,
                "first_va": None, "last_va": None, "last_mnem": "",
                "has_c": saw_c}

    info = {
        "file": base,
        "declared_size": declared_size,
        "declared_insns": declared_insns,
        "rows": rows,
        "bare_rows": bare_rows,
        "first_va": first_va,
        "last_va": last_va,
        "last_mnem": last_mnems[-1] if last_mnems else "",
        "has_c": saw_c,
    }

    # A whole stream with no address column at all. Graded before the
    # headerless test, because `rows == 0` would otherwise send a complete
    # dump to HEADERLESS_C_ONLY - which reads as "no evidence here" when the
    # evidence is all present and merely unlabelled.
    if rows == 0 and bare_rows > 0:
        info["shape"] = "ADDRLESS_DISASM"
        return info

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

    if declared_insns is not None and declared_insns != rows:
        info["shape"] = "SIZE_MISMATCH"
        return info
    if declared_size != rows * 4:
        # The instruction count agrees with the stream; only the byte count is
        # larger. A Ghidra body is an ADDRESS SET, so a routine with an inline
        # jump table or alignment padding legitimately spans more bytes than it
        # has instructions - which is a fact about the routine, not a defect in
        # the dump. Conflating the two makes every big overlay dispatcher (the
        # ones whose bodies most often need repairing) report as broken right
        # after it is repaired.
        info["shape"] = "BODY_WITH_DATA"
        info["record"] = True
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

    infos, records, skipped, not_a_dump = [], [], 0, 0
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
        if info.get("record"):
            records.append(info)
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

    if records:
        rcat = Counter(i["shape"] for i in records)
        print("\n=== recorded answers, not defective dumps (%d) ==="
              % len(records))
        for k in RECORD_CLASSES:
            if rcat[k]:
                print("  %-24s %5d" % (k, rcat[k]))
        print("  Each is a correct answer about an address stored as a file. "
              "Counting them as defects inflates the defect rate with the "
              "corpus's own good work.")

    if n_interior:
        print("\n%d dump(s) are a SLICE of a larger sibling function, not an "
              "entry point. Retire the dump and the citation - do not "
              "re-dump the address." % n_interior)

    if mismatched:
        neg = sum(1 for i in mismatched if i["first_va"] < i["name_va"])
        print("\n%d dump(s) are NAME-MISMATCHED: the filename's address is not "
              "where the content starts, so the file asserts a function entry "
              "that does not exist." % len(mismatched))
        print("  %d of %d deltas are NEGATIVE - the resolved entry sits BELOW "
              "the requested address. Nothing but `getFunctionContaining()` "
              "paired with a requested-address filename produces that "
              "signature, so it identifies the defect rather than merely "
              "being consistent with it." % (neg, len(mismatched)))
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

    addrless = [i for i in infos if i["shape"] == "ADDRLESS_DISASM"]
    if addrless:
        insns = sum(i.get("bare_rows", 0) for i in addrless)
        print("\n%d dump(s) carry a whole instruction stream with NO address "
              "column (%d instructions). Complete evidence that reads as "
              "empty to anything keyed on addresses." % (len(addrless), insns))

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
            for i in sorted(infos + records, key=lambda i: (
                    SHAPE_SEVERITY.get(i["shape"], 9), i["file"])):
                f.write("%s,%s,%s,%d,%s,%s,%s\n" % (
                    i["shape"], i["file"],
                    "" if i["declared_size"] is None else i["declared_size"],
                    i["rows"] or i.get("bare_rows", 0),
                    "" if i["first_va"] is None else "%08x" % i["first_va"],
                    "" if i["last_va"] is None else "%08x" % i["last_va"],
                    i["last_mnem"]))
        print("\nwrote %s (verdicts only - no dump text)" % args.emit_csv)

    return 0


GHIDRA_SCRIPTS = os.path.join(ROOT, "ghidra", "scripts")

# The three source-level defects behind the corpus classes above. Each is a
# one-line mistake that leaves the dump internally consistent, so nothing
# downstream can tell a defective dump from a sound one - only the script can.
DUMPER_DEFECTS = (
    ("NAME_MISMATCH",
     "names its output from the REQUESTED address while resolving with "
     "getFunctionContaining() - the file asserts an entry point that may not "
     "exist"),
    ("ADDRLESS",
     "writes `ins` without `ins.getAddress()` - a whole correct stream with "
     "no address column, which every address-keyed instrument reads as empty"),
    ("C_MARKER",
     "emits `--- DECOMPILED C ---`; readers expecting `--- DECOMPILED ---` "
     "parse the C body as more disassembly"),
)

# `def out_path_for(addr_str)` is the helper's own signature, not a call
# naming a file after a requested address - a negative lookbehind on `def `
# keeps the audit from flagging every script that defines the helper
# correctly. (The audit is itself an instrument, so it gets the same scrutiny:
# a checker that cries wolf on correct code is worse than no checker.)
_OUT_REQUESTED_RE = re.compile(
    r"(?<!def )out_path_for\(\s*addr_str\s*\)|OUT_DIR\s*,\s*addr_str\s*\+")
_ADDRLESS_RE = re.compile(r"\.write\(\s*[\"']\{\}\\n[\"']\.format\(ins\)")
# Must be a `write(...)` of the marker, not a mention of it in a comment or
# docstring - a script that documents the defect it no longer has is correct.
_C_MARKER_RE = re.compile(r"\.write\([^\n]*---\s*DECOMPILED C\s*---")


def audit_dumpers(scripts_dir):
    """Which dump scripts still carry each defect. Reads source, not dumps."""
    found = []
    for path in sorted(glob.glob(os.path.join(scripts_dir, "*.py"))):
        try:
            text = open(path, "r", errors="replace").read()
        except OSError:
            continue
        if "--- DISASSEMBLY ---" not in text:
            continue
        flags = []
        if "getFunctionContaining" in text and _OUT_REQUESTED_RE.search(text):
            flags.append("NAME_MISMATCH")
        if _ADDRLESS_RE.search(text):
            flags.append("ADDRLESS")
        if _C_MARKER_RE.search(text):
            flags.append("C_MARKER")
        if flags:
            found.append((os.path.basename(path), flags))
    return found


def run_audit_dumpers(args):
    found = audit_dumpers(args.scripts_dir)
    print("=== dump-script audit (%s) ===" % args.scripts_dir)
    for key, why in DUMPER_DEFECTS:
        hits = [n for n, f in found if key in f]
        print("\n%s - %d script(s)" % (key, len(hits)))
        print("  %s" % why)
        for n in hits:
            print("    %s" % n)
    print("\n%d script(s) carry at least one defect." % len(found))
    print("The corpus classes are downstream of these: repairing dumps "
          "without repairing the script that wrote them regenerates the "
          "defect on the next run.")
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
    ap.add_argument("--emit-base-csv", metavar="PATH",
                    help="base axis: write per-dump printed/resolved VA + delta "
                         "+ image to CSV - what a re-dump pass needs to pick a "
                         "program from the bytes rather than the filename")
    ap.add_argument("--audit-dumpers", action="store_true",
                    help="report which ghidra/scripts/*.py still carry each "
                         "source-level dumper defect; needs neither dumps nor "
                         "extracted/")
    ap.add_argument("--scripts-dir", default=GHIDRA_SCRIPTS)
    ap.add_argument("--check", action="store_true",
                    help="gate mode: fail only on a dump that is SHIFTED and "
                         "absent from the committed baseline; skip cleanly "
                         "where the corpus or extracted/ is absent")
    ap.add_argument("--update-baseline", action="store_true",
                    help="rewrite the baseline from this corpus (say why in "
                         "the commit message)")
    args = ap.parse_args()

    if args.audit_dumpers:
        return run_audit_dumpers(args)
    if args.shape:
        return run_shape(args)

    if args.check and not os.path.isdir(args.funcs_dir):
        print("[dump-base-integrity] SKIPPED - no dump corpus (gitignored).")
        return 0

    images = load_images()
    if not images:
        if args.check:
            print("[dump-base-integrity] SKIPPED - no extracted/ images "
                  "(gitignored; this gate only measures locally).")
            return 0
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
    per_dump = []
    files = sorted(glob.glob(os.path.join(args.funcs_dir, "*.txt")))
    for path in files:
        base_name = os.path.basename(path)
        try:
            hdr, rows = parse_dump(path, n)
        except Exception:
            cat["PARSE_ERR"] += 1
            per_dump.append((base_name, "PARSE_ERR", None, None, None, "", 0))
            continue
        if hdr is None or len(rows) < n:
            cat["SHORT"] += 1
            per_dump.append((base_name, "SHORT",
                             rows[0][0] if rows else None, None, None, "", 0))
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
            per_dump.append((base_name, "NOT_FOUND", va0, None, None, "", 0))
            continue
        if not ivas:
            cat["FOUND_NO_BASE"] += 1
            per_dump.append((base_name, "FOUND_NO_BASE", va0, None, None,
                             "|".join(sorted({h[0] for h in hits})), len(hits)))
            continue
        if va0 in [v for _, v in ivas]:
            cat["MATCH"] += 1
            img = "|".join(sorted({nm for nm, v in ivas if v == va0}))
            per_dump.append((base_name, "MATCH", va0, va0, 0, img, len(hits)))
            continue
        name, iva = min(ivas, key=lambda t: abs(t[1] - va0))
        d = iva - va0
        deltas[d] += 1
        cat["SHIFTED"] += 1
        shifted.append((base_name, va0, d, name, iva, len(hits)))
        per_dump.append((base_name, "SHIFTED", va0, iva, d, name, len(hits)))

    if args.update_baseline:
        with open(BASE_BASELINE, "w") as f:
            json.dump({"shifted": sorted(nm for nm, _, _, _, _, _ in shifted)},
                      f, indent=2)
            f.write("\n")
        print("[dump-base-integrity] baseline updated: %s (%d SHIFTED dump(s))"
              % (BASE_BASELINE, len(shifted)))
        return 0

    if args.check:
        # Ratchet on the SHIFTED *set*, not on its size. The corpus grows every
        # time an overlay is imported, so a count ratchet would fire on healthy
        # growth and stay silent when a mis-based dump replaced a sound one.
        # NOT_FOUND is deliberately outside the ratchet: the docstring above
        # grades it UNVERIFIABLE rather than known-bad, and gating on it would
        # fail every RAM-capture-derived dump.
        if not os.path.exists(BASE_BASELINE):
            print("[dump-base-integrity] no baseline yet; run "
                  "--update-baseline once.")
            return 0
        with open(BASE_BASELINE) as f:
            known = set(json.load(f).get("shifted", []))
        seen = {nm for nm, _, _, _, _, _ in shifted}
        for nm in sorted(known - seen):
            print("[dump-base-integrity] baselined dump %s is no longer "
                  "SHIFTED - re-run --update-baseline to tighten the ratchet"
                  % nm)
        new = sorted(seen - known)
        if new:
            by_name = {r[0]: r for r in shifted}
            print("[dump-base-integrity] %d dump(s) print addresses their "
                  "bytes do not occupy, and are not in the baseline:" % len(new))
            for nm in new:
                _, va0, d, img, iva, _ = by_name[nm]
                print("   %-44s printed %08x  real %08x  %+#x  %s"
                      % (nm, va0, iva, d, img))
            print("[dump-base-integrity] a filename prefix is not evidence of "
                  "the load base - only the resolved bytes are. Re-dump at the "
                  "resolved base, or --update-baseline and say why in the "
                  "commit message. See docs/tooling/dump-corpus-integrity.md.")
            return 1
        print("[dump-base-integrity] OK - %d MATCH, %d baselined SHIFTED, no "
              "new mis-based dump." % (cat["MATCH"], len(seen)))
        return 0

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

    if args.emit_base_csv:
        # Per-dump base verdicts. A re-dump pass has to be told which program
        # to run against, and the filename is not evidence of that - this is.
        with open(args.emit_base_csv, "w") as f:
            f.write("file,class,printed_va,resolved_va,delta,image,hits\n")
            for nm, cls, va0, iva, d, img, nh in sorted(per_dump):
                f.write("%s,%s,%s,%s,%s,%s,%d\n" % (
                    nm, cls,
                    "" if va0 is None else "%08x" % va0,
                    "" if iva is None else "%08x" % iva,
                    "" if d is None else "%d" % d, img, nh))
        print("\nwrote %s (verdicts only - no dump text)" % args.emit_base_csv)

    # A single dominant delta means one mis-based batch run, not scattered
    # one-offs; that is the finding worth acting on.
    return 1 if cat["SHIFTED"] else 0


if __name__ == "__main__":
    sys.exit(main())
