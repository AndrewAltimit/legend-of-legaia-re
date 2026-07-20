#!/usr/bin/env python3
"""Classify the port-catalog "dumped + documented + not ported" worklist.

`scripts/ci/port-catalog.py --missing-ports` counts one row per address. Not
every row is a portable function entry: Ghidra promotes intra-function labels to
fake `FUN_` entries, overlays hold relocated copies of one routine, some dumps
are data regions, and some addresses resolve to a different enclosing function.
This script reads the Ghidra dumps under `ghidra/scripts/funcs/` and assigns
every worklist address a class plus a mechanical, restatable reason.

Classes

    REAL             self-entry dump whose body returns via `jr ra`
    INTERIOR         the dump resolves `entry=` to a different address, is an
                     explicit citation stub, or another dumped body in the same
                     image disassembles an instruction at this exact VA
    PHANTOM          no body of its own - the only dump is a degenerate stub, or
                     a Ghidra `caseD_` switch-case stub
    SHARED_TAIL      a distinct body with no `jr ra` that exits by jumping into
                     code it does not own: an entry into a multi-entry routine,
                     not an independently callable function
    DATA             the dump is a data-region / hex-blob / pointer-table listing
    DUPLICATE        instruction stream identical modulo relocated absolute
                     addresses to another worklist address or a ported function,
                     or a truncated dump that is a strict prefix of one
    VA_ALIASED       two or more images dump distinct bodies at this VA, so the
                     row is not one port site
    REAL_BUT_VENDOR  real function, but PsyQ / BIOS / libgte / libspu glue
    UNCERTAIN        evidence is thin or the heuristics disagree

Invocation:

    python3 scripts/ghidra-analysis/classify-worklist.py \
        --repo /path/to/legend-of-legaia-re \
        --catalog /path/to/legend-of-legaia-re/target/port-catalog/catalog.csv \
        --out target/worklist-classification.csv \
        --ignore-out scripts/ci/proposed-ignore-additions.toml

`--repo` must point at a checkout whose `ghidra/scripts/funcs/` is populated.
That directory is gitignored, so a fresh worktree has none; point `--repo` at
the checkout that ran the Ghidra dumps and `--out` wherever you want the
artifact.

The output carries addresses, classes and short reasons only - never dump text -
so it is safe to commit. The dumps themselves are Sony-derived and are not.
"""

import argparse
import csv
import os
import re
import sys
from collections import Counter, defaultdict

# --- dump header forms -----------------------------------------------------
# == FUN_<name> <addr> (entry=<entry>) [<source>] ==
# == FUN_<name> <addr> ==                      (entry omitted; entry == addr)
# == <name> 0x<ADDR> (entry=<entry>) [<src>] == (0x-prefixed VA column)
HDR_FUNC = re.compile(
    r"^==\s+(\S+)\s+(?:0x)?([0-9a-fA-F]{8})\s*"
    r"(?:\(entry=(?:0x)?([0-9a-fA-F]{8})\))?"
    r"(?:\s+\[([^\]]*)\])?"
)
# == <name> <addr> (len=N) ==  followed by a `Hex:` block - a data blob dump.
HDR_HEXBLOB = re.compile(r"^==\s+\S+\s+(?:0x)?[0-9a-fA-F]{8}\s+\(len=\d+\)")
HDR_DATA = re.compile(r"^==\s+\S*\s*DATA REGION", re.IGNORECASE)
# -- <addr> in <image> (entry <entry>)   -- the overlay-sweep dump header.
HDR_DASH = re.compile(
    r"^--\s+([0-9a-fA-F]{8})\s+in\s+(\S+)\s+\(entry\s+(?:0x)?([0-9a-fA-F]{8})\)",
    re.IGNORECASE,
)
# A pointer/handler table dump: `Base: 0x... Entries: 0x...`.
HDR_TABLE = re.compile(r"^Base:\s*0x[0-9a-fA-F]+\s+Entries:", re.IGNORECASE)
HDR_CITE_PTR = re.compile(r"^==\s+citation pointer\s+0x([0-9a-fA-F]{8})", re.IGNORECASE)
HDR_CITE_OF = re.compile(
    r"^==\s+([0-9a-fA-F]{8})\s+\(cite of\s+FUN_([0-9a-fA-F]{8})\)", re.IGNORECASE
)

SIZE_RE = re.compile(r"^size=(\d+)\s+bytes,\s+(\d+)\s+instructions")
INSN_RE = re.compile(r"^([0-9a-f]{8})\s\s(_?)([a-z0-9.]+)\s*(.*)$")
SECTION_RE = re.compile(r"^---\s*(.*?)\s*---\s*$")

# Absolute code/data addresses; masked so a relocated copy hashes equal.
ABS_ADDR_RE = re.compile(r"0x[0-9a-f]{6,8}")
# Every hex immediate; the looser mask, reported only as a weak signal.
ANY_IMM_RE = re.compile(r"-?0x[0-9a-f]+")

# Ghidra's synthetic switch-case labels.
CASE_LABEL_RE = re.compile(r"\bcaseD_[0-9a-fA-F]+\b")

# BIOS thunk shape: li tN, 0xA0/0xB0/0xC0 then jr tN.
BIOS_VEC_RE = re.compile(r"\b(?:li|addiu)\s+t\d,(?:\s*\w+,)?\s*0x[abc]0\b")

# A dump body at or below this instruction count carries no useful evidence:
# Ghidra emits these when a VA is defined as a function in one program image but
# holds a jump stub or nothing at all in the image that was dumped.
STUB_INSNS = 4
# Below this, two streams matching is not strong duplicate evidence - short
# thunks alias trivially.
MIN_DUP_INSNS = 6

VENDOR_HINTS = ("libgte", "libspu", "libsnd", "libcd", "psyq")


def parse_dump(path):
    """Parse one dump file into a record dict."""
    try:
        with open(path, "r", errors="ignore") as fh:
            lines = fh.read().splitlines()
    except OSError:
        return None
    rec = {
        "stem": os.path.basename(path)[:-4],
        "kind": "other",
        "addr": None,
        "entry": None,
        "source": "",
        "size": None,
        "ninsn": None,
        "insns": [],
        "decomp": [],
        "cite_of": None,
    }
    hdr = lines[0] if lines else ""
    m = HDR_CITE_OF.match(hdr)
    md = HDR_DASH.match(hdr)
    if HDR_DATA.match(hdr) or HDR_HEXBLOB.match(hdr):
        rec["kind"] = "data"
    elif any(HDR_TABLE.match(ln) for ln in lines[:6]):
        rec["kind"] = "data"
    elif HDR_CITE_PTR.match(hdr):
        rec["kind"] = "cite"
    elif m:
        rec["kind"] = "cite"
        rec["addr"] = m.group(1).lower()
        rec["cite_of"] = m.group(2).lower()
    elif md:
        rec["kind"] = "func"
        rec["addr"] = md.group(1).lower()
        rec["source"] = md.group(2)
        rec["entry"] = md.group(3).lower()
    else:
        m = HDR_FUNC.match(hdr)
        if m:
            rec["kind"] = "func"
            rec["addr"] = m.group(2).lower()
            rec["entry"] = (m.group(3) or m.group(2)).lower()
            rec["source"] = (m.group(4) or "").strip()

    section = None
    for ln in lines[1:]:
        sm = SECTION_RE.match(ln)
        if sm:
            section = sm.group(1).upper()
            continue
        szm = SIZE_RE.match(ln)
        if szm and rec["size"] is None:
            rec["size"] = int(szm.group(1))
            rec["ninsn"] = int(szm.group(2))
            continue
        if section and section.startswith("DISASSEMBL"):
            im = INSN_RE.match(ln)
            if im:
                rec["insns"].append((im.group(1), im.group(3), im.group(4).strip()))
        elif section and section.startswith("DECOMP"):
            rec["decomp"].append(ln)
    # Header-less dumps: the file opens straight into decompiled C. Treat the
    # filename address as the entry - there is no other identity on offer.
    if rec["kind"] == "other" and (rec["insns"] or len(rec["decomp"]) > 5):
        rec["kind"] = "func"
    # A dump's own overlay identity: prefer the bracketed source, else the
    # filename prefix (`overlay_<label>_<addr>` / bare `<addr>` = SCUS).
    src = rec["source"]
    if not src:
        stem = rec["stem"]
        src = stem[:-8].rstrip("_") if len(stem) > 8 else "SCUS"
    # Normalise the image name: the same overlay is spelt several ways across
    # dump generations (bracketed `[overlay_x.bin]`, `[overlay_x base=0x...]`,
    # filename prefix `overlay_x_<addr>`). Containment is only sound when two
    # dumps of one image agree on a name.
    src = re.sub(r"\s+base=0x[0-9a-fA-F]+", "", src or "SCUS").strip()
    src = re.sub(r"\.bin$", "", re.sub(r"^(overlay_)+", "", src))
    rec["image"] = "SCUS" if src.upper().startswith("SCUS") else src
    return rec


C_DECL_RE = re.compile(r"\bFUN_[0-9a-fA-F]{8}\b|\bfunc_0x[0-9a-fA-F]+\b")
C_HEX_RE = re.compile(r"0x[0-9a-fA-F]+")


def norm_c(decomp):
    """Normalised decompiled-C body, for dumps that carry no disassembly.

    Function names and hex literals are masked so a relocated copy of the same
    routine normalises to the same text. Weaker evidence than the instruction
    stream - only used when the dump has no `--- DISASSEMBLY ---` section.
    """
    body = []
    for ln in decomp:
        ln = ln.strip()
        if not ln or ln.startswith(("/*", "*", "//")):
            continue
        ln = C_DECL_RE.sub("@F", ln)
        ln = C_HEX_RE.sub("@H", ln)
        body.append(ln)
    return "\n".join(body)


def norm_stream(insns, mode="strict"):
    """Normalised instruction stream used for duplicate / alias comparison."""
    sub = ABS_ADDR_RE if mode == "strict" else ANY_IMM_RE
    return "\n".join(mn + " " + sub.sub("@", ops) for _, mn, ops in insns)


def has_own_return(insns):
    """True if the body returns on its own - `jr ra` anywhere in the stream."""
    for _, mn, ops in insns:
        if mn == "jr" and ops.replace("$", "").strip() in ("ra", "ra,"):
            return True
    return False


def exit_jump_target(insns):
    """Final unconditional jump target if the body ends by jumping away.

    The shared-epilogue / label-call idiom: a fragment Ghidra promoted to a
    `FUN_` entry ends `j <addr>` into code it does not own instead of `jr ra`.
    """
    for _, mn, ops in reversed(insns[-3:]):
        if mn in ("j", "b") and ops.startswith("0x"):
            return ops
    return None


def substantial(rec):
    """A dump body carrying real evidence: an instruction stream past the stub
    threshold, or - for decompile-only dumps - a non-trivial C body."""
    if rec["kind"] != "func":
        return False
    if len(rec["insns"]) > STUB_INSNS:
        return True
    return not rec["insns"] and len(norm_c(rec["decomp"]).splitlines()) > 5


def body_key(rec):
    """The comparison key for alias / duplicate grouping."""
    if rec["insns"]:
        return "I:" + norm_stream(rec["insns"])
    return "C:" + norm_c(rec["decomp"])


def is_vendor(rec):
    """Conservative PsyQ / BIOS detection.

    Deliberately narrow: a false vendor call silently drops real game logic off
    the worklist, which is the expensive direction of error.
    """
    for _, mn, ops in rec["insns"][:6]:
        if BIOS_VEC_RE.search(mn + " " + ops):
            for _, mn2, ops2 in rec["insns"][:8]:
                if mn2 == "jr" and ops2.startswith("t"):
                    return "BIOS vector thunk (li tN,0x[ABC]0 + jr tN)"
    txt = "\n".join(rec["decomp"]).lower()
    for hint in VENDOR_HINTS:
        if hint in txt:
            return "decompiled body names %s infrastructure" % hint
    return None


def enclosing(addr, image, owners):
    """Entry of another dumped body in `image` that lists an instruction at `addr`.

    Exact, not interval-based: the enclosing dump literally disassembles this
    VA as one of its own instructions. Interval containment is unsound because a
    dump's listing is not guaranteed to be address-ordered.
    """
    owner = owners.get((image, addr))
    return owner if owner and owner != addr else None


def prefix_peer(rec, dup_groups, addr):
    """Entry whose stream this truncated body is a strict prefix of, if any."""
    if not rec["insns"] or len(rec["insns"]) < MIN_DUP_INSNS:
        return None
    s = "I:" + norm_stream(rec["insns"])
    for key, entries in dup_groups.items():
        if len(key) > len(s) and key.startswith(s):
            peers = sorted(entries - {addr})
            if peers:
                return peers[0]
    return None


def dup_evidence(rec):
    if rec["insns"]:
        return "%d-instruction stream" % len(rec["insns"])
    return "%d-line decompiled body" % len(norm_c(rec["decomp"]).splitlines())


NON_PORTABLE = (
    "INTERIOR",
    "PHANTOM",
    "SHARED_TAIL",
    "DATA",
    "DUPLICATE",
    "REAL_BUT_VENDOR",
)


def classify(addr, dumps, dup_groups, ported, owners=None):
    """Return (class, reason) for one worklist address."""
    owners = owners or {}
    if not dumps:
        return ("UNCERTAIN", "no dump file matches this address")

    funcs = [d for d in dumps if d["kind"] == "func"]
    self_funcs = [d for d in funcs if d["entry"] == addr]
    other_funcs = [d for d in funcs if d["entry"] != addr]
    datas = [d for d in dumps if d["kind"] == "data"]
    cites = [d for d in dumps if d["kind"] == "cite"]

    # Only dumps with a real body carry evidence. Degenerate stubs (a VA that is
    # a function in one image and a 2-instruction jump pad in another) are noise.
    self_sub = [d for d in self_funcs if substantial(d)]

    # 1. Explicit citation stub: the dump itself states the address is interior.
    if cites and not self_sub:
        tgt = next((c["cite_of"] for c in cites if c["cite_of"]), None)
        return (
            "INTERIOR",
            "dump is a citation stub" + (" for %s" % tgt if tgt else ""),
        )

    # 2. Data region / hex blob and nothing else.
    if datas and not self_sub:
        return ("DATA", "the dump covering this VA is a data-region listing")

    # 3. No self-entry body -> the VA sits inside another function.
    if not self_sub:
        if other_funcs:
            ent = sorted({d["entry"] for d in other_funcs})
            return ("INTERIOR", "dump resolves entry=%s, not %s" % ("/".join(ent), addr))
        if self_funcs:
            rec = self_funcs[0]
            if rec["ninsn"] == 0 and len(rec["decomp"]) > 20:
                return (
                    "UNCERTAIN",
                    "Ghidra reports size=%s with no disassembly but a decompiled "
                    "body - function bounds not established" % rec["size"],
                )
            return (
                "PHANTOM",
                "only dump body is a %d-instruction stub" % len(rec["insns"]),
            )
        return ("UNCERTAIN", "dump present but header is not code, data or a citation")

    # 3b. Containment: some other dumped function in the same image owns a body
    #     that strictly covers this VA. That is the Ghidra label-call idiom -
    #     the address is a jump label promoted to a fake FUN_ entry.
    for d in self_sub:
        enc = enclosing(addr, d["image"], owners)
        if enc:
            return (
                "INTERIOR",
                "VA lies inside the dumped body of %s in %s" % (enc, d["image"]),
            )

    # 4. VA aliasing across overlays: distinct substantial bodies at one VA.
    #    Compare like with like - some dump generations carry decompiled C only,
    #    and an instruction stream never equals a C body, so mixing the two
    #    would report every such pair as aliased.
    #    The instruction stream is the authority; decompiled C is a rendering
    #    whose variable naming and inferred signature drift with analysis state,
    #    so C bodies only decide when no dump at this VA has disassembly.
    with_insns = [d for d in self_sub if d["insns"]]
    if with_insns:
        streams = {body_key(d): d for d in with_insns}
        self_sub = with_insns
    else:
        streams = {norm_c(d["decomp"]): d for d in self_sub}
    if len(streams) > 1:
        return (
            "VA_ALIASED",
            "%d overlays dump distinct bodies at this VA (%s)"
            % (len(streams), ", ".join(sorted({d["image"] for d in streams.values()}))),
        )
    if any(substantial(d) for d in other_funcs):
        ent = sorted({d["entry"] for d in other_funcs if substantial(d)})
        return (
            "VA_ALIASED",
            "self-entry body in %s, but entry=%s in another image"
            % (self_sub[0]["image"], "/".join(ent)),
        )

    rec = self_sub[0]

    # 5. Phantom: Ghidra switch-case stub.
    if CASE_LABEL_RE.search("\n".join(rec["decomp"])) and len(rec["insns"]) < 12:
        return (
            "PHANTOM",
            "Ghidra caseD_ switch-case stub, %d instructions" % len(rec["insns"]),
        )

    # 6. No return of its own: the body exits by jumping into code it does not
    #    own. Distinct body, but not an independently callable function.
    if rec["insns"] and not has_own_return(rec["insns"]):
        tgt = exit_jump_target(rec["insns"])
        if tgt:
            own = owners.get((rec["image"], tgt[2:].lower().zfill(8)))
            return (
                "SHARED_TAIL",
                "no `jr ra`; body exits `j %s`%s - an entry into a multi-entry "
                "routine, not a standalone function"
                % (tgt, ", inside %s" % own if own else ""),
            )
        # A body with neither a return nor an exit jump is a truncated dump
        # (Ghidra's `halt_baddata()`). If it is a strict prefix of another
        # entry's stream, it is that routine, dumped short.
        pfx = prefix_peer(rec, dup_groups, addr)
        if pfx:
            return (
                "DUPLICATE",
                "truncated %d-instruction dump; stream is a strict prefix of %s"
                % (len(rec["insns"]), pfx),
            )
        return (
            "UNCERTAIN",
            "no `jr ra` and no trailing unconditional jump - body may be truncated",
        )

    # 7. Duplicate of another worklist entry or of already-ported work.
    if not rec["insns"] or len(rec["insns"]) >= MIN_DUP_INSNS:
        peers = sorted(dup_groups.get(body_key(rec), set()) - {addr})
        if peers:
            ported_peers = [p for p in peers if p in ported]
            if ported_peers:
                return (
                    "DUPLICATE",
                    "%s identical modulo relocation to the already-ported %s"
                    % (dup_evidence(rec), ported_peers[0]),
                )
            return (
                "DUPLICATE",
                "%s identical modulo relocation to %s" % (dup_evidence(rec), peers[0]),
            )

    # 8. Vendor infrastructure.
    v = is_vendor(rec)
    if v:
        return ("REAL_BUT_VENDOR", v)

    return ("REAL", "self-entry body of %d instructions returning `jr ra`" % len(rec["insns"]))


IGNORE_CATEGORY = {
    "INTERIOR": "worklist_interior",
    "PHANTOM": "worklist_phantom",
    "SHARED_TAIL": "worklist_shared_tail",
    "DATA": "worklist_data",
    "DUPLICATE": "worklist_duplicate",
    "REAL_BUT_VENDOR": "worklist_vendor",
}


def write_ignore(path, results):
    outp = os.path.abspath(path)
    os.makedirs(os.path.dirname(outp) or ".", exist_ok=True)
    buckets = defaultdict(list)
    for addr, cls, reason in sorted(results):
        if cls in IGNORE_CATEGORY:
            buckets[IGNORE_CATEGORY[cls]].append((addr, cls, reason))
    with open(outp, "w") as fh:
        fh.write(
            "# Proposed additions to `scripts/ci/port-catalog-ignore.toml`.\n"
            "#\n"
            "# Generated by `scripts/ghidra-analysis/classify-worklist.py`. Every row is\n"
            "# a worklist address that is not a distinct portable function entry. The\n"
            "# reason restates the mechanical evidence read out of the Ghidra dump.\n"
            "# `docs/tooling/worklist-classification.md` documents what each class means\n"
            "# and which false positives a reviewer should spot-check.\n"
            "#\n"
            "# VA_ALIASED and UNCERTAIN rows are deliberately absent: an aliased VA is\n"
            "# more work than one port site, not less, and UNCERTAIN needs a human.\n"
            "#\n"
            "# Merge into the main ignore list after review - do not consume directly.\n"
        )
        for cat in sorted(buckets):
            fh.write("\n[%s]\n" % cat)
            for addr, cls, reason in buckets[cat]:
                fh.write('"%s" = "%s: %s"\n' % (addr, cls, reason.replace('"', "'")))


def main():
    ap = argparse.ArgumentParser(description="classify the port-catalog worklist")
    ap.add_argument("--repo", required=True, help="checkout with ghidra/scripts/funcs/")
    ap.add_argument("--catalog", required=True, help="port-catalog catalog.csv")
    ap.add_argument("--out", required=True, help="classification CSV to write")
    ap.add_argument("--ignore-out", help="proposed ignore-list TOML to write")
    ap.add_argument("--explain", help="print full evidence for one address")
    args = ap.parse_args()

    funcs_dir = os.path.join(args.repo, "ghidra", "scripts", "funcs")
    if not os.path.isdir(funcs_dir):
        sys.stderr.write("no dumps under %s\n" % funcs_dir)
        return 2

    by_addr = defaultdict(list)
    all_dumps = []
    for name in sorted(os.listdir(funcs_dir)):
        if not name.endswith(".txt"):
            continue
        m = re.search(r"([0-9a-fA-F]{8})\.txt$", name)
        if not m:
            continue
        rec = parse_dump(os.path.join(funcs_dir, name))
        if rec is None:
            continue
        rec["file_addr"] = m.group(1).lower()
        if rec["kind"] == "func" and not rec["entry"]:
            # Header-less dump: the filename address is the only identity given.
            rec["entry"] = rec["file_addr"]
        by_addr[rec["file_addr"]].append(rec)
        all_dumps.append(rec)

    with open(args.catalog) as fh:
        rows = list(csv.DictReader(fh))
    worklist = [
        r["addr"].lower()
        for r in rows
        if r["dumped"] == "1"
        and r["documented"] == "1"
        and r["ported"] == "0"
        and r["ignored"] == "0"
    ]
    ported = {r["addr"].lower() for r in rows if r["ported"] == "1"}

    # Duplicate index over every self-entry function body in the dump corpus,
    # keyed by relocation-masked body -> set of entry addresses.
    dup_groups = defaultdict(set)
    # (image, addr) -> entry of the dumped body that lists an instruction at
    # that VA. Backs both the containment test and jump-target attribution.
    owners = {}
    for rec in all_dumps:
        if rec["entry"] != rec["file_addr"] or not substantial(rec):
            continue
        if rec["insns"] and len(rec["insns"]) < MIN_DUP_INSNS:
            continue
        dup_groups[body_key(rec)].add(rec["entry"])
        for a, _, _ in rec["insns"]:
            owners.setdefault((rec["image"], a), rec["entry"])

    if args.explain:
        a = args.explain.lower()
        for d in by_addr.get(a, []):
            sys.stdout.write(
                "%s kind=%s entry=%s image=%s size=%s ninsn=%d jr_ra=%s exit=%s\n"
                % (
                    d["stem"],
                    d["kind"],
                    d["entry"],
                    d["image"],
                    d["size"],
                    len(d["insns"]),
                    has_own_return(d["insns"]),
                    exit_jump_target(d["insns"]),
                )
            )
        sys.stdout.write(
            "=> %s\n"
            % (classify(a, by_addr.get(a, []), dup_groups, ported, owners),)
        )
        return 0

    results = [
        (a,) + classify(a, by_addr.get(a, []), dup_groups, ported, owners)
        for a in worklist
    ]

    outp = os.path.abspath(args.out)
    os.makedirs(os.path.dirname(outp) or ".", exist_ok=True)
    with open(outp, "w", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(["addr", "class", "reason"])
        for row in sorted(results):
            w.writerow(row)

    hist = Counter(c for _, c, _ in results)
    total = len(results)
    sys.stdout.write("worklist rows: %d\n" % total)
    for cls, n in hist.most_common():
        sys.stdout.write("  %-16s %4d  (%4.1f%%)\n" % (cls, n, 100.0 * n / total))
    sys.stdout.write(
        "\nsingle-site port work, lower bound (REAL)                  : %d\n"
        "single-site port work, upper bound (REAL + UNCERTAIN)      : %d\n"
        "plus VA_ALIASED rows, each covering >1 distinct body       : %d\n"
        "plus SHARED_TAIL rows, entries into multi-entry routines   : %d\n"
        % (
            hist.get("REAL", 0),
            hist.get("REAL", 0) + hist.get("UNCERTAIN", 0),
            hist.get("VA_ALIASED", 0),
            hist.get("SHARED_TAIL", 0),
        )
    )

    if args.ignore_out:
        write_ignore(args.ignore_out, results)
    return 0


if __name__ == "__main__":
    sys.exit(main())
