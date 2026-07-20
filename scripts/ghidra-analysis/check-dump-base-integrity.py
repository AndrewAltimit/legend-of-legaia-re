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

Usage:
  scripts/ghidra-analysis/check-dump-base-integrity.py
  scripts/ghidra-analysis/check-dump-base-integrity.py --min-insns 10
  scripts/ghidra-analysis/check-dump-base-integrity.py --list-shifted
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
    args = ap.parse_args()

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
