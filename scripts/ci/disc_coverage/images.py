"""Coverage per code image: SCUS, every overlay, and PROT format recognition.
"""

from __future__ import annotations

import glob
import json
import os
import struct
import tomllib

import inherited_tail
import slot_b_band

from disc_coverage.attribution import ResidueAmong, owners_exclude, owners_name
from disc_coverage.gaps import (
    band_bytes_in,
    classify_gap,
    data_floor,
    gap_shape,
    merge,
    split_gap,
    split_on_band,
)
from disc_coverage.paths import OVERLAY_MAP
from disc_coverage.thresholds import (
    FILLER_CLASSES,
    PLACEHOLDER_CLASSES,
    UNEXPLAINED_CLASSES,
)


def cover_image(name, image, base_va, span, extents, attrib=None, unambiguous=(),
                tail=None):
    """Coverage of one loaded image. `span` is its byte length.

    `attrib` is the byte-attribution map. Where it places an extent in some
    other image - or in none - the extent is dropped from this image rather
    than counted for it. Pass `None` for an image with no VA aliasing
    (`SCUS_942.54`): the filter must not touch a row that is already exact.

    `unambiguous` is the set of extent keys exactly ONE measured span contains.
    Those need no attribution to belong here - address arithmetic already
    answers it, which is why the attribution sweep does not write a row for
    them - so they count toward the floor as well as toward the upper bound.
    Without that set the floor silently punished an image for every extent
    only its own span reaches, which is the opposite of what the number means.
    """
    lo, hi = base_va, base_va + span
    # The image is measured against its OWN content. Where an inherited tail
    # was found, everything from `tail` up is a longer sibling's code at the
    # same file offset (`inherited_tail.py`), so it leaves the numerator and
    # the denominator together - counting a dump of it while excluding its
    # bytes from the denominator would credit this module for a neighbour's
    # routine. The tail's size is reported in the shape census below so the
    # bytes stay visible rather than vanishing.
    tail_bytes = 0
    if tail is not None and lo <= tail < hi:
        tail_bytes = hi - tail
        hi = tail
    # One `jr ra` scan per image, threaded into every gap test below.
    seg_floor = data_floor(image, base_va)
    mine, floor, dropped = [], [], 0
    # Extents this image counts toward its UPPER bound but not toward its
    # floor, because the byte-attribution CSV has no row for them. They are the
    # difference between the two numbers, and when the CSV lags the corpus they
    # are also the whole of an apparent floor regression - see `--check`.
    unattributed = []
    for a, b in extents:
        if not lo <= a < hi:
            continue
        owners = (attrib or {}).get((a, b), "residue")
        if owners_exclude(owners, name):
            dropped += 1
            continue
        mine.append((a, min(b, hi)))
        # The floor takes the extents the bytes NAME for this image, plus the
        # ones no other measured span reaches. An unambiguous image
        # (`attrib is None`, i.e. SCUS) has nothing to attribute, so its floor
        # is its numerator.
        if (attrib is None or owners_name(owners, name)
                or (a, b) in unambiguous):
            floor.append((a, min(b, hi)))
        else:
            unattributed.append((a, b))
    merged = merge(mine)
    covered = sum(b - a for a, b in merged)

    def gaps_of(intervals):
        out, prev = [], lo
        for a, b in intervals:
            if a > prev:
                out.append((prev, a))
            prev = b
        if prev < hi:
            out.append((prev, hi))
        return out

    gaps = gaps_of(merged)

    # The DENOMINATOR keeps the whole-gap classification. Its calibration is
    # controlled (see docs/tooling/disc-coverage.md) and the ratchet is written
    # against it; re-classifying in windows moves `SCUS_942.54`'s code
    # denominator by ~28 KB in the other direction, which is a separate claim
    # about that image's rodata and not one this instrument can settle. The
    # window split is applied only where granularity is what makes the output
    # usable - the worklist below.
    code_gap = data_gap = 0
    code_gaps = []
    shapes = {}
    # The shape census covers EVERY gap, not only the ones the denominator
    # counts. `padding` and `mostly_padding` no longer reach `code_gap` (see
    # `classify_gap`), and dropping them from the census along with the
    # denominator would hide exactly the bytes the change is about - a reader
    # could no longer check that a shrinking code gap went to padding rather
    # than to a dump. A gap the statistic rejects that carries no structural
    # shape of its own is `data`.
    for a, b in gaps:
        is_code = classify_gap(image, base_va, a, b, seg_floor, tail=tail)
        shape = gap_shape(image, base_va, a, b, seg_floor, tail=tail)
        if not is_code and shape == "code":
            shape = "data"
        # The slot-B spawn records inside this gap leave the denominator with
        # their own shape, and the REST of the gap keeps the verdict the whole
        # gap earned. Cutting the gap and re-classifying the pieces instead
        # would move bytes that have nothing to do with the band: a piece is a
        # different window than the gap, and both `in_data_segment` and the
        # tiny-gap fiat answer per window.
        band = band_bytes_in(image, base_va, a, b)
        if band:
            n, nb = shapes.get("spawn_record_band", (0, 0))
            shapes["spawn_record_band"] = (n + 1, nb + band)
            data_gap += band
        rest = (b - a) - band
        if rest:
            n, nb = shapes.get(shape, (0, 0))
            shapes[shape] = (n + 1, nb + rest)
        # The denominator counts a gap as un-dumped CODE only where the shape
        # census also calls it code. Both verdicts are already computed per
        # gap; counting `is_code` alone let every shape the statistic cannot
        # see - `no_exit`, `no_boundary`, `constant_table`, `return_tail`,
        # `psyq_lib_stamp`, `bios_thunk_slot` - into the code denominator under
        # a name that says it is not code, which is what made the table and the
        # worklist disagree about the same bytes. `classify_gap` had already
        # been taught two of the shapes (`padding`, `mostly_padding`); this is
        # the rest of that move, made once rather than shape by shape.
        if is_code and shape == "code":
            code_gap += rest
            code_gaps.append((a, b))
        else:
            data_gap += rest
    if tail_bytes:
        n, nb = shapes.get("inherited_tail", (0, 0))
        shapes["inherited_tail"] = (n + 1, nb + tail_bytes)

    # The worklist is cut against the FLOOR, not against `merged`. `merged`
    # credits every extent the byte attribution could not place to each span
    # containing it, so on a row whose dumps are mostly some sibling overlay's
    # it papers over the image's real gaps with another image's dumps - the
    # summon-stager rows would report almost no work while nothing in the corpus
    # is attributable to them at all. Each run therefore carries whether an
    # unattributed extent overlaps it: `ambiguous` runs are un-dumped unless one
    # of the ambiguous extents at that VA turns out to be this image's, and the
    # rest are un-dumped outright.
    floor_merged = merge(floor)
    runs = []
    for a, b in gaps_of(floor_merged):
        # Split the run where the upper-bound crediting changes, so `ambiguous`
        # is a property of each piece rather than of the whole gap. A single
        # flag over a 62 KB gap that one 40-byte residue extent touches would
        # read as "all of this is ambiguous" and bury the part nothing covers.
        cuts = [a, b]
        for x, y in merged:
            if x < b and a < y:
                cuts += [max(a, x), min(b, y)]
        cuts = sorted(set(cuts))
        for u, v in zip(cuts, cuts[1:]):
            amb = any(x < v and u < y for x, y in merged)
            for p, q, is_code in split_gap(image, base_va, u, v, seg_floor,
                                           tail=tail):
                # `is_code` already carries the gap-level data-segment verdict
                # (`split_gap`), so the shape must not re-run it per window.
                shape = (gap_shape(image, base_va, p, q, seg_floor, data_seg=False)
                         if is_code
                         else ("inherited_tail" if tail is not None and p >= tail
                               else "data"))
                # Label the spawn-record pieces without re-classifying the run.
                for x, y, in_band in split_on_band(image, base_va, p, q):
                    runs.append((x, y, "spawn_record_band" if in_band else shape,
                                 amb))

    denom = covered + code_gap
    # Lower bound on the same image. `mine` credits an extent the byte
    # attribution could not place to EVERY span containing it, which is the
    # upper bound the row is labelled with. `floor` credits only the extents
    # the bytes actually name for this image (plus the ones no other measured
    # span contains, which need no attribution). The truth is between the two,
    # and a row whose upper bound is withheld as "not meaningful" still has a
    # defensible floor - which is the difference between an unmeasured overlay
    # and one that is merely imprecise.
    floor_cov = sum(b - a for a, b in floor_merged)
    return {
        "name": name,
        "base_va": base_va,
        "span": span,
        "dumps": len(mine),
        "dumps_attributed": len(floor),
        "attributed_out": dropped,
        "covered": covered,
        "covered_attributed": floor_cov,
        "code_gap": code_gap,
        "data_gap": data_gap,
        "code_denominator": denom,
        "pct": (100.0 * covered / denom) if denom else 0.0,
        "pct_floor": (100.0 * floor_cov / denom) if denom else 0.0,
        "gap_shapes": shapes,
        "inherited_tail_bytes": tail_bytes,
        "runs": runs,
        # The extents the floor credits, i.e. the dumps the bytes place in
        # THIS image. Their entries are the image's function list for the
        # per-image port table (`update-progress-metrics.py`), which therefore
        # inherits this function's placement rule instead of restating it.
        "floor_extents": sorted(set(floor)),
        "unattributed": sorted(set(unattributed)),
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
    # The measured spans have to be known BEFORE the first row is covered: an
    # extent's floor membership depends on how many spans contain it, which is
    # a property of the whole map rather than of the row being measured.
    for row in rows:
        base, label = row.get("base_va"), row.get("label")
        span = row.get("content_bytes") or row.get("clean_copy_bytes")
        if not base or not span or not label:
            continue
        candidates = sorted(glob.glob(
            os.path.join(extracted, "overlays", "overlay_%s_*.bin" % label)))
        if not candidates:
            continue
        spans.append((base, base + min(span, os.path.getsize(candidates[0])), label))
    unambiguous = {k for k in set(extents)
                   if sum(1 for lo, hi, _ in spans if lo <= k[0] < hi) == 1}

    # Where each image stops being its own content. The packer wrote every
    # overlay into a buffer it did not clear, so a module shorter than the
    # buffer flushes its own bytes and then the previous, longer module's
    # residue - inside `content_bytes`, at the file offsets that module
    # occupies. Counting that residue puts another module's code in this
    # module's denominator, and no dump of this module can ever close it.
    # `inherited_tail.py` states the rule and why it is asymmetric.
    tail_inputs = []
    for row in rows:
        base, span, label = (row.get("base_va"), row.get("content_bytes"),
                             row.get("label"))
        if not base or not span or not label:
            continue
        candidates = sorted(glob.glob(
            os.path.join(extracted, "overlays", "overlay_%s_*.bin" % label)))
        if not candidates:
            continue
        with open(candidates[0], "rb") as fh:
            tail_inputs.append((row.get("prot_index"), base, fh.read()[:span]))
    labels_by_index = {row.get("prot_index"): row.get("label") for row in rows}
    # An equal-extent sibling can be the donor too - `content_bytes` is the
    # sector extent, not the module's content - but only where the bytes say
    # which of the two owns the shared suffix. `own_ends` is that measurement:
    # for a slot-B module the top of its spawn-record chain, otherwise the end
    # of its frame-matched code partition.
    # Iterated rather than measured once. `content_end` walks the image, and
    # the cut is what makes that walk stop at this module's own content, so the
    # first estimate overshoots on the images whose record chain runs into the
    # residue. `tail_starts_fixpoint` re-measures against each round's cut.
    # Then the packer-buffer leg (`inherited_tail.tail_cuts`): the buffer is
    # one buffer for the whole of PROT.DAT in TOC order, so it also cuts the
    # images whose donor is not an overlay (PROT 0898 / 0895, donor 0894) and
    # the one the sibling gate stops short on (PROT 0901). The byte account
    # applies the same rule (`legaia_disc_audit::inherited_tail::tails_in`).
    by_index = inherited_tail.tail_cuts(
        [t for t in tail_inputs if t[0] is not None],
        lambda _key, base, data: slot_b_band.content_end(data, base),
        prot_dir=os.path.join(extracted, "PROT"))
    tails = {labels_by_index[k]: (off, labels_by_index.get(d, "prot_%04d" % d))
             for k, (off, d) in by_index.items()}

    spans = []
    for row in rows:
        base = row.get("base_va")
        # `content_bytes` is the overlay's OWN content length - its PROT entry's
        # sector extent, which is exactly the slice the runtime loader streams
        # into the overlay window. NOT `clean_copy_bytes`, which answers how
        # much of a row a RAM capture has byte-verified: on PROT 0899 that is
        # 0x_f174 bytes shorter than the image, and measuring the menu overlay
        # against it silently drops a third of the overlay from the
        # denominator AND from the worklist. `clean_copy_bytes` stays as the
        # fallback so a hand-written map with only the old key still measures.
        span = row.get("content_bytes") or row.get("clean_copy_bytes")
        label = row.get("label")
        if not base or not span or not label:
            # A row with no cited own-content length has no honest denominator,
            # so it is skipped rather than guessed at.
            continue
        candidates = sorted(glob.glob(
            os.path.join(extracted, "overlays", "overlay_%s_*.bin" % label)))
        if not candidates:
            continue
        image = open(candidates[0], "rb").read()[:span]
        if len(image) < span:
            span = len(image)
        cut = tails.get(label)
        tail_va = base + cut[0] if cut and cut[0] < span else None
        row = cover_image(label, image, base, span, extents, attrib=attrib,
                          unambiguous=unambiguous, tail=tail_va)
        if tail_va is not None:
            row["inherited_tail"] = (span - cut[0], cut[1])
        row["_image_span"] = (base, base + span)
        # Kept so `--check` can re-measure the row against the corpus the
        # attribution CSV knows about, without re-reading every overlay.
        row["_image"] = image
        out.append(row)
        spans.append((base, base + span, label))

    # Overlays alias in VA space (nineteen share 0x801CE818 and thirteen share
    # 0x801F69D8, and their spans nest), so an extent can fall inside more than
    # one image's span and be counted by each. That is a real ambiguity, not something to paper
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
    resolved = sum(1 for k in ambiguous
                   if k in attrib and not isinstance(attrib[k], ResidueAmong))
    totals = (len(ambiguous), resolved, len(ambiguous) - resolved)

    # Per-image share of this image's extents that the bytes could not place.
    # An extent the bytes assign elsewhere is no longer ambiguous *for this
    # image* - it is simply not this image's - so it leaves the numerator and
    # the denominator both. A row whose remaining share is high has no
    # defensible number at all, and the table says so ON the row rather than in
    # prose underneath it.
    for row in out:
        lo, hi = row["_image_span"]
        mine = [k for k in distinct if lo <= k[0] < hi]
        resid = dropped = 0
        for k in mine:
            if sum(1 for l2, h2, _ in spans if l2 <= k[0] < h2) <= 1:
                continue  # unambiguous by address; attribution has nothing to do
            owners = attrib.get(k, "residue")
            if owners_exclude(owners, row["name"]):
                dropped += 1
            elif owners == "residue" or isinstance(owners, ResidueAmong):
                resid += 1
        kept = len(mine) - dropped
        row["ambiguous"] = resid
        row["ambiguous_pct"] = (100.0 * resid / kept) if kept else 0.0
        # The share of this image's CREDITED BYTES that rests on an extent the
        # attribution could not place - which is the same question the extent
        # count above asks, in the unit the coverage figure is actually stated
        # in. The two answers are not close: the corpus is full of 4-to-36-byte
        # Ghidra fragments (a `halt_baddata` stub over a data word, a tail the
        # dumper resolved as its own body), and each of those counts once in
        # `ambiguous_pct` against a 6-KB module that counts once as well. On
        # the slot-B modules that reads as ~90% ambiguity over ~0.2% of the
        # bytes. `covered - covered_attributed` is exactly the byte span the
        # upper bound rests on and the floor does not, so it is the honest
        # discount on the upper bound, and it still reads 100% for the two
        # images (`summon_mushura`, `cast_earthquake`) that really have no
        # attributed byte at all.
        cov = row["covered"]
        row["ambiguous_bytes"] = cov - row["covered_attributed"]
        row["ambiguous_bytes_pct"] = (
            100.0 * row["ambiguous_bytes"] / cov) if cov else 0.0
    return out, totals, unambiguous


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
    filler = sum(v[1] for k, v in by.items() if k in FILLER_CLASSES)
    content = total - filler
    return {
        "entries": sum(v[0] for v in by.values()),
        "total": total,
        "parsed": parsed,
        "placeholder": placeholder,
        "unexplained": unexplained,
        "pct_parsed": 100.0 * parsed / total if total else 0.0,
        "pct_unexplained": 100.0 * unexplained / total if total else 0.0,
        # The same numerator over the disc's CONTENT: every byte except
        # structurally confirmed filler. The headline figure - filler is
        # documented but carries nothing to parse, so it belongs in neither
        # side of the ratio.
        "filler": filler,
        "filler_entries": sum(v[0] for k, v in by.items() if k in FILLER_CLASSES),
        "content": content,
        "pct_content_parsed": 100.0 * parsed / content if content else 0.0,
        "by_class": sorted(
            ([k, v[0], v[1]] for k, v in by.items()), key=lambda r: -r[2]),
    }
