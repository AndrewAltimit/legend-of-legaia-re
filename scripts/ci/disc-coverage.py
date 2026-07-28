#!/usr/bin/env python3
"""Disc-denominated coverage: how much of the game's own bytes we can explain.

`port-catalog.py` measures three status columns over the set of addresses this
project has *cited*. That is the right tool for tracking work, but its
denominator is our own documentation, so it can never say how much of the game
is left. A page can read "99.9% covered" while an entire un-dumped subsystem
sits outside the citation graph, because nothing cites it.

This script takes the denominator from the disc instead.

Two measurements, and they are NOT the same kind of number - the report says so
in its own text, because quoting them interchangeably is the obvious way to
misuse this page:

  CODE  - byte-exact. A Ghidra dump header states the body's entry and size, so
          the dumped functions are real byte intervals. Merge them, subtract
          from an image's extent, and every remaining byte is genuinely
          un-dumped. Gaps are then classified code-vs-data so the rodata an
          executable carries inside its text segment does not inflate the
          denominator. The header is parsed by `dump_header`, shared with the
          attribution sweep - the corpus spells every header field more than
          one way and a private regex silently under-counts.

  DATA  - format RECOGNITION, one level coarser. `asset categorize` says which
          format class each PROT entry is. Knowing an entry is a
          `scene_vab_stream` is not the same as accounting for every byte
          inside it, and no parser currently reports consumed-vs-unconsumed
          bytes. Treat the data figure as an upper bound.

Overlay images alias in VA space, so an address alone cannot say which of them a
dump belongs to. `scripts/ghidra-analysis/dump-extent-attribution.csv` carries
the per-extent verdict the *bytes* give, and this script applies it: an extent
the bytes place in another image (or in none) leaves the row rather than
inflating it. That file is committed and keyed by `(entry, bytes)`, so it does
not rot when a dump lands. It is optional - without it every overlay extent
stays ambiguous by address, which is the same honest upper bound, just looser.

Disc-gated, like the rest of the repo: with no `extracted/` tree and no dump
corpus this exits 0 and reports SKIPPED, so CI passes without disc data. Both
inputs are gitignored, so this only produces numbers on a developer's machine.

The DATA half reads a *cache* (`extracted/PROT/categorize.json`) that nothing
here regenerates. A tree whose `categorize` detectors have moved on will report
the old classification through a passing gate. Re-run `asset categorize
extracted/PROT` before trusting a data figure or taking a baseline.

No Sony bytes are emitted - the report carries addresses, byte counts and class
names, the same things the committed docs already carry.

Usage:
    python3 scripts/ci/disc-coverage.py                 # report to stdout + target/
    python3 scripts/ci/disc-coverage.py --md            # markdown to stdout
    python3 scripts/ci/disc-coverage.py --check         # ratchet against the baseline
    python3 scripts/ci/disc-coverage.py --update-baseline
"""

from __future__ import annotations

import argparse
import csv
import glob
import json
import os
import struct
import sys
import tomllib

# scripts/ci/disc-coverage.py -> repo root is three levels up, matching
# port-catalog.py's `Path(__file__).resolve().parent.parent.parent`.
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# The dump header has one parser, shared with the attribution sweep. Each
# instrument over this corpus used to carry its own regex, and because the
# corpus spells every header field more than one way, each silently rejected a
# different subset of real dumps as "no parseable header". See
# `scripts/ghidra-analysis/dump_header.py`.
sys.path.insert(0, os.path.join(REPO, "scripts", "ghidra-analysis"))
import dump_header  # noqa: E402

DEFAULT_FUNCS = os.path.join(REPO, "ghidra", "scripts", "funcs")
DEFAULT_EXTRACTED = os.path.join(REPO, "extracted")
DEFAULT_OUT = os.path.join(REPO, "target", "disc-coverage")
OVERLAY_MAP = os.path.join(REPO, "crates", "asset", "data", "static-overlays.toml")
BASELINE = os.path.join(REPO, "scripts", "ci", "disc-coverage-baseline.json")
ATTRIBUTION = os.path.join(
    REPO, "scripts", "ghidra-analysis", "dump-extent-attribution.csv")

# MIPS I primary opcodes the R3000A actually issues. Used only to tell a gap of
# code from a gap of data; it is a statistical test over a whole gap, never a
# per-instruction decode.
PLAUSIBLE_OPS = {
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
    16, 17, 18, 32, 33, 34, 35, 36, 37, 38, 40, 41, 42, 43, 46, 47, 49, 57,
}
# A gap counts as code when nearly every word decodes to a plausible opcode AND
# it is not dense with 0x80xxxxxx words (the signature of a pointer table).
CODE_PLAUSIBLE_MIN = 0.90
CODE_PTR_MAX = 0.03
# Gaps shorter than this are inter-function alignment padding, not a finding.
TINY_GAP_WORDS = 8

# PROT classes that are placeholders or absence rather than an unparsed format.
# `pochi_filler` is a DOCUMENTED class (docs/formats/pochi.md), so it counts as
# explained; it is broken out separately because calling reserved dev filler
# "content we understand" overstates the result.
PLACEHOLDER_CLASSES = {"pochi_filler", "mostly_zeros", "zero_sector_high_entropy"}
UNEXPLAINED_CLASSES = {
    "unknown", "unknown_other", "unknown_high_entropy", "unknown_low_entropy",
}


def read_dump_extents(funcs_dir):
    """Every dump's (entry_va, end_va), plus a census of what was excluded.

    Returns `(extents, rejects)` where `rejects` counts `dump_header`'s reject
    classes. The census is the point: a single "N files had no parseable
    header" number invites - and previously carried - a wrong explanation of
    what those files are. Most of them are the corpus storing an ANSWER, and
    only a handful are defective dumps.
    """
    out = []
    rejects = {}
    for path in sorted(glob.glob(os.path.join(funcs_dir, "*.txt"))):
        dump, reject = dump_header.parse_file(path)
        if dump is None:
            rejects[reject] = rejects.get(reject, 0) + 1
            continue
        out.append(dump.extent)
    return out, rejects


# Extent classes in `dump-extent-attribution.csv` whose verdict is that the
# bytes belong to no mapped image at all - a mis-based print, a gapped stream,
# or a region that does not disassemble. Crediting them to the image whose span
# happens to contain the printed VA is exactly the fiction this filter removes.
CREDIT_NOBODY = {"misbased", "data", "gapped"}
# Classes that name the owning image(s) by bytes. `identical` names several
# because they hold byte-identical code there, and each of them really does
# contain those bytes, so each is credited.
CREDIT_NAMED = {"unique", "identical"}
# Everything else (`short`, `unresolved`, `no_disassembly`) is residue: the
# bytes could not place the extent, so it stays ambiguous for every image whose
# span contains it.


def read_attribution(path=ATTRIBUTION):
    """`{(entry, end): owner_labels_or_None}` from the byte-attribution CSV.

    `None` means "credit nobody". A key absent from the map is residue - either
    the CSV classed it unresolvable or the CSV is not there at all - and callers
    treat both the same way, which is what makes attribution optional.

    Keyed by `(entry, bytes)`, the same key `read_dump_extents` builds, so the
    map is about *extents* rather than dump filenames and does not rot when a
    dump lands, is renamed, or is re-dumped at the same VA.
    """
    if not os.path.exists(path):
        return {}
    out = {}
    with open(path, newline="") as fh:
        for row in csv.DictReader(fh):
            try:
                entry, nbytes = int(row["entry"], 16), int(row["bytes"])
            except (KeyError, TypeError, ValueError):
                continue
            klass = row.get("class")
            if klass in CREDIT_NOBODY:
                out[(entry, entry + nbytes)] = None
            elif klass in CREDIT_NAMED:
                out[(entry, entry + nbytes)] = {
                    n.split("(")[0] for n in (row.get("image") or "").split("|")
                    if n and n != "-"}
    return out


def merge(intervals):
    merged = []
    for a, b in sorted(intervals):
        if merged and a <= merged[-1][1]:
            merged[-1][1] = max(merged[-1][1], b)
        else:
            merged.append([a, b])
    return merged


MIPS_JR_RA = 0x03E00008
# `addiu $t2, $zero, imm` - the register a PSX BIOS-call thunk loads its jump
# vector into before `jr $t2`.
_ADDIU_T2_ZERO = 0x240A0000
BIOS_VECTORS = (0xA0, 0xB0, 0xC0)


def gap_shape(image, base_va, a, b):
    """Why a code gap is a gap. Four shapes, and only one of them is work.

    A gap is not automatically an un-analysed routine, and reporting the total
    as if it were makes the last percent read as a dump worklist when most of it
    can never be closed by dumping:

    | shape | what it is |
    |---|---|
    | `padding` | every word is `nop`. Inter-function alignment; no function body will ever contain it. |
    | `return_tail` | `jr ra` (+ `nop`): a routine's exit that its analysed body stops short of. |
    | `bios_thunk_slot` | the delay slot of a `jr $t2` BIOS-call thunk. The body ends at the `jr` because the target is a register, so the slot falls outside it. |
    | `code` | genuinely un-dumped instructions. |

    The three non-`code` shapes are properties of where a function *body* ends,
    not of what has been analysed, so they persist however much is dumped.
    """
    n = (b - a) // 4
    start = a - base_va
    if n <= 0 or start < 0 or start + n * 4 > len(image):
        return "code"
    words = struct.unpack_from("<%dI" % n, image, start)
    if all(w == 0 for w in words):
        return "padding"
    if words[0] == MIPS_JR_RA and all(w == 0 for w in words[1:]):
        return "return_tail"
    if start >= 8:
        pre = struct.unpack_from("<2I", image, start - 8)
        # `addiu $t2, $zero, 0xA0|B0|C0` then `jr $t2`
        if ((pre[0] & 0xFFFF0000) == _ADDIU_T2_ZERO
                and (pre[0] & 0xFFFF) in BIOS_VECTORS
                and (pre[1] & 0xFC1FFFFF) == 0x00000008):
            return "bios_thunk_slot"
    return "code"


def classify_gap(image, base_va, a, b):
    """True when the bytes in [a, b) look like code rather than data."""
    n = (b - a) // 4
    if n < TINY_GAP_WORDS:
        return True
    start = a - base_va
    if start < 0 or start + n * 4 > len(image):
        return False
    words = struct.unpack_from("<%dI" % n, image, start)
    plausible = sum(1 for w in words if (w >> 26) in PLAUSIBLE_OPS) / n
    ptrs = sum(1 for w in words if 0x80000000 <= w < 0x80200000) / n
    return plausible >= CODE_PLAUSIBLE_MIN and ptrs < CODE_PTR_MAX


def cover_image(name, image, base_va, span, extents, attrib=None):
    """Coverage of one loaded image. `span` is its byte length.

    `attrib` is the byte-attribution map. Where it places an extent in some
    other image - or in none - the extent is dropped from this image rather
    than counted for it. Pass `None` for an image with no VA aliasing
    (`SCUS_942.54`): the filter must not touch a row that is already exact.
    """
    lo, hi = base_va, base_va + span
    mine, dropped = [], 0
    for a, b in extents:
        if not lo <= a < hi:
            continue
        owners = (attrib or {}).get((a, b), "residue")
        if owners != "residue" and (owners is None or name not in owners):
            dropped += 1
            continue
        mine.append((a, min(b, hi)))
    merged = merge(mine)
    covered = sum(b - a for a, b in merged)

    gaps, prev = [], lo
    for a, b in merged:
        if a > prev:
            gaps.append((prev, a))
        prev = b
    if prev < hi:
        gaps.append((prev, hi))

    code_gap = data_gap = 0
    code_gaps = []
    shapes = {}
    for a, b in gaps:
        if classify_gap(image, base_va, a, b):
            code_gap += b - a
            shape = gap_shape(image, base_va, a, b)
            n, nb = shapes.get(shape, (0, 0))
            shapes[shape] = (n + 1, nb + b - a)
            if shape == "code":
                code_gaps.append((a, b))
        else:
            data_gap += b - a

    denom = covered + code_gap
    return {
        "name": name,
        "base_va": base_va,
        "span": span,
        "dumps": len(mine),
        "attributed_out": dropped,
        "covered": covered,
        "code_gap": code_gap,
        "data_gap": data_gap,
        "code_denominator": denom,
        "pct": (100.0 * covered / denom) if denom else 0.0,
        "gap_shapes": shapes,
        "top_code_gaps": sorted(code_gaps, key=lambda g: g[0] - g[1])[:8],
    }


def scus_report(extracted, extents):
    path = os.path.join(extracted, "SCUS_942.54")
    if not os.path.exists(path):
        return None
    blob = open(path, "rb").read()
    if blob[:8] != b"PS-X EXE":
        return None
    t_addr, t_size = struct.unpack_from("<II", blob, 0x18)
    # The PS-X EXE header's load image starts at file offset 0x800.
    image = blob[0x800:0x800 + t_size]
    return cover_image("SCUS_942.54", image, t_addr, t_size, extents)


def overlay_reports(extracted, extents, attrib=None):
    if not os.path.exists(OVERLAY_MAP):
        return [], (0, 0, 0)
    attrib = attrib or {}
    rows = tomllib.load(open(OVERLAY_MAP, "rb")).get("overlays", [])
    out = []
    spans = []
    for row in rows:
        base = row.get("base_va")
        span = row.get("clean_copy_bytes")
        label = row.get("label")
        if not base or not span or not label:
            # `field` (0897) has no clean_copy_bytes: its own content length is
            # not established, so it has no honest denominator. Skipped rather
            # than guessed.
            continue
        candidates = sorted(glob.glob(
            os.path.join(extracted, "overlays", "overlay_%s_*.bin" % label)))
        if not candidates:
            continue
        image = open(candidates[0], "rb").read()[:span]
        if len(image) < span:
            span = len(image)
        row = cover_image(label, image, base, span, extents, attrib=attrib)
        row["_image_span"] = (base, base + span)
        out.append(row)
        spans.append((base, base + span, label))

    # Overlays alias in VA space (several share 0x801CE818, and the two measured
    # spans are nested), so an extent can fall inside more than one image's span
    # and be counted by each. That is a real ambiguity, not something to paper
    # over: quantify it and let the reader discount accordingly.
    #
    # This whole block counts DISTINCT extents, not dump files. One extent can
    # back dozens of dumps - the phantom-print batches are the extreme case -
    # and weighting the ambiguity by how often we happened to dump the same
    # bytes measures the corpus rather than the image. Distinct extents are also
    # the key `dump-extent-attribution.csv` is written on, so the report and the
    # artifact are the same denominator and can be compared directly.
    distinct = sorted(set(extents))
    ambiguous = [k for k in distinct
                 if sum(1 for lo, hi, _ in spans if lo <= k[0] < hi) > 1]
    resolved = sum(1 for k in ambiguous if k in attrib)
    totals = (len(ambiguous), resolved, len(ambiguous) - resolved)

    # Per-image share of this image's extents that the bytes could not place.
    # An extent the bytes assign elsewhere is no longer ambiguous *for this
    # image* - it is simply not this image's - so it leaves the numerator and
    # the denominator both. A row whose remaining share is high has no
    # defensible number at all, and the table says so ON the row rather than in
    # prose underneath it.
    for row in out:
        lo, hi = row.pop("_image_span")
        mine = [k for k in distinct if lo <= k[0] < hi]
        resid = dropped = 0
        for k in mine:
            if sum(1 for l2, h2, _ in spans if l2 <= k[0] < h2) <= 1:
                continue  # unambiguous by address; attribution has nothing to do
            owners = attrib.get(k, "residue")
            if owners == "residue":
                resid += 1
            elif owners is None or row["name"] not in owners:
                dropped += 1
        kept = len(mine) - dropped
        row["ambiguous"] = resid
        row["ambiguous_pct"] = (100.0 * resid / kept) if kept else 0.0
    return out, totals


def data_report(extracted):
    cat = os.path.join(extracted, "PROT", "categorize.json")
    if not os.path.exists(cat):
        return None
    per_file = json.load(open(cat)).get("per_file", [])
    if not per_file:
        return None
    # `asset categorize` emits `per_file` as a filename -> record MAP. Older
    # extractions on disk carry a LIST of records that each repeat `path`.
    # Accept both: while the shapes disagreed this gate ran green only because
    # nobody had re-run `legaia-extract`, and re-running it crashed the gate
    # with `'str' object has no attribute 'get'` (iterating a dict yields keys).
    records = list(per_file.values()) if isinstance(per_file, dict) else per_file
    by = {}
    for e in records:
        klass = e.get("class") or "?"
        n = e.get("size") or 0
        c = by.setdefault(klass, [0, 0])
        c[0] += 1
        c[1] += n
    total = sum(v[1] for v in by.values())
    placeholder = sum(v[1] for k, v in by.items() if k in PLACEHOLDER_CLASSES)
    unexplained = sum(v[1] for k, v in by.items() if k in UNEXPLAINED_CLASSES)
    parsed = total - placeholder - unexplained
    return {
        "entries": sum(v[0] for v in by.values()),
        "total": total,
        "parsed": parsed,
        "placeholder": placeholder,
        "unexplained": unexplained,
        "pct_parsed": 100.0 * parsed / total if total else 0.0,
        "pct_unexplained": 100.0 * unexplained / total if total else 0.0,
        "by_class": sorted(
            ([k, v[0], v[1]] for k, v in by.items()), key=lambda r: -r[2]),
    }


GAP_SHAPE_TEXT = {
    "code": "genuinely un-dumped instructions - the only shape that is work",
    "padding": "every word is `nop`: inter-function alignment, which no function "
               "body will ever contain",
    "return_tail": "`jr ra` (+ `nop`) that the preceding routine's analysed body "
                   "stops short of",
    "bios_thunk_slot": "the delay slot of a `jr $t2` PSX BIOS-call thunk; the "
                       "body ends at the `jr` because its target is a register",
}

REJECT_TEXT = {
    "pointer_stub": ("answer", "recorded interior citation naming its enclosing "
                     "dump - the corpus's correct handling of a mid-function "
                     "address, not a missing dump"),
    "nofunc_record": ("answer", "recorded negative: no analyzed function at or "
                      "containing that address"),
    "data_window": ("answer", "a fixed hex or address-range window, declared as "
                    "a window rather than a body"),
    "not_a_dump": ("answer", "an analysis script's output whose filename happens "
                   "to end `_<addr>.txt` - xref sweeps, listings, notes"),
    "zero_insns": ("defect", "states a size but `0 instructions`: Ghidra decoded "
                   "none, so the window is data being asked for as code"),
    "no_extent": ("defect", "a body dump stating neither a size nor a signable "
                  "instruction stream"),
    "gapped_stream": ("defect", "printed addresses stop being consecutive, so "
                      "the range between them is not evidenced"),
    "empty_dump": ("defect", "a header with no body at all - the dump script "
                   "wrote its header and then failed"),
    "zero_bytes": ("defect", "states `size=0`"),
    "no_entry": ("defect", "an extent with no recoverable entry address"),
}


def render(scus, overlays, amb_totals, data, rejects, attributed):
    ambiguous, resolved, residue = amb_totals
    L = []
    add = L.append
    add("# Disc coverage")
    add("")
    add("Generated by `scripts/ci/disc-coverage.py`. Unlike "
        "[`port-catalog.py`](port-catalog.py), whose denominator is the set of "
        "addresses this project cites, every figure here is denominated in the "
        "game's own bytes.")
    add("")
    add("**The two halves are different kinds of measurement.** Code coverage is "
        "byte-exact: a byte is inside a dumped function or it is not. Data "
        "coverage is format *recognition* - knowing an entry's format class is "
        "not the same as accounting for every byte inside it, and no parser "
        "reports consumed-vs-unconsumed bytes yet. Do not quote them "
        "interchangeably.")
    add("")

    add("## Code")
    add("")
    add("A gap between dumped functions is classified as code or data by opcode "
        "plausibility and pointer density, so the rodata an executable carries "
        "inside its text segment does not inflate the denominator. Gaps under "
        f"{TINY_GAP_WORDS} words are inter-function alignment and count as code.")
    add("")
    add("| image | base | span | dumps | in a dump | code gap | data gap | code denom | covered | VA-ambiguous |")
    add("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    rows = ([scus] if scus else []) + overlays
    for r in rows:
        amb = r.get("ambiguous_pct")
        # An image whose dumps are mostly claimable by a sibling overlay has no
        # defensible figure. Say that ON THE ROW - a caveat in prose underneath
        # does not travel when the table is quoted on its own.
        if amb is None:
            cover, ambcell = "**%.1f%%**" % r["pct"], "-"
        elif amb >= 50.0:
            cover, ambcell = "not meaningful", "%.1f%%" % amb
        elif amb > 0.0:
            cover, ambcell = "<= %.1f%%" % r["pct"], "%.1f%%" % amb
        else:
            cover, ambcell = "**%.1f%%**" % r["pct"], "0%"
        add("| `%s` | `0x%08X` | %d | %d | %d | %d | %d | %d | %s | %s |" % (
            r["name"], r["base_va"], r["span"], r["dumps"], r["covered"],
            r["code_gap"], r["data_gap"], r["code_denominator"], cover, ambcell))
    add("")
    if attributed:
        add("**VA-ambiguous** is the share of an image's extents that the *bytes* "
            "could not place: extents whose entry address lands in more than one "
            "mapped overlay span and which byte attribution left unresolved. An "
            "extent the bytes assign to another image leaves this row entirely - "
            "it is not this image's, so it is neither ambiguous for it nor "
            "counted against it.")
    else:
        add("**VA-ambiguous** is the share of an image's extents whose entry "
            "address also lands inside another mapped overlay's span.")
    add("At 50% or more the coverage figure is not reported, because what is "
        "left cannot support one.")
    add("")
    add("The share counts **distinct extents**, not dump files: one extent can "
        "back many dumps, and weighting by how often the same bytes were dumped "
        "would measure the corpus rather than the image. It is the same "
        "denominator `dump-extent-attribution.csv` is keyed on, so the two can "
        "be read against each other directly. The **dumps** column is the other "
        "denominator and stays per dump file"
        + (", counting only the dumps the bytes place in this image."
           if attributed else "."))
    add("")
    if scus:
        add("`SCUS_942.54` is the only image here with an unambiguous answer: it "
            "is a single load image at a fixed base with no VA aliasing.")
        add("")
        shapes = scus.get("gap_shapes") or {}
        if shapes:
            add("### What the `SCUS_942.54` code gap is")
            add("")
            add("Not every gap is an un-analysed routine, and reading the total "
                "as a worklist overstates what dumping can close. Three of the "
                "four shapes are properties of where a function *body* ends "
                "rather than of what has been analysed, so they persist however "
                "much is dumped.")
            add("")
            add("| shape | gaps | bytes | what it is |")
            add("|---|---:|---:|---|")
            for key in ("code", "padding", "return_tail", "bios_thunk_slot"):
                if key not in shapes:
                    continue
                n, nb = shapes[key]
                add("| `%s` | %d | %d | %s |" % (key, n, nb, GAP_SHAPE_TEXT[key]))
            add("")
        if scus["top_code_gaps"]:
            add("Largest un-dumped **code** runs in `SCUS_942.54` - this is a dump "
                "worklist, not a defect list:")
            add("")
            add("| range | bytes | instructions |")
            add("|---|---:|---:|")
            for a, b in scus["top_code_gaps"]:
                add("| `0x%08X`..`0x%08X` | %d | %d |" % (a, b, b - a, (b - a) // 4))
            add("")
    if overlays and attributed:
        add("### Overlay caveat")
        add("")
        add("Overlay images alias in VA space - several share base `0x801CE818`, "
            "and the two measured spans are nested - so an extent in that band "
            f"cannot be attributed by address. **{resolved}** of the "
            f"**{ambiguous}** ambiguous extents are resolved by bytes against the "
            "extracted images "
            "(`scripts/ghidra-analysis/dump-extent-attribution.csv`); "
            f"**{residue}** remain unattributable and keep those rows an upper "
            "bound. See "
            "[`dump-corpus-integrity.md`](../../docs/tooling/dump-corpus-integrity.md) "
            "and [`phantom-print-index.md`](../../docs/tooling/phantom-print-index.md).")
        add("")
        add("What the residue is decides what closes it, and re-dumping is no "
            "longer the answer for most of it. Three shapes remain, in "
            "descending size: windows a few instructions long that no image's "
            "own content reproduces at that VA - too short to search for "
            "elsewhere without inviting a coincidence; extents whose bytes are "
            "in no extracted image at any VA, which need an *extraction* rather "
            "than a dump, most of them dumped from live RAM captures of "
            "overlays that have never been extracted statically; and extents "
            "where two dumps genuinely disagree, which is several routines "
            "sharing a range and is a real answer rather than a gap.")
        add("")
        add("The inner of two nested spans starts at total ambiguity, because "
            "every extent in it falls in both spans by construction. That much "
            "is structural. It is **not** a reason the row cannot be measured: "
            "the bytes place most of those extents in one image or the other, "
            "and what is structural is only the starting point, not the "
            "outcome.")
        add("")
    elif overlays:
        add("### Overlay caveat")
        add("")
        add("Overlay images alias in VA space - several share base `0x801CE818` - "
            "so a dump whose entry lands in that band cannot be attributed to one "
            "image by address alone. Overlay rows are therefore an **upper "
            "bound**: a dump counted for one image may belong to another. "
            f"**{ambiguous}** distinct dump extents fall inside more than one "
            "mapped overlay span. Resolving them needs byte-level attribution "
            "against the extracted images - see "
            "[`dump-corpus-integrity.md`](../../docs/tooling/dump-corpus-integrity.md) "
            "and [`phantom-print-index.md`](../../docs/tooling/phantom-print-index.md).")
        add("")
        add("Byte-level attribution is not available "
            "(`scripts/ghidra-analysis/dump-extent-attribution.csv` absent); "
            "overlay rows are attributed by address alone.")
        add("")
    add("### Files excluded from the numerator")
    add("")
    answers = sum(n for k, n in rejects.items()
                  if REJECT_TEXT.get(k, ("defect",))[0] == "answer")
    defects = sum(rejects.values()) - answers
    add("`%d` file(s) in the dump directory are not counted as evidence. They "
        "are not one population: **%d** are the corpus storing an *answer* "
        "rather than a dump, and only **%d** are defective dumps. A single "
        "count over the two invites the wrong reading of what is left to "
        "repair." % (sum(rejects.values()), answers, defects))
    add("")
    add("| class | files | kind | what it is |")
    add("|---|---:|---|---|")
    for k, n in sorted(rejects.items(), key=lambda r: -r[1]):
        kind, why = REJECT_TEXT.get(k, ("defect", "unclassified"))
        add("| `%s` | %d | %s | %s |" % (k, n, kind, why))
    add("")
    add("Header parsing is shared with the attribution sweep "
        "(`scripts/ghidra-analysis/dump_header.py`) so the two agree about what "
        "a dump is. The corpus spells every header field more than one way, and "
        "an instrument with its own regex rejects a different subset of real "
        "dumps as unparseable - which reads as a corpus gap and is really a "
        "parser one.")
    add("")

    add("## Data")
    add("")
    if not data:
        add("No `extracted/PROT/categorize.json`. Generate it with "
            "`asset categorize extracted/PROT`.")
    else:
        add("Format recognition over every PROT entry, weighted by bytes.")
        add("")
        add("| | bytes | share |")
        add("|---|---:|---:|")
        add("| parsed to a named format | %d | %.1f%% |" % (
            data["parsed"], data["pct_parsed"]))
        add("| documented placeholder / padding | %d | %.1f%% |" % (
            data["placeholder"], 100.0 * data["placeholder"] / data["total"]))
        add("| **unexplained** | %d | **%.1f%%** |" % (
            data["unexplained"], data["pct_unexplained"]))
        add("| total | %d | |" % data["total"])
        add("")
        add("Placeholder covers reserved dev filler and zero padding. It is "
            "*explained* - `pochi_filler` has its own format page - but counting "
            "it as content we understand would overstate the result, so it is "
            "broken out.")
        add("")
        add("| class | entries | bytes | share |")
        add("|---|---:|---:|---:|")
        for k, n, b in data["by_class"][:16]:
            add("| `%s` | %d | %d | %.1f%% |" % (
                k, n, b, 100.0 * b / data["total"]))
        add("")
    return "\n".join(L) + "\n"


def snapshot(scus, overlays, data):
    out = {"code": {}, "data": {}}
    for r in ([scus] if scus else []) + overlays:
        # Only ratchet figures that mean something. A VA-ambiguous overlay row
        # moves with dump attribution rather than with real coverage, so
        # baselining it would produce failures nobody can act on.
        if r.get("ambiguous_pct", 0.0) >= 50.0:
            continue
        out["code"][r["name"]] = round(r["pct"], 2)
    if data:
        out["data"]["pct_parsed"] = round(data["pct_parsed"], 2)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--funcs", default=DEFAULT_FUNCS)
    ap.add_argument("--extracted", default=DEFAULT_EXTRACTED)
    ap.add_argument("--attribution", default=ATTRIBUTION,
                    help="byte-level extent attribution CSV; absent = attribute "
                         "overlay extents by address alone")
    ap.add_argument("--out", default=DEFAULT_OUT)
    ap.add_argument("--md", action="store_true",
                    help="write the markdown report to stdout as well")
    ap.add_argument("--check", action="store_true",
                    help="fail if any figure regressed below the committed baseline")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--tolerance", type=float, default=0.5,
                    help="percentage points a figure may drop before --check fails")
    ap.add_argument("--quiet", action="store_true",
                    help="drop the per-image rollup and print only the ratchet "
                         "verdict (for the pre-commit hook)")
    args = ap.parse_args()

    # Disc-gated, exactly like the LEGAIA_DISC_BIN tests: both inputs are
    # gitignored, so a checkout without disc data must pass rather than fail.
    if not os.path.isdir(args.funcs) or not os.path.isdir(args.extracted):
        print("[disc-coverage] SKIPPED - no dump corpus and/or no extracted/ tree.")
        print("[disc-coverage] Both are gitignored; this gate only measures locally.")
        return 0

    extents, rejects = read_dump_extents(args.funcs)
    if not extents:
        print("[disc-coverage] SKIPPED - dump corpus present but empty.")
        return 0

    # Optional. Without the CSV every overlay extent stays ambiguous by address,
    # which is the pre-attribution behaviour and still an honest upper bound.
    attrib = read_attribution(args.attribution)

    scus = scus_report(args.extracted, extents)
    overlays, amb_totals = overlay_reports(args.extracted, extents, attrib)
    data = data_report(args.extracted)

    if scus is None and not overlays:
        print("[disc-coverage] SKIPPED - no extractable images found.")
        return 0

    report = render(scus, overlays, amb_totals, data, rejects, bool(attrib))
    os.makedirs(args.out, exist_ok=True)
    md_path = os.path.join(args.out, "disc-coverage.md")
    with open(md_path, "w") as fh:
        fh.write(report)
    if args.md:
        sys.stdout.write(report)

    # Informational rollup. --quiet drops it so the hook shows only the
    # ratchet verdict - including the NOT MEASURED / NOT RATCHETED lines,
    # which are the ones a redirect used to swallow.
    if not args.quiet:
        if scus:
            print("[disc-coverage] SCUS_942.54 code: %.1f%% (%d/%d bytes)" % (
                scus["pct"], scus["covered"], scus["code_denominator"]))
        for r in overlays:
            amb = r.get("ambiguous_pct", 0.0)
            if amb >= 50.0:
                print("[disc-coverage] overlay %-22s not meaningful "
                      "(%.1f%% of its extents are VA-ambiguous)" % (r["name"], amb))
            else:
                print("[disc-coverage] overlay %-22s %.1f%%%s" % (
                    r["name"], r["pct"],
                    "" if amb == 0 else " (<=, %.1f%% VA-ambiguous)" % amb))
        if data:
            print("[disc-coverage] PROT data parsed to a named format: %.1f%% "
                  "(unexplained %.1f%%)" % (data["pct_parsed"], data["pct_unexplained"]))
        print("[disc-coverage] wrote %s" % md_path)

    current = snapshot(scus, overlays, data)
    if args.update_baseline:
        with open(BASELINE, "w") as fh:
            json.dump(current, fh, indent=2, sort_keys=True)
            fh.write("\n")
        print("[disc-coverage] baseline updated: %s" % BASELINE)
        return 0

    if args.check:
        if not os.path.exists(BASELINE):
            # Not a pass. The ratchet has no reference, so it measured nothing
            # this run, and a bare "OK" here would be the exact shape this file
            # exists to prevent: a green line standing in for an absent
            # comparison.
            print("[disc-coverage] NOT RATCHETED - no baseline at %s. Nothing "
                  "was compared. Run --update-baseline once." % BASELINE)
            return 0
        base = json.load(open(BASELINE))
        bad = []
        absent = []
        for section in ("code", "data"):
            for key, was in base.get(section, {}).items():
                now = current.get(section, {}).get(key)
                if now is None:
                    # A missing image is a local extraction gap, not a
                    # regression - but it is also not a pass for that key, and
                    # an image dropping out of the corpus entirely would
                    # otherwise read identically to a clean run. Name it.
                    absent.append("%s/%s (baselined at %.2f%%)"
                                  % (section, key, was))
                    continue
                if now < was - args.tolerance:
                    bad.append("%s/%s: %.2f%% -> %.2f%%" % (section, key, was, now))
        for a in absent:
            print("[disc-coverage] NOT MEASURED THIS RUN: %s - the image is "
                  "absent from this tree, so the ratchet skipped it" % a)
        if bad:
            print("[disc-coverage] REGRESSION:")
            for b in bad:
                print("   " + b)
            print("[disc-coverage] coverage may only go up. If a dump was "
                  "legitimately removed, re-run with --update-baseline and say "
                  "why in the commit message.")
            return 1
        print("[disc-coverage] OK - %d/%d baselined figure(s) compared, none "
              "regressed beyond %.2f pp."
              % (sum(len(base.get(s, {})) for s in ("code", "data")) - len(absent),
                 sum(len(base.get(s, {})) for s in ("code", "data")),
                 args.tolerance))
    return 0


if __name__ == "__main__":
    sys.exit(main())
