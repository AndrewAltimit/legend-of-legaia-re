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

SCUS_BASE = 0x80010000
SCUS_HEADER = 0x800

# Signature length, and the floor below which a window carries too little signal
# to name an image. Both match `classify-worklist.py`'s static arbiter, so the
# two artifacts agree about what "these bytes are that image's" means.
WINDOW = 24
MIN_SIGNABLE = 8

# A PROT entry's head, long enough that finding it inside another entry is the
# over-read and not a coincidence. Same probe `classify-worklist.py` uses.
OVER_READ_PROBE = 256

HDR_RE = re.compile(r"^==\s+(\S+)\s+([0-9a-fA-F]{8})\s+\(entry=([0-9a-fA-F]{8})\)")
SIZE_RE = re.compile(r"^size=(\d+) bytes,\s*(\d+) instructions")
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
        m = HDR_RE.match(lines[0].strip())
        s = SIZE_RE.match(lines[1].strip()) if len(lines) > 1 else None
        if not m or not s:
            continue
        nbytes = int(s.group(1))
        if nbytes <= 0:
            continue
        entry = int(m.group(3), 16)
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
        return ("short", [], "%d instructions, below the %d-instruction floor "
                             "for naming an image" % (len(toks), MIN_SIGNABLE))

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
