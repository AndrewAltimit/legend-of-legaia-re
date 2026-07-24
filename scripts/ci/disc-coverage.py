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

  CODE  - byte-exact. Every Ghidra dump header carries `entry=` and
          `size=N bytes`, so the dumped functions are real byte intervals.
          Merge them, subtract from an image's extent, and every remaining byte
          is genuinely un-dumped. Gaps are then classified code-vs-data so the
          rodata an executable carries inside its text segment does not inflate
          the denominator.

  DATA  - format RECOGNITION, one level coarser. `asset categorize` says which
          format class each PROT entry is. Knowing an entry is a
          `scene_vab_stream` is not the same as accounting for every byte
          inside it, and no parser currently reports consumed-vs-unconsumed
          bytes. Treat the data figure as an upper bound.

Disc-gated, like the rest of the repo: with no `extracted/` tree and no dump
corpus this exits 0 and reports SKIPPED, so CI passes without disc data. Both
inputs are gitignored, so this only produces numbers on a developer's machine.

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
import glob
import json
import os
import re
import struct
import sys
import tomllib

# scripts/ci/disc-coverage.py -> repo root is three levels up, matching
# port-catalog.py's `Path(__file__).resolve().parent.parent.parent`.
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

DEFAULT_FUNCS = os.path.join(REPO, "ghidra", "scripts", "funcs")
DEFAULT_EXTRACTED = os.path.join(REPO, "extracted")
DEFAULT_OUT = os.path.join(REPO, "target", "disc-coverage")
OVERLAY_MAP = os.path.join(REPO, "crates", "asset", "data", "static-overlays.toml")
BASELINE = os.path.join(REPO, "scripts", "ci", "disc-coverage-baseline.json")

HDR_RE = re.compile(r"^==\s+(\S+)\s+([0-9a-fA-F]{8})\s+\(entry=([0-9a-fA-F]{8})\)")
SIZE_RE = re.compile(r"^size=(\d+) bytes,\s*(\d+) instructions")

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
    """Every dump's (entry_va, end_va). Returns [] when the corpus is absent."""
    out = []
    unparsed = 0
    for path in sorted(glob.glob(os.path.join(funcs_dir, "*.txt"))):
        try:
            with open(path, errors="replace") as fh:
                first, second = fh.readline(), fh.readline()
        except OSError:
            continue
        m, s = HDR_RE.match(first.strip()), SIZE_RE.match(second.strip())
        if not m or not s:
            # Dumps that report `0 instructions` and carry only decompiled C
            # land here. They are not evidence of coverage and are excluded.
            unparsed += 1
            continue
        nbytes = int(s.group(1))
        if nbytes <= 0:
            unparsed += 1
            continue
        entry = int(m.group(3), 16)
        out.append((entry, entry + nbytes))
    return out, unparsed


def merge(intervals):
    merged = []
    for a, b in sorted(intervals):
        if merged and a <= merged[-1][1]:
            merged[-1][1] = max(merged[-1][1], b)
        else:
            merged.append([a, b])
    return merged


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


def cover_image(name, image, base_va, span, extents):
    """Coverage of one loaded image. `span` is its byte length."""
    lo, hi = base_va, base_va + span
    mine = [(a, min(b, hi)) for a, b in extents if lo <= a < hi]
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
    for a, b in gaps:
        if classify_gap(image, base_va, a, b):
            code_gap += b - a
            code_gaps.append((a, b))
        else:
            data_gap += b - a

    denom = covered + code_gap
    return {
        "name": name,
        "base_va": base_va,
        "span": span,
        "dumps": len(mine),
        "covered": covered,
        "code_gap": code_gap,
        "data_gap": data_gap,
        "code_denominator": denom,
        "pct": (100.0 * covered / denom) if denom else 0.0,
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


def overlay_reports(extracted, extents):
    if not os.path.exists(OVERLAY_MAP):
        return [], 0
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
        row = cover_image(label, image, base, span, extents)
        row["_image_span"] = (base, base + span)
        out.append(row)
        spans.append((base, base + span, label))

    # Overlays alias in VA space (several share 0x801CE818), so a dump can fall
    # inside more than one image's span and be counted by each. That is a real
    # ambiguity, not something to paper over: quantify it and let the reader
    # discount accordingly.
    ambiguous = 0
    for a, _b in extents:
        if sum(1 for lo, hi, _ in spans if lo <= a < hi) > 1:
            ambiguous += 1

    # Per-image share of attributed dumps that could equally belong to another
    # mapped overlay. A row whose share is high has no defensible number at all,
    # and the table says so on the row rather than in prose underneath it.
    for row in out:
        lo, hi = row.pop("_image_span")
        mine = [a for a, _ in extents if lo <= a < hi]
        amb = sum(1 for a in mine
                  if sum(1 for l2, h2, _ in spans if l2 <= a < h2) > 1)
        row["ambiguous"] = amb
        row["ambiguous_pct"] = (100.0 * amb / len(mine)) if mine else 0.0
    return out, ambiguous


def data_report(extracted):
    cat = os.path.join(extracted, "PROT", "categorize.json")
    if not os.path.exists(cat):
        return None
    per_file = json.load(open(cat)).get("per_file", [])
    if not per_file:
        return None
    by = {}
    for e in per_file:
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


def render(scus, overlays, ambiguous, data, unparsed):
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
            cover, ambcell = "not meaningful", "%.0f%%" % amb
        elif amb > 0.0:
            cover, ambcell = "<= %.1f%%" % r["pct"], "%.0f%%" % amb
        else:
            cover, ambcell = "**%.1f%%**" % r["pct"], "0%"
        add("| `%s` | `0x%08X` | %d | %d | %d | %d | %d | %d | %s | %s |" % (
            r["name"], r["base_va"], r["span"], r["dumps"], r["covered"],
            r["code_gap"], r["data_gap"], r["code_denominator"], cover, ambcell))
    add("")
    add("**VA-ambiguous** is the share of an image's attributed dumps whose entry "
        "address also lands inside another mapped overlay's span. At 50% or more "
        "the coverage figure is not reported, because address attribution alone "
        "cannot support one.")
    add("")
    if scus:
        add("`SCUS_942.54` is the only image here with an unambiguous answer: it "
            "is a single load image at a fixed base with no VA aliasing.")
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
    if overlays:
        add("### Overlay caveat")
        add("")
        add("Overlay images alias in VA space - several share base `0x801CE818` - "
            "so a dump whose entry lands in that band cannot be attributed to one "
            "image by address alone. Overlay rows are therefore an **upper "
            "bound**: a dump counted for one image may belong to another. "
            f"**{ambiguous}** dump extents fall inside more than one mapped "
            "overlay span. Resolving them needs byte-level attribution against "
            "the extracted images - see "
            "[`dump-corpus-integrity.md`](../../docs/tooling/dump-corpus-integrity.md) "
            "and [`phantom-print-index.md`](../../docs/tooling/phantom-print-index.md).")
        add("")
    add("`%d` dump file(s) carried no parseable `size=` header (typically the "
        "ones that report `0 instructions` and hold only decompiled C). They are "
        "excluded - such a dump is not evidence of coverage." % unparsed)
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
    ap.add_argument("--out", default=DEFAULT_OUT)
    ap.add_argument("--md", action="store_true",
                    help="write the markdown report to stdout as well")
    ap.add_argument("--check", action="store_true",
                    help="fail if any figure regressed below the committed baseline")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--tolerance", type=float, default=0.5,
                    help="percentage points a figure may drop before --check fails")
    args = ap.parse_args()

    # Disc-gated, exactly like the LEGAIA_DISC_BIN tests: both inputs are
    # gitignored, so a checkout without disc data must pass rather than fail.
    if not os.path.isdir(args.funcs) or not os.path.isdir(args.extracted):
        print("[disc-coverage] SKIPPED - no dump corpus and/or no extracted/ tree.")
        print("[disc-coverage] Both are gitignored; this gate only measures locally.")
        return 0

    extents, unparsed = read_dump_extents(args.funcs)
    if not extents:
        print("[disc-coverage] SKIPPED - dump corpus present but empty.")
        return 0

    scus = scus_report(args.extracted, extents)
    overlays, ambiguous = overlay_reports(args.extracted, extents)
    data = data_report(args.extracted)

    if scus is None and not overlays:
        print("[disc-coverage] SKIPPED - no extractable images found.")
        return 0

    report = render(scus, overlays, ambiguous, data, unparsed)
    os.makedirs(args.out, exist_ok=True)
    md_path = os.path.join(args.out, "disc-coverage.md")
    with open(md_path, "w") as fh:
        fh.write(report)
    if args.md:
        sys.stdout.write(report)

    if scus:
        print("[disc-coverage] SCUS_942.54 code: %.1f%% (%d/%d bytes)" % (
            scus["pct"], scus["covered"], scus["code_denominator"]))
    for r in overlays:
        amb = r.get("ambiguous_pct", 0.0)
        if amb >= 50.0:
            print("[disc-coverage] overlay %-22s not meaningful "
                  "(%.0f%% of its dumps are VA-ambiguous)" % (r["name"], amb))
        else:
            print("[disc-coverage] overlay %-22s %.1f%%%s" % (
                r["name"], r["pct"],
                "" if amb == 0 else " (<=, %.0f%% VA-ambiguous)" % amb))
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
            print("[disc-coverage] no baseline yet; run --update-baseline once.")
            return 0
        base = json.load(open(BASELINE))
        bad = []
        for section in ("code", "data"):
            for key, was in base.get(section, {}).items():
                now = current.get(section, {}).get(key)
                if now is None:
                    # A missing image is a local extraction gap, not a regression.
                    continue
                if now < was - args.tolerance:
                    bad.append("%s/%s: %.2f%% -> %.2f%%" % (section, key, was, now))
        if bad:
            print("[disc-coverage] REGRESSION:")
            for b in bad:
                print("   " + b)
            print("[disc-coverage] coverage may only go up. If a dump was "
                  "legitimately removed, re-run with --update-baseline and say "
                  "why in the commit message.")
            return 1
        print("[disc-coverage] OK - no figure regressed beyond %.2f pp." % args.tolerance)
    return 0


if __name__ == "__main__":
    sys.exit(main())
