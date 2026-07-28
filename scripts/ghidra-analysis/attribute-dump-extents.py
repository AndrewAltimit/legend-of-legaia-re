#!/usr/bin/env python3
"""Attribute VA-ambiguous dump extents to the extracted image that holds them.

`scripts/ci/disc-coverage.py` measures each image's code coverage by asking
which dump extents fall inside that image's mapped span. Several overlays load
at one base, so an extent can fall inside more than one span and be counted by
each - which the coverage report reports as `VA-ambiguous` and, past a
threshold, as `not meaningful`. Address arithmetic cannot separate those cases:
the two spans are nested and every extent in the overlap belongs to both.

The bytes can. This sweep canonicalises each dump's opening instruction window
and asks, of every extracted image's OWN CONTENT at that VA, which one
reproduces it. Output is a CSV of `entry,bytes,image,class,reason` - addresses,
image labels and one-line mechanical reasons, no dump text, so it is safe to
commit next to `worklist-classification.csv`.

The instrument that consumes it must not be edited by this sweep: the CSV is the
interface. `docs/tooling/dump-corpus-integrity.md` documents the classes and the
residue that stays unattributable.

Usage:
  scripts/ghidra-analysis/attribute-dump-extents.py
  scripts/ghidra-analysis/attribute-dump-extents.py --out /tmp/attr.csv
  scripts/ghidra-analysis/attribute-dump-extents.py --explain 801d84c0
  scripts/ghidra-analysis/attribute-dump-extents.py --all-extents
"""

import argparse
import collections
import csv
import glob
import importlib.util
import os
import re
import sys

try:
    import tomllib
except ImportError:  # py<3.11
    import tomli as tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
DEFAULT_FUNCS = os.path.join(REPO, "ghidra", "scripts", "funcs")
DEFAULT_EXTRACTED = os.path.join(REPO, "extracted")
OVERLAY_MAP = os.path.join(REPO, "crates", "asset", "data", "static-overlays.toml")
DEFAULT_OUT = os.path.join(HERE, "dump-extent-attribution.csv")

# One header parser for the whole corpus, shared with `scripts/ci/disc-coverage.py`.
sys.path.insert(0, HERE)
import dump_header  # noqa: E402

SCUS_BASE = 0x80010000
SCUS_HEADER = 0x800

# Signature length, and the floor below which a window carries too little signal
# to name an image. Both match `classify-worklist.py`'s static arbiter, so the
# two artifacts agree about what "these bytes are that image's" means.
WINDOW = 24
MIN_SIGNABLE = 8

# Floor for the AT-VA test only (see `attribute_dump`), and it is set from its
# own control (`--validate-short-floor`) rather than from judgement. Truncating
# every extent the full window resolves and re-running the at-VA test gives, over
# ~3000 trials: no wrong answer at ANY length down to one instruction, and a
# precision that falls off only through the honest direction - the short window
# names several images instead of one, which returns `identical` and credits all
# of them. Agreement is 99.9% at three instructions and 98.9% at one. Three is
# where the curve flattens, so it buys the reach without the extra imprecision.
SHORT_VA_FLOOR = 3

# A PROT entry's head, long enough that finding it inside another entry is the
# over-read and not a coincidence. Same probe `classify-worklist.py` uses.
OVER_READ_PROBE = 256

INSN_RE = re.compile(r"^([0-9a-fA-F]{8})\s+(\S+)\s*(.*)$")

MEM_MNEMONICS = {
    "lb", "lbu", "lh", "lhu", "lw", "lwl", "lwr",
    "sb", "sh", "sw", "swl", "swr",
}
ZERO_ABS_RE = re.compile(r",\s*-?(?:0x)?[0-9a-fA-F]+\(\$?zero\)")


def _cdbi():
    """The shared canonicaliser from `check-dump-base-integrity.py`."""
    path = os.path.join(HERE, "check-dump-base-integrity.py")
    spec = importlib.util.spec_from_file_location("_cdbi", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


CDBI = None  # set in main(), after arg parsing, so --help needs no capstone


# --- images ----------------------------------------------------------------

class Image:
    """One extracted image, cut down to the bytes its PROT entry owns.

    An extraction is the entry's `read_entry` footprint and runs into its
    neighbours' sectors, so the raw file answers for VAs its overlay never loads
    - with a neighbour's code. Two independent cuts are available and they are
    not equally good:

    * `clean_copy_bytes` from `static-overlays.toml`, a cited own-content
      length. Only two rows carry one.
    * the offset at which another image's head appears, sector-aligned. That is
      the neighbour's start, so it bounds the over-read.

    Where both exist they agree (`battle_action`: `0x28800` either way), which is
    what makes the second usable where the first is absent. `own_source` records
    which one a row used, because a `trim` cut is an inference and a
    `clean_copy_bytes` cut is a citation.
    """

    def __init__(self, label, prot, base, data, own_end, own_source):
        self.label = label
        self.prot = prot
        self.base = base
        self.data = data
        self.own_end = own_end
        self.own_source = own_source

    @property
    def name(self):
        return "%s(%d)" % (self.label, self.prot) if self.prot else self.label

    def offset_of(self, va):
        """File offset for a VA, or None when the VA is outside own content."""
        off = va - self.base
        if off < 0 or off >= self.own_end:
            return None
        return off

    def window_at(self, va, n):
        off = self.offset_of(va)
        if off is None or off + 4 * n > self.own_end:
            return None
        toks = CDBI.canon_bytes(self.data[off:off + 4 * n], n)
        return norm_tokens(toks) if len(toks) >= n else None


def load_images(extracted):
    rows = []
    if os.path.exists(OVERLAY_MAP):
        with open(OVERLAY_MAP, "rb") as fh:
            rows = tomllib.load(fh).get("overlays", [])

    raw = []
    for row in rows:
        label, base = row.get("label"), row.get("base_va")
        if not label or not base:
            continue
        cands = sorted(glob.glob(
            os.path.join(extracted, "overlays", "overlay_%s_*.bin" % label)))
        if not cands:
            continue
        with open(cands[0], "rb") as fh:
            raw.append((row, fh.read()))

    heads = [(r.get("label"), d[:OVER_READ_PROBE])
             for r, d in raw if len(d) >= OVER_READ_PROBE]

    images = []
    for row, data in raw:
        label = row["label"]
        cut = len(data)
        for other, head in heads:
            if other == label:
                continue
            at = data.find(head)
            if 0 < at < cut and at % 0x800 == 0:
                cut = at
        clean = row.get("clean_copy_bytes")
        if clean and clean <= len(data):
            own, src = clean, "clean_copy_bytes"
        elif cut < len(data):
            own, src = cut, "trim"
        else:
            own, src = len(data), "whole_file"
        images.append(Image(label, row.get("prot_index"), row["base_va"],
                            data, own, src))

    scus = os.path.join(extracted, "SCUS_942.54")
    if os.path.exists(scus):
        with open(scus, "rb") as fh:
            blob = fh.read()
        if blob[:8] == b"PS-X EXE":
            import struct
            t_addr, t_size = struct.unpack_from("<II", blob, 0x18)
            body = blob[SCUS_HEADER:SCUS_HEADER + t_size]
            images.append(Image("SCUS", None, t_addr, body, len(body), "exe_header"))
    return images


def measured_spans(extracted):
    """The spans `disc-coverage.py` measures - the ones an extent is ambiguous over.

    Its filter is exactly `base_va` and `clean_copy_bytes` both present and an
    extracted image on disk; a row without a cited own-content length has no
    honest denominator and is skipped there rather than guessed.
    """
    spans = []
    if not os.path.exists(OVERLAY_MAP):
        return spans
    with open(OVERLAY_MAP, "rb") as fh:
        for row in tomllib.load(fh).get("overlays", []):
            base, span, label = (row.get("base_va"), row.get("clean_copy_bytes"),
                                 row.get("label"))
            if not base or not span or not label:
                continue
            cands = sorted(glob.glob(
                os.path.join(extracted, "overlays", "overlay_%s_*.bin" % label)))
            if not cands:
                continue
            span = min(span, os.path.getsize(cands[0]))
            spans.append((base, base + span, label, row.get("prot_index")))
    return spans


# --- dumps -----------------------------------------------------------------

def read_dumps(funcs_dir):
    """[(entry, nbytes, stem, [(va, mnem, ops)])] over the whole corpus.

    The extent is `(entry, nbytes)` from the header, which is the same key
    `disc-coverage.py` builds its intervals from - not the filename address, and
    not the printed address of the first instruction.
    """
    out = []
    for path in sorted(glob.glob(os.path.join(funcs_dir, "*.txt"))):
        try:
            with open(path, errors="replace") as fh:
                lines = fh.readlines()
        except OSError:
            continue
        if not lines:
            continue
        # One header parser, shared with `disc-coverage.py`, so the two
        # instruments agree about which files are dumps and where each one
        # starts. A private regex here silently dropped real dumps whose header
        # uses one of the corpus's other spellings, and the resulting extents
        # then had no attribution row at all - which reads in the coverage
        # report as unresolvable ambiguity rather than as a parser gap.
        dump, _reject = dump_header.parse_text("".join(lines), path)
        if dump is None:
            continue
        entry, nbytes = dump.entry, dump.nbytes
        insns, in_dis = [], False
        for line in lines[2:]:
            if "--- DISASSEMBLY ---" in line:
                in_dis = True
                continue
            if not in_dis:
                continue
            t = line.rstrip("\n").strip()
            if t.startswith("---"):
                break
            if not t:
                if insns:
                    break
                continue
            im = INSN_RE.match(t)
            if im:
                insns.append((int(im.group(1), 16), im.group(2), im.group(3) or ""))
            if len(insns) >= WINDOW:
                break
        stem = os.path.splitext(os.path.basename(path))[0]
        out.append((entry, nbytes, stem, insns))
    return out


def looks_like_data(insns):
    """The `$zero`-absolute signature: a table decoded as opcodes.

    Real MIPS reaches statics through `gp` or a `lui`/`addiu` pair, so a run of
    `lb rN,0xNNNN(zero)` is a table of `0x80`-high bytes read as instructions. A
    window that matches an image here matches that image's DATA, and two images
    agreeing on nothing but a table must not read as a shared routine.
    """
    if not insns:
        return False
    hits = sum(1 for _, mn, ops in insns
               if mn.lower().lstrip("_") in MEM_MNEMONICS
               and ZERO_ABS_RE.search("," + ops))
    return hits * 2 >= len(insns)


def gapped(insns):
    """Non-contiguous printed addresses: Ghidra left holes in the body.

    Instructions are four bytes and a dumped body is contiguous, so a jump in
    the address column means every later instruction is offset against the
    image. Such a window can match no image as a contiguous run, and that
    failure says nothing about the corpus.
    """
    return any(b[0] - a[0] != 4 for a, b in zip(insns, insns[1:]))


# `break`'s operand is a 20-bit code the two disassemblers split differently:
# Ghidra prints the whole field (`break 0x1c00`), capstone a sub-field
# (`break 7`). The instruction is the same instruction, so the immediate is
# dropped on BOTH sides rather than compared. This is not a loosening chosen for
# convenience - it is the only systematic divergence a survey of near-miss
# windows turned up, and it matters out of proportion to its size because
# `div; bne; break 0x1c00` is the signed-division overflow check the compiler
# emits at every integer divide.
#
# The divergence lives in the shared canonicaliser, so it also inflates
# `check-dump-base-integrity.py`'s NOT_FOUND count; fixing it there is that
# tool's owner's call, and this normalisation is local until then.
def norm_tokens(toks):
    return ["BREAK||" if t.startswith("BREAK|") else t for t in toks]


def dump_tokens(insns):
    return norm_tokens([CDBI.canon(mn, ops) for _, mn, ops in insns[:WINDOW]])


# --- attribution -----------------------------------------------------------

def holders(images, va, toks):
    """Images whose own content reproduces this token window at this VA."""
    hits = []
    for img in images:
        w = img.window_at(va, len(toks))
        if w is not None and w == toks:
            hits.append(img)
    return hits


class Relocator:
    """Lazy index from a token signature to every (image, file offset).

    Answers "if the bytes are not at this VA, are they anywhere?" - the question
    that separates a mis-based print from a dump whose source was a live RAM
    capture no extraction covers. Only the first `SIG` tokens are indexed; a
    candidate is then re-checked over the full window.
    """

    SIG = 10

    def __init__(self, images):
        self.images = images
        self._idx = None

    def _build(self):
        idx = collections.defaultdict(list)
        for img in self.images:
            n = img.own_end // 4
            for w in range(max(0, n - self.SIG)):
                off = w * 4
                toks = CDBI.canon_bytes(img.data[off:off + 4 * self.SIG], self.SIG)
                if len(toks) == self.SIG:
                    idx["\n".join(norm_tokens(toks))].append((img, off))
        self._idx = idx

    def find(self, toks):
        if self._idx is None:
            self._build()
        out = []
        for img, off in self._idx.get("\n".join(toks[:self.SIG]), ()):
            w = norm_tokens(
                CDBI.canon_bytes(img.data[off:off + 4 * len(toks)], len(toks)))
            if w == toks:
                out.append((img, img.base + off))
        return out


def attribute_dump(images, reloc, entry, insns):
    """(class, [image names], reason) for one dump at one VA."""
    if not insns:
        return ("no_disassembly", [], "dump carries no instruction stream")
    if looks_like_data(insns):
        return ("data", [], "opening window is the `$zero`-absolute data "
                            "signature - a table decoded as opcodes")
    if gapped(insns):
        return ("gapped", [], "printed addresses are non-contiguous, so the "
                              "window matches no image as a contiguous run")
    toks = dump_tokens(insns)
    if len(toks) < MIN_SIGNABLE:
        # A short window can still answer the AT-VA question, and that is a
        # different question from the one the floor guards. `MIN_SIGNABLE` is
        # calibrated for the relocation SEARCH - "do these bytes appear anywhere
        # in any image" - where a short signature has millions of chances to
        # match by accident. `holders()` asks only "does THIS image's own
        # content reproduce this window at THIS VA", a two-way test at a fixed
        # offset with no multiple-comparison problem. A short window that
        # matches several images returns `identical`, which credits all of them
        # and is honest; the only outcome it cannot support is a `misbased`
        # verdict, so a short window that matches nothing stays `short` rather
        # than falling through to the search.
        if len(toks) >= SHORT_VA_FLOOR:
            hits = holders(images, entry, toks)
            if hits:
                names = [h.name for h in hits]
                kind = "unique" if len(hits) == 1 else "identical"
                return (kind, names,
                        "own content of %s reproduces the %d-instruction window "
                        "at this VA (short window, at-VA test only)"
                        % (", ".join(names), len(toks)))
            # The at-VA test RAN and no image's own content reproduces the
            # window. Say that, rather than repeating the floor message: the two
            # are different findings and only one of them is about the window's
            # length. These bytes most likely live at another VA, but a short
            # window is exactly what the relocation search cannot be trusted
            # with, so the extent stays residue rather than being called
            # `misbased` on evidence that would not support it.
            return ("short", [], "%d instructions: no image's own content "
                                 "reproduces this window at this VA, and the "
                                 "window is too short to search for it "
                                 "elsewhere" % len(toks))
        return ("short", [], "%d instructions, below the %d-instruction at-VA "
                             "floor for naming an image"
                             % (len(toks), SHORT_VA_FLOOR))

    hits = holders(images, entry, toks)
    if len(hits) == 1:
        img = hits[0]
        return ("unique", [img.name],
                "own content of %s reproduces the %d-instruction window at this "
                "VA (%s cut)" % (img.name, len(toks), img.own_source))
    if len(hits) > 1:
        names = [h.name for h in hits]
        return ("identical", names,
                "%d images hold byte-identical code here (%s) - the window "
                "cannot separate them" % (len(hits), ", ".join(names)))

    found = reloc.find(toks)
    if found:
        where = ", ".join("%s 0x%08x (delta +0x%x)"
                          % (img.name, va, (va - entry) & 0xFFFFFFFF)
                          for img, va in found[:3])
        return ("misbased", [], "no image holds these bytes at this VA; they "
                               "live at %s" % where)
    return ("unresolved", [], "no extracted image holds these bytes at this VA "
                              "or anywhere - RAM-capture-derived or an "
                              "un-extracted overlay")


# Ordered worst-to-best: when several dumps share an extent, the weakest verdict
# is the one the extent can support.
CLASS_RANK = {
    "unique": 0, "identical": 1, "misbased": 2, "unresolved": 3,
    "gapped": 4, "short": 5, "data": 6, "no_disassembly": 7,
}


def combine(per_dump):
    """One verdict for an extent several dumps produce.

    A `unique` verdict outranks everything: one dump resolving to one image's own
    content at this VA is positive evidence, and the others failing to sign is
    the absence of it. Two dumps resolving to DIFFERENT images is the one case
    that has to stay ambiguous, because both are positive.
    """
    uniques = {}
    for cls, names, reason in per_dump:
        if cls == "unique":
            uniques[names[0]] = reason
    if len(uniques) == 1:
        name, reason = next(iter(uniques.items()))
        return ("unique", [name], reason)
    if len(uniques) > 1:
        names = sorted(uniques)
        return ("divergent", names,
                "dumps at this extent resolve to different images (%s), so it is "
                "genuinely more than one routine" % ", ".join(names))
    best = min(per_dump, key=lambda t: CLASS_RANK.get(t[0], 99))
    return best


def validate_short_floor(images, reloc, by_extent):
    """Does the short at-VA test agree with the full-window verdict?

    A relaxation of a confidence floor is a measurement change, and a
    measurement change has no oracle - so it needs a control built from data the
    relaxation did not choose. This one is available for free: every extent the
    FULL window already resolves has a known answer, so truncating it to each
    short length and re-running the same at-VA test measures the short test's
    error rate directly.

    Three outcomes per trial, and they are not equally bad. `agree` is the
    short test reaching the same verdict. `weaker` is it returning several
    images where the full window named one - honest, since `identical` credits
    all of them, and the only cost is precision. `WRONG` is it naming a
    different single image, which is the failure that would put a false
    attribution in the CSV. Only the third invalidates the floor.
    """
    trials = collections.Counter()
    wrong = []
    for (entry, _nbytes), members in sorted(by_extent.items()):
        for _stem, insns in members:
            toks = dump_tokens(insns)
            if len(toks) < MIN_SIGNABLE:
                continue
            full = holders(images, entry, toks)
            if len(full) != 1:
                continue
            for n in range(SHORT_VA_FLOOR, MIN_SIGNABLE):
                if len(toks) < n:
                    break
                short = holders(images, entry, toks[:n])
                key = "n=%d" % n
                if not short:
                    trials[key + " none"] += 1
                elif len(short) == 1 and short[0].name == full[0].name:
                    trials[key + " agree"] += 1
                elif full[0].name in [s.name for s in short]:
                    trials[key + " weaker"] += 1
                else:
                    trials[key + " WRONG"] += 1
                    wrong.append((entry, n, full[0].name,
                                  [s.name for s in short]))
    print("control: short at-VA test vs the full-window verdict it should match")
    print("  agree = same single image · weaker = several, including the right "
          "one · WRONG = a different single image")
    for n in range(SHORT_VA_FLOOR, MIN_SIGNABLE):
        row = {k.split()[-1]: v for k, v in trials.items()
               if k.startswith("n=%d " % n)}
        total = sum(row.values())
        if not total:
            continue
        print("  %d insns: %5d trials  agree %5d (%.1f%%)  weaker %4d  "
              "none %4d  WRONG %d"
              % (n, total, row.get("agree", 0),
                 100.0 * row.get("agree", 0) / total, row.get("weaker", 0),
                 row.get("none", 0), row.get("WRONG", 0)))
    for entry, n, right, got in wrong[:20]:
        print("  WRONG 0x%08x at n=%d: full says %s, short says %s"
              % (entry, n, right, "|".join(got)))
    return 1 if wrong else 0


def main():
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--funcs", default=DEFAULT_FUNCS)
    ap.add_argument("--extracted", default=DEFAULT_EXTRACTED)
    ap.add_argument("--out", default=DEFAULT_OUT)
    ap.add_argument("--explain", default=None,
                    help="print per-dump evidence for one entry VA")
    ap.add_argument("--all-extents", action="store_true",
                    help="attribute every extent, not only the VA-ambiguous ones")
    ap.add_argument("--per-dump", action="store_true",
                    help="print one row per DUMP FILE rather than per extent - "
                         "the form a re-dump needs, since it must be told which "
                         "program to run against. Printed, never committed: it "
                         "is keyed to filenames over a gitignored corpus and "
                         "rots as soon as a dump is added.")
    ap.add_argument("--validate-short-floor", action="store_true",
                    help="control run for the SHORT_VA_FLOOR relaxation: take "
                         "every extent the full window resolves, truncate it to "
                         "each short length, and report how often the short "
                         "at-VA test gives the same answer. A relaxation that "
                         "cannot pass its own control is guesswork.")
    ap.add_argument("--min-insns", type=int, default=None,
                    help="signature floor; lower values trade confidence for "
                         "reach and are a sensitivity check, not a default "
                         "(default %d)" % MIN_SIGNABLE)
    args = ap.parse_args()

    globals()["CDBI"] = _cdbi()
    if args.min_insns:
        globals()["MIN_SIGNABLE"] = args.min_insns

    images = load_images(args.extracted)
    if not images:
        print("[attribute-dump-extents] no extracted images - nothing to do")
        return 0
    spans = measured_spans(args.extracted)
    dumps = read_dumps(args.funcs)
    if not dumps:
        print("[attribute-dump-extents] no dump corpus - nothing to do")
        return 0

    def ambiguous(va):
        return sum(1 for lo, hi, _, _ in spans if lo <= va < hi) > 1

    by_extent = collections.defaultdict(list)
    for entry, nbytes, stem, insns in dumps:
        if not args.all_extents and not ambiguous(entry):
            continue
        by_extent[(entry, nbytes)].append((stem, insns))

    reloc = Relocator(images)

    if args.explain:
        va = int(args.explain, 16)
        print("images covering 0x%08x by own content:" % va)
        for img in images:
            if img.offset_of(va) is not None:
                print("  %-24s base=0x%08x own_end=0x%-6x (%s)"
                      % (img.name, img.base, img.own_end, img.own_source))
        print("measured spans containing it: %s"
              % ", ".join(l for lo, hi, l, _ in spans if lo <= va < hi))
        for (entry, nbytes), members in sorted(by_extent.items()):
            if entry != va:
                continue
            print("extent 0x%08x +0x%x:" % (entry, nbytes))
            for stem, insns in members:
                cls, names, reason = attribute_dump(images, reloc, entry, insns)
                print("  %-52s %-12s %s" % (stem, cls, reason))
            print("  => %s" % (combine([attribute_dump(images, reloc, entry, i)
                                        for _, i in members]),))
        return 0

    if args.validate_short_floor:
        return validate_short_floor(images, reloc, by_extent)

    if args.per_dump:
        print("# dump\tentry\tbytes\timage\tclass")
        for (entry, nbytes), members in sorted(by_extent.items()):
            for stem, insns in sorted(members):
                cls, names, _ = attribute_dump(images, reloc, entry, insns)
                print("%s\t%08x\t%d\t%s\t%s"
                      % (stem, entry, nbytes, "|".join(names) or "-", cls))
        return 0

    rows = []
    for (entry, nbytes), members in sorted(by_extent.items()):
        per = [attribute_dump(images, reloc, entry, insns) for _, insns in members]
        cls, names, reason = combine(per)
        rows.append(("%08x" % entry, nbytes, "|".join(names) or "-", cls, reason))

    outp = os.path.abspath(args.out)
    os.makedirs(os.path.dirname(outp) or ".", exist_ok=True)
    with open(outp, "w", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(["entry", "bytes", "image", "class", "reason"])
        for row in rows:
            w.writerow(row)

    # Independent cross-check on the dumper defect: a dump is named after the
    # REQUESTED address but `getFunctionContaining` resolves the ENCLOSING
    # function, so the header's `entry=` is the truth and the filename is a
    # claim about an entry point that may not exist. The signature of that rule
    # is that the resolved entry is always BELOW the requested address; a
    # mismatch in the other direction would mean something else is wrong.
    named = mismatched = below = 0
    for entry, _nbytes, stem, _insns in dumps:
        m = re.search(r"([0-9a-f]{8})$", stem)
        if not m:
            continue
        named += 1
        req = int(m.group(1), 16)
        if req != entry:
            mismatched += 1
            below += 1 if entry < req else 0
    print("dumps whose filename address is not their `entry=`: %d of %d "
          "(%d resolve BELOW the requested address%s)"
          % (mismatched, named, below,
             ", i.e. all of them" if below == mismatched else ""))

    hist = collections.Counter(c for _, _, _, c, _ in rows)
    total = len(rows)
    print("\nambiguous extents: %d" % total)
    for cls, n in hist.most_common():
        print("  %-15s %5d  (%4.1f%%)" % (cls, n, 100.0 * n / total if total else 0))

    resolved = hist["unique"]
    print("\nattributed to exactly one image : %d (%.1f%%)"
          % (resolved, 100.0 * resolved / total if total else 0))
    print("excluded from every image       : %d (misbased + data + gapped)"
          % (hist["misbased"] + hist["data"] + hist["gapped"]))
    print("stays ambiguous                 : %d (identical + divergent + "
          "unresolved + short + no_disassembly)"
          % (hist["identical"] + hist["divergent"] + hist["unresolved"]
             + hist["short"] + hist["no_disassembly"]))

    credited = collections.Counter()
    for _, _, image, cls, _ in rows:
        if cls == "unique":
            credited[image] += 1
    print("\nunique attributions by image:")
    for name, n in credited.most_common():
        measured = any(name.startswith(l + "(") for _, _, l, _ in spans)
        print("  %-24s %5d%s" % (name, n, "" if measured else "   [not measured]"))

    # What the attribution does to the two rows the coverage report calls "not
    # meaningful". An extent stops being ambiguous FOR an image once the bytes
    # say it belongs to some other image, or to no image at all - both of those
    # remove it from this image's set without a judgement call about which of
    # the overlapping spans deserves it.
    verdict = {(int(e, 16), b): (img, cls) for e, b, img, cls, _ in rows}
    all_extents = {(e, n) for e, n, _, _ in dumps}
    print("\nprojected effect on the measured rows (distinct extents):")
    print("  %-18s %7s %7s %7s %7s %7s %8s %8s" % (
        "image", "extents", "keep", "other", "exclude", "residue",
        "amb% now", "after"))
    for lo, hi, label, prot in spans:
        name = "%s(%d)" % (label, prot)
        mine = [(e, n) for e, n in all_extents if lo <= e < hi]
        keep = other = excl = residue = 0
        for key in mine:
            got = verdict.get(key)
            if got is None:
                # Unambiguous already: only one measured span contains it.
                continue
            img, cls = got
            if cls == "unique":
                keep += 1 if img == name else 0
                other += 0 if img == name else 1
            elif cls in ("misbased", "data", "gapped"):
                excl += 1
            elif cls == "identical" and name in img.split("|"):
                keep += 1
            else:
                residue += 1
        amb_now = keep + other + excl + residue
        after_total = len(mine) - other - excl
        print("  %-18s %7d %7d %7d %7d %7d %7.1f%% %7.1f%%" % (
            name, len(mine), keep, other, excl, residue,
            100.0 * amb_now / len(mine) if mine else 0.0,
            100.0 * residue / after_total if after_total else 0.0))
    print("  keep = bytes say this image · other = bytes say a different image")
    print("  exclude = belongs to no image at any VA · residue = unattributable")
    print("  `after` divides residue by the extents that remain in the image's "
          "set, since `other` and `exclude` leave it entirely")

    print("\nwrote %s" % outp)
    return 0


if __name__ == "__main__":
    sys.exit(main())
