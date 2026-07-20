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
# A citation stub's body names the function it sits inside:
#   "Mid-function citation. Enclosing function dumped as <file>_<addr>.txt"
CITE_HOST_RE = re.compile(
    r"Enclosing function dumped as\s+\S*?([0-9a-fA-F]{8})\.txt", re.IGNORECASE
)
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

# Interior-fragment liveness signature. Callee-saved registers the body reads
# without ever setting them; `unaff_gp` is deliberately excluded - gp-relative
# addressing makes it normal in this codebase and it carries no signal.
UNAFF_SAVED_RE = re.compile(r"\bunaff_(s[0-8]|fp|retaddr|ra)\b")
# A read through the caller's frame: a stack slot this body never wrote.
IN_STACK_RE = re.compile(r"\bin_stack_[0-9a-fA-F]+\b")
# `addiu sp,sp,-N` - the body allocating a frame of its own.
FRAME_ALLOC_RE = re.compile(r"^sp,sp,-")
# How far into the body the frame allocation may be scheduled.
PROLOGUE_WINDOW = 8

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

# Signatures of a dump taken at a VA the image holds no code for - data decoded
# as instructions. See `decode_failure`.
MEM_MNEMONICS = frozenset(
    ("lb", "lbu", "lh", "lhu", "lw", "lwl", "lwr", "sb", "sh", "sw", "swl", "swr")
)
ZERO_ABS_RE = re.compile(r",-?0x[0-9a-fA-F]+\(zero\)\s*$")
BAD_INSN_RE = re.compile(r"bad instruction data")


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
        if rec["kind"] == "cite" and not rec["cite_of"]:
            em = CITE_HOST_RE.search(ln)
            if em:
                rec["cite_of"] = em.group(1).lower()
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


def establishes_frame(insns):
    """True if the body opens by building its own stack frame.

    A genuine non-leaf entry starts with `addiu sp,sp,-N` within the first few
    instructions - the compiler may schedule one or two independent loads ahead
    of it, but not more. A tail fragment that Ghidra promoted to a `FUN_` entry
    never adjusts `sp`: the frame belongs to the routine it was cut out of.
    """
    for _, mn, ops in insns[:PROLOGUE_WINDOW]:
        if mn in ("addiu", "addi") and FRAME_ALLOC_RE.match(ops.replace(" ", "")):
            return True
    return False


def fragment_host(rec):
    """Interior-fragment evidence, or None.

    `jr ra` in the stream is not proof of a self-contained function: a *tail*
    fragment ends in the parent's epilogue, and an epilogue restores `ra` and
    returns. What separates the two is register and stack liveness. A fragment
    reads callee-saved registers it never sets - Ghidra renders those
    `unaff_s0..s8` / `unaff_fp` / `unaff_retaddr` - and reads the parent's frame
    through slots it never wrote, rendered `in_stack_<offset>`.

    All three conditions are required together. `unaff_gp` alone is normal here
    (gp-relative addressing), an `in_stack_` read alone is an ordinary
    stack-passed argument, and a large real function can pick up a stray
    `unaff_` from an incomplete decompile - so the frame test decides last.
    """
    if not rec["insns"] or establishes_frame(rec["insns"]):
        return None
    body = "\n".join(rec["decomp"])
    saved = sorted(set(UNAFF_SAVED_RE.findall(body)))
    if not saved or not IN_STACK_RE.search(body):
        return None
    return saved


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


def decode_failure(rec):
    """True when this dump's body is a data region decoded as instructions.

    A dump's printed addresses are a property of the load base it was taken at
    (`docs/tooling/dump-corpus-integrity.md`). Point a dump at a VA the image
    does not hold code for and Ghidra still emits a disassembly - of whatever
    bytes are there. Two signatures identify the result mechanically:

    * a body dominated by loads/stores off `$zero`. Real MIPS reaches statics
      through `gp` or a `lui`/`addiu` pair; a long run of `lb rN,0xNNNN(zero)`
      is a table of `0x80`-high bytes being read as opcodes.
    * Ghidra's own bad-instruction warning over a body too short to be a
      function.

    Such a body is not evidence of a distinct routine, so it must not enter the
    alias or duplicate comparison - otherwise one bad-base dump splits a VA that
    every other image agrees on.
    """
    n = len(rec["insns"])
    if not n:
        return None
    zero = sum(
        1
        for _, mn, ops in rec["insns"]
        if mn.lstrip("_") in MEM_MNEMONICS and ZERO_ABS_RE.search(ops)
    )
    if zero >= 8 and zero * 5 >= n * 2:
        return "%d of %d instructions are $zero-absolute loads/stores" % (zero, n)
    if n <= STUB_INSNS + 4 and any(BAD_INSN_RE.search(ln) for ln in rec["decomp"]):
        return "Ghidra reports bad instruction data over a %d-instruction body" % n
    return None


def stream_gaps(rec):
    """Count holes in this dump's instruction-address sequence.

    MIPS instructions are four bytes and a dumped body is contiguous, so a
    jump in the address column means Ghidra failed to disassemble part of the
    range and the dump under-reports the routine.
    """
    addrs = [int(a, 16) for a, _, _ in rec["insns"]]
    return sum(1 for i in range(len(addrs) - 1) if addrs[i + 1] != addrs[i] + 4)


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


class StaticArbiter:
    """Resolve dumps against the statically extracted overlay images.

    The classifier's other tests read dump metadata. This one reads the bytes,
    which is the only evidence that survives a wrong load base. It answers two
    questions per dump: does it testify about the queried VA at all, and if so
    which overlay's code is it.

    Needs `extracted/` populated. That directory is gitignored, so the
    arbiter is optional and the classifier degrades to its metadata-only
    behaviour without it.
    """

    WINDOW = 24  # instructions compared; long enough that a match is not chance

    def __init__(self, repo):
        import importlib.util

        import capstone

        helper = os.path.join(
            os.path.dirname(os.path.abspath(__file__)),
            "check-dump-base-integrity.py",
        )
        spec = importlib.util.spec_from_file_location("_cdbi", helper)
        self._cdbi = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self._cdbi)
        self._md = capstone.Cs(
            capstone.CS_ARCH_MIPS,
            capstone.CS_MODE_MIPS32 + capstone.CS_MODE_LITTLE_ENDIAN,
        )
        self.images = []
        try:
            import tomllib
        except ImportError:
            import tomli as tomllib
        mp = os.path.join(repo, "crates", "asset", "data", "static-overlays.toml")
        ovl = os.path.join(repo, "extracted", "overlays")
        with open(mp, "rb") as fh:
            spec_map = tomllib.load(fh)
        for o in spec_map["overlays"]:
            p = os.path.join(ovl, "overlay_%s_%04d.bin" % (o["label"], o["prot_index"]))
            if os.path.exists(p):
                with open(p, "rb") as fh:
                    self.images.append(
                        ("%s(%d)" % (o["label"], o["prot_index"]), o["base_va"], 0, fh.read())
                    )
        self._trim_over_read()
        scus = os.path.join(repo, "extracted", "SCUS_942.54")
        if os.path.exists(scus):
            with open(scus, "rb") as fh:
                self.images.append(("SCUS", 0x80010000, 0x800, fh.read()))

    # A PROT entry's head, long enough that finding it inside another entry is
    # the over-read and not a coincidence.
    OVER_READ_PROBE = 256

    def _trim_over_read(self):
        """Cut each image down to the bytes that entry actually owns.

        An extracted image is the entry's `read_entry` FOOTPRINT, which runs
        past the entry into its neighbours' sectors; the runtime slice is only
        `[entry start, next entry start)`. Left whole, an image answers for
        VAs its overlay never occupies, with its neighbour's code - so the
        arbiter would attribute a routine to the wrong overlay at an address
        that overlay never loads. The field overlay is the case that bites:
        PROT 0898's image is byte-identical to PROT 0897's from file
        `0x25000`, so the battle dispatcher `FUN_801D0748` also appears at a
        phantom `0x801F5748` inside 0897's tail.

        The cut is where some other image's head appears, which is that
        neighbour's start; sector alignment keeps a chance byte-run from
        truncating a real overlay.
        """
        heads = [
            (label, data[: self.OVER_READ_PROBE])
            for label, _, _, data in self.images
            if len(data) >= self.OVER_READ_PROBE
        ]
        trimmed = []
        for label, base, hdr, data in self.images:
            cut = len(data)
            for other, head in heads:
                if other == label:
                    continue
                at = data.find(head)
                if 0 < at < cut and at % 0x800 == 0:
                    cut = at
            trimmed.append((label, base, hdr, data[:cut]))
        self.images = trimmed

    def available(self):
        return bool(self.images)

    def _img_tokens(self, data, off, n):
        out = []
        for k in range(n):
            w = data[off + 4 * k : off + 4 * k + 4]
            if len(w) < 4:
                return None
            tok = None
            # Decode each word independently: a streaming disassembly stops at
            # the first word capstone rejects, silently truncating the compare.
            for ins in self._md.disasm(w, 0x80000000):
                tok = self._cdbi.canon(ins.mnemonic, ins.op_str)
            out.append(tok or "BAD||")
        return out

    def _dump_tokens(self, d, n):
        return [self._cdbi.canon(mn, ops) for _, mn, ops in d["insns"][:n]]

    @staticmethod
    def _looks_like_data(insns):
        """The `$zero`-absolute signature: a table being decoded as opcodes.

        Real MIPS reaches statics through `gp` or a `lui`/`addiu` pair, so a
        run of `lb rN,0xNNNN(zero)` is a table of `0x80`-high bytes read as
        instructions. An image window that matches a dump here is matching
        that image's DATA, and two images agreeing on nothing but data must
        not be reported as holding distinct code.
        """
        if not insns:
            return False
        zero_abs = sum(
            1
            for mn, ops in insns
            if mn in MEM_MNEMONICS and ZERO_ABS_RE.search("," + ops)
        )
        return zero_abs * 2 >= len(insns)

    def owner_at(self, addr_int, d):
        """Images whose bytes equal this dump's opening instructions at this VA."""
        toks = self._dump_tokens(d, self.WINDOW)
        if len(toks) < 8:
            return None
        if self._looks_like_data([(mn, ops) for _, mn, ops in d["insns"][: self.WINDOW]]):
            return None
        hits = []
        for label, base, hdr, data in self.images:
            off = hdr + (addr_int - base)
            if addr_int < base or off < 0 or off + 4 * len(toks) > len(data):
                continue
            if self._img_tokens(data, off, len(toks)) == toks:
                hits.append(label)
        return hits

    def testifying(self, addr, funcs):
        """Dumps that both start at this VA and match some image there."""
        va = int(addr, 16)
        out = []
        for d in funcs:
            if d["entry"] != addr:
                continue
            if self.owner_at(va, d):
                out.append(d)
        return out

    def arbitrate(self, addr, funcs):
        """(class, reason) when the bytes settle the row, else None."""
        va = int(addr, 16)
        owners = {}
        for d in funcs:
            if d["entry"] != addr:
                continue
            for label in self.owner_at(va, d) or []:
                owners.setdefault(label, []).append(d["image"])
        if len(owners) == 1:
            label, seen = next(iter(owners.items()))
            # Deliberately narrow: this says every DUMP here is of `label`, not
            # that no other co-based overlay holds code at this VA. Nothing has
            # dumped those, so they are not worklist rows.
            return (
                "REAL",
                "every dump at this VA is of %s (captures: %s); no second "
                "dumped body here" % (label, "/".join(sorted(set(seen)))),
            )
        if len(owners) > 1:
            return (
                "VA_ALIASED",
                "%d overlays hold distinct code at this VA (%s) - confirmed "
                "against the extracted images, not the dump image tags"
                % (len(owners), ", ".join(sorted(owners))),
            )
        return None

    def peer_is_real(self, peer_addr, by_addr, key):
        """Does the MATCHING body at `peer_addr` actually live at that VA?

        Testing "some routine exists at the peer VA" is too weak: a VA can
        host a real function in one overlay while the stream that matched came
        from a mis-based dump printed at the same address. Only a dump whose
        body_key equals this one's, and which resolves at the peer VA, makes
        the peer a stable name for the routine.
        """
        va = int(peer_addr, 16)
        for d in by_addr.get(peer_addr, []):
            if d["kind"] != "func" or d["entry"] != peer_addr:
                continue
            if body_key(d) != key:
                continue
            if self.owner_at(va, d):
                return True
        return False

    UNSUPPORTED_ALIAS = (
        "UNCERTAIN",
        "no extracted image holds the dumped bytes at this VA - the owning "
        "overlay is either un-extracted or the dumps are mis-based; alias "
        "cannot be confirmed or refuted",
    )

    # Signature length for the relocation index. Shorter than WINDOW so the
    # index stays affordable; every candidate is then re-checked over the full
    # WINDOW before it is reported.
    RELOC_SIG = 10

    def _reloc_index(self):
        """Lazy {token-signature: [(label, base, file_offset)]} over every image.

        Built only when a row has reached UNSUPPORTED_ALIAS, which is a handful
        of rows per run; it costs a full canonical disassembly of every image.
        """
        if getattr(self, "_reloc", None) is None:
            idx = defaultdict(list)
            for label, base, hdr, data in self.images:
                toks = self._cdbi.canon_bytes(data, len(data) // 4)
                for i in range(len(toks) - self.RELOC_SIG):
                    key = "\x00".join(toks[i : i + self.RELOC_SIG])
                    idx[key].append((label, base, hdr, i * 4))
            self._reloc = idx
        return self._reloc

    def relocate(self, addr, funcs):
        """Where a mis-based dump's bytes actually live.

        `owner_at` asks whether a dump's bytes are at the VA it prints. This
        asks the complementary question the dump-base-integrity sweep asks:
        are they anywhere at all? A single consistent answer means the printed
        VA is fiction rather than the overlay being un-extracted, which is a
        different piece of work - and one no extraction will ever close.

        Returns the sorted `[(label, real_va)]` the bytes resolve to, when
        every self-entry dump here resolves somewhere; otherwise None. A
        routine that is linked into several overlays resolves several times -
        that is still a resolution, and it still means the printed VA is not
        one of them.
        """
        idx = self._reloc_index()
        by_label = {label: data for label, _, _, data in self.images}
        found = set()
        for d in funcs:
            if d["entry"] != addr:
                continue
            toks = self._dump_tokens(d, self.WINDOW)
            if len(toks) < self.RELOC_SIG:
                return None
            hits = idx.get("\x00".join(toks[: self.RELOC_SIG]), [])
            resolved = set()
            for label, base, hdr, off in hits:
                if base is None:
                    continue
                # Re-check over the full window: a 10-token signature can
                # collide, and a prefix match is not a resolution.
                if self._img_tokens(by_label[label], off, len(toks)) != toks:
                    continue
                resolved.add((label, base + off - hdr))
            if not resolved:
                return None
            found |= resolved
        return sorted(found, key=lambda t: (t[1], t[0])) or None

    def batch_delta(self, image, by_addr):
        """The modal base error of the Ghidra program `image`, if it has one.

        A wrong load base is a property of one import, so every dump taken
        from that program inherits the same delta. Establishing it from the
        program's *other* dumps gives a candidate offset to re-check a dump
        too short to sign on its own - which is a byte test seeded by a
        measurement, not the image-tag inference this class exists to avoid.
        """
        cache = getattr(self, "_batch", None)
        if cache is None:
            cache = self._batch = {}
        if image in cache:
            return cache[image]
        seen = Counter()
        for a, dumps in (by_addr or {}).items():
            for d in dumps:
                if d["kind"] != "func" or d["entry"] != a or d["image"] != image:
                    continue
                if len(self._dump_tokens(d, self.WINDOW)) < self.WINDOW:
                    continue
                if self.owner_at(int(a, 16), d):
                    continue  # correctly based; contributes no delta
                hits = self.relocate(a, [d]) or []
                if len(hits) == 1:
                    seen[hits[0][1] - int(a, 16)] += 1
        # One delta shared by an outright majority of the program's resolved
        # dumps is a mis-based batch; a split vote is not evidence of
        # anything. A minority delta is expected even in a clean batch, since
        # a dump landing in an entry's over-read tail resolves against the
        # neighbour it was read from. The proposal is a candidate either way -
        # `relocate_short` still has to match the bytes at it.
        got = None
        if seen:
            delta, votes = seen.most_common(1)[0]
            if votes >= 3 and votes * 2 > sum(seen.values()):
                got = delta
        cache[image] = got
        return got

    def relocate_short(self, addr, funcs, by_addr):
        """Resolve a body too short to sign, via its program's batch delta."""
        va = int(addr, 16)
        by_label = {label: data for label, _, _, data in self.images}
        out = set()
        for d in funcs:
            if d["entry"] != addr:
                continue
            toks = self._dump_tokens(d, self.WINDOW)
            if not toks:
                return None
            delta = self.batch_delta(d["image"], by_addr)
            if delta is None:
                return None
            hit = None
            for label, base, hdr, data in self.images:
                off = hdr + (va + delta - base)
                if off < 0 or off + 4 * len(toks) > len(data):
                    continue
                if self._img_tokens(by_label[label], off, len(toks)) == toks:
                    if hit is not None:
                        return None
                    hit = (label, va + delta)
            if hit is None:
                return None
            out.add(hit)
        return sorted(out) or None

    def unsupported_alias(self, addr, funcs, by_addr=None):
        """UNSUPPORTED_ALIAS, sharpened when the bytes resolve elsewhere."""
        try:
            hits = self.relocate(addr, funcs) or self.relocate_short(
                addr, funcs, by_addr
            )
        except Exception:  # index build failure must not decide a verdict
            hits = None
        if not hits:
            # A gapped stream cannot match any image as a contiguous window,
            # so failing to resolve says nothing about the overlay corpus.
            gapped = [
                d
                for d in funcs
                if d["entry"] == addr and d["insns"] and stream_gaps(d)
            ]
            if gapped and len(gapped) == len([d for d in funcs if d["entry"] == addr]):
                return (
                    "UNCERTAIN",
                    "every dump at this VA has a gapped instruction stream "
                    "(%d hole(s) in %s), so it matches no image as a "
                    "contiguous window - re-dump before classifying"
                    % (stream_gaps(gapped[0]), gapped[0]["image"]),
                )
            return self.UNSUPPORTED_ALIAS
        va = int(addr, 16)
        where = ", ".join("%s 0x%08x" % (lbl, v) for lbl, v in hits[:3])
        if len(hits) > 1:
            where += " (the same routine linked into each)"
        else:
            where += " (delta %+#x)" % (hits[0][1] - va)
        return (
            "UNCERTAIN",
            "every dump at this VA is mis-based: the bytes live at %s, and at "
            "this VA in no image - so nothing attests the address. Not an "
            "un-extracted overlay; no extraction closes it" % where,
        )


def classify(addr, dumps, dup_groups, ported, owners=None, arb=None,
             all_dumps_by_addr=None):
    """Return (class, reason) for one worklist address."""
    owners = owners or {}
    if not dumps:
        return ("UNCERTAIN", "no dump file matches this address")

    funcs = [d for d in dumps if d["kind"] == "func"]

    # 0. Static-image arbitration. A dump's `[overlay_foo.bin]` tag is a Ghidra
    #    PROGRAM name, and most programs in this corpus are live-RAM captures
    #    named after a game scenario rather than an overlay. A capture spans
    #    slot A + slot B + the resident executable, so the tag says which
    #    capture the bytes came from, never which overlay owns the VA. Two
    #    differently-tagged dumps routinely hold one overlay's code, which the
    #    image-name test reads as an alias.
    #    Where the extracted images are available, ask the bytes instead: a
    #    dump testifies about this VA only if its opening instructions match
    #    some image AT this VA. A dump that matches at a different VA is
    #    mis-based and its printed address is fiction.
    if arb is not None and funcs:
        cls, why = classify(addr, dumps, dup_groups, ported, owners, None, all_dumps_by_addr)
        if cls == "DUPLICATE":
            # A relocated-stream match names a peer ADDRESS. If no dump at that
            # peer VA actually holds those bytes there, the peer is a printed
            # address from a mis-based dump and identifies no routine, so the
            # duplicate claim is withdrawn rather than acted on.
            peer = re.search(r"\b([0-9a-f]{8})\b", why)
            mine = next((d for d in funcs if d["entry"] == addr and substantial(d)), None)
            if (
                peer
                and mine is not None
                and not arb.peer_is_real(
                    peer.group(1), all_dumps_by_addr or {}, body_key(mine)
                )
            ):
                return (
                    "REAL",
                    "duplicate claim withdrawn: peer %s is a printed address "
                    "from a mis-based dump, not a body at that VA"
                    % peer.group(1),
                )
            return (cls, why)
        if cls != "VA_ALIASED":
            # Nothing to arbitrate: the metadata tests reached a verdict that
            # does not turn on which image a dump came from.
            return (cls, why)
        verdict = arb.arbitrate(addr, funcs)
        if verdict is not None:
            return verdict
        # No dump testifies about this VA, so the alias rests on image tags
        # that are capture-program names. Downgrade to the class that is
        # never ignored rather than delete or assert the row - and say which
        # of the two causes it is, when the bytes resolve somewhere else.
        return arb.unsupported_alias(addr, funcs, all_dumps_by_addr)

    # Drop dumps whose body is a misdecoded data region before any comparison.
    # One such dump at a VA a dozen other images agree on would otherwise read
    # as an alias, which is the wrong direction of error: it turns a single
    # port site into "needs per-image identity work".
    bad = [(d, decode_failure(d)) for d in funcs]
    rejected = [(d, why) for d, why in bad if why]
    funcs = [d for d, why in bad if not why]
    if rejected and not funcs:
        d, why = rejected[0]
        return (
            "UNCERTAIN",
            "every dump at this VA decodes data as code (%s in %s) - no valid "
            "body to classify" % (why, d["image"]),
        )

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
    #     Containment is a per-image fact, so it only settles the row when it
    #     holds in every image that dumps a body here. An address that is a
    #     jump label in one overlay and a function entry in another is aliased,
    #     not interior, and ignoring it would delete the second overlay's
    #     routine - the failure this test used to produce.
    #     Containment is read per image and is independent of any one dump's
    #     completeness. What must be complete is the dump on the other side of
    #     the disagreement: only a stream with no holes testifies that the VA
    #     is a function entry rather than a jump pad Ghidra mis-bounded.
    contained = [(d, enclosing(addr, d["image"], owners)) for d in self_sub]
    hits = [(d, enc) for d, enc in contained if enc]
    free = [d for d, enc in contained if not enc and not stream_gaps(d)]
    if hits and not free:
        d, enc = hits[0]
        return (
            "INTERIOR",
            "VA lies inside the dumped body of %s in %s" % (enc, d["image"]),
        )
    if hits:
        d, enc = hits[0]
        return (
            "VA_ALIASED",
            "interior to %s in %s, but a whole self-entry body in %s"
            % (enc, d["image"], "/".join(sorted({o["image"] for o in free}))),
        )

    # 4. VA aliasing across overlays: distinct substantial bodies at one VA.
    #    Compare like with like - some dump generations carry decompiled C only,
    #    and an instruction stream never equals a C body, so mixing the two
    #    would report every such pair as aliased.
    #    The instruction stream is the authority; decompiled C is a rendering
    #    whose variable naming and inferred signature drift with analysis state,
    #    so C bodies only decide when no dump at this VA has disassembly.
    with_insns = [d for d in self_sub if d["insns"]]
    # A dump whose instruction addresses are non-contiguous did not disassemble
    # the whole body: Ghidra left holes, and every instruction after a hole is
    # offset against a complete dump of the same routine. Comparing a gapped
    # stream against a contiguous one reports a difference that is an artifact
    # of the dump, not of the code, so a contiguous dump outranks it.
    contiguous = [d for d in with_insns if not stream_gaps(d)]
    if contiguous and len(contiguous) < len(with_insns):
        with_insns = contiguous
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

    # 5b. Tail fragment: the body reaches the parent's epilogue, so it contains
    #     `jr ra` without owning the frame that epilogue tears down.
    frag = fragment_host(rec)
    if frag:
        return (
            "INTERIOR",
            "no `addiu sp,sp,-N` prologue; body reads %s and a caller stack slot "
            "it never writes - a tail fragment reaching the parent's epilogue"
            % "/".join(frag),
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

    # 9. `REAL` is the class the worklist acts on, so it is the one verdict that
    #    must not rest on a decompiled body alone. A dump reporting
    #    `size=1 bytes, 0 instructions` under a full C rendering is a catalogued
    #    Ghidra artifact - function bounds were never established, so the C is a
    #    guess about where the routine ends and there is no evidence the VA is
    #    an entry rather than a label. Every earlier test that could have caught
    #    that (own return, exit jump, fragment shape) reads the instruction
    #    stream and silently passes an empty one.
    if not rec["insns"]:
        return (
            "UNCERTAIN",
            "dump carries a decompiled body but no disassembly (Ghidra reports "
            "%s bytes) - function bounds never established" % rec["size"],
        )

    return (
        "REAL",
        "self-entry body of %d instructions returning `jr ra`" % len(rec["insns"]),
    )


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
    ap.add_argument(
        "--no-static-arbitration",
        action="store_true",
        help="classify from dump metadata alone, without resolving dump bytes "
        "against the extracted overlay images",
    )
    ap.add_argument(
        "--audit-ignored",
        action="store_true",
        help="re-classify the rows the ignore list already absorbed under a "
        "worklist_* category and report any that no longer read non-portable",
    )
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
    # Rows the ignore list already absorbed under a `worklist_*` category. The
    # classifier proposed each of them, so re-classifying them is the only way
    # a later dump - one taken at a base the earlier run did not have - can
    # overturn a verdict that has since deleted a real port site.
    absorbed = [
        r["addr"].lower()
        for r in rows
        if r["ignored"] == "1" and r["ignore_category"].startswith("worklist_")
    ]

    # Duplicate index over every self-entry function body in the dump corpus,
    # keyed by relocation-masked body -> set of entry addresses.
    dup_groups = defaultdict(set)
    # (image, addr) -> entry of the dumped body that lists an instruction at
    # that VA. Backs both the containment test and jump-target attribution.
    owners = {}
    for rec in all_dumps:
        if rec["entry"] != rec["file_addr"] or not substantial(rec):
            continue
        # A misdecoded data region owns no instructions and duplicates nothing.
        if decode_failure(rec):
            continue
        if rec["insns"] and len(rec["insns"]) < MIN_DUP_INSNS:
            continue
        dup_groups[body_key(rec)].add(rec["entry"])
        for a, _, _ in rec["insns"]:
            owners.setdefault((rec["image"], a), rec["entry"])

    # The byte-level arbiter is optional: it needs `extracted/`, which is
    # gitignored. Without it the classifier falls back to dump metadata, and
    # the image-tag caveat in worklist-classification.md applies in full.
    arb = None
    if not args.no_static_arbitration:
        try:
            cand = StaticArbiter(args.repo)
            if cand.available():
                arb = cand
        except Exception as exc:  # missing capstone, missing images, bad map
            sys.stderr.write("static arbitration unavailable: %s\n" % exc)
    if arb is None:
        sys.stderr.write(
            "note: classifying from dump metadata only - image tags are Ghidra "
            "program names, not overlay identities\n"
        )

    if args.explain:
        a = args.explain.lower()
        if arb is not None:
            va = int(a, 16)
            for d in by_addr.get(a, []):
                if d["kind"] != "func":
                    continue
                owns = arb.owner_at(va, d) if d["entry"] == a else None
                sys.stdout.write(
                    "  bytes(%s): %s\n"
                    % (
                        d["stem"],
                        ",".join(owns) if owns else
                        ("does not start at this VA (entry=%s)" % d["entry"]
                         if d["entry"] != a else "match no extracted image here"),
                    )
                )
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
            % (classify(a, by_addr.get(a, []), dup_groups, ported, owners, arb, by_addr),)
        )
        return 0

    if args.audit_ignored:
        by_cat = {r["addr"].lower(): r["ignore_category"] for r in rows}
        bad = 0
        for a in absorbed:
            cls, why = classify(a, by_addr.get(a, []), dup_groups, ported, owners, arb, by_addr)
            if cls in NON_PORTABLE:
                continue
            bad += 1
            sys.stdout.write(
                "%s ignored as %s but now classifies %s: %s\n"
                % (a, by_cat[a], cls, why)
            )
        sys.stdout.write(
            "\n%d of %d absorbed rows no longer classify non-portable\n"
            % (bad, len(absorbed))
        )
        return 1 if bad else 0

    results = [
        (a,) + classify(a, by_addr.get(a, []), dup_groups, ported, owners, arb, by_addr)
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
