"""`--check`: the snapshot compared against the baseline, and the attribution
lag that explains a fall.
"""

from __future__ import annotations

from disc_coverage.images import cover_image


def attribution_lag(row, extents, attrib, unambiguous, was, tolerance):
    """Is this image's floor drop an attribution lag rather than lost coverage?

    A `code_floor` figure is `floor_bytes / (covered_bytes + code_gap_bytes)`.
    A dump that lands with no row in `dump-extent-attribution.csv` is residue:
    it joins the image's UPPER bound - and so the denominator - while the floor,
    which only counts extents the bytes name for this image, does not move. The
    ratio therefore falls, and the ratchet reads a *new dump* as lost coverage.
    That is backwards, and it is not a worktree artifact: the committed CSV lags
    the local corpus in the main checkout too, because the corpus is gitignored
    and the CSV is not regenerated per dump.

    So the test is direct rather than inferred: re-measure the image over the
    corpus **the CSV knows about** - every extent minus this image's
    unattributed ones - and see whether the floor clears its baseline there. If
    it does, the drop is entirely the lag, and what the reader needs is the list
    of dumps to attribute, not a coverage failure. Returns `None` when the drop
    survives that removal, i.e. when it is a real regression.
    """
    missing = set(row.get("unattributed") or ())
    if not missing:
        return None
    kept = [k for k in extents if k not in missing]
    base_va, end_va = row["_image_span"]
    alt = cover_image(row["name"], row["_image"], base_va, end_va - base_va,
                      kept, attrib=attrib, unambiguous=unambiguous)
    if alt["pct_floor"] < was - tolerance:
        return None
    return {
        "extents": sorted(missing),
        "bytes": sum(b - a for a, b in missing),
        "pct_floor_without": alt["pct_floor"],
    }


def snapshot(scus, overlays, data):
    out = {"code": {}, "code_floor": {}, "data": {}}
    for r in ([scus] if scus else []) + overlays:
        # `code` only ratchets figures that mean something as an UPPER bound. A
        # row most of whose covered BYTES are claimable by a sibling overlay
        # moves with dump attribution rather than with real coverage, so
        # baselining its upper bound would produce failures nobody can act on.
        # The gate reads the byte share rather than the extent count for the
        # reason the report gives: an extent count weighs a 4-byte Ghidra stub
        # the same as a 6 KB module.
        if r.get("ambiguous_bytes_pct", 0.0) < 50.0:
            out["code"][r["name"]] = round(r["pct"], 2)
        # `code_floor` ratchets every image, including the ones with no
        # defensible upper bound. The floor counts only extents the bytes NAME
        # for the image, so it is well defined for every row - which is the
        # point: an overlay nothing is attributed to reads 0.0% here rather than
        # dropping out of the baseline and being indistinguishable from one that
        # is fully covered.
        out["code_floor"][r["name"]] = round(r["pct_floor"], 2)
    if data:
        out["data"]["pct_parsed"] = round(data["pct_parsed"], 2)
        out["data"]["pct_content_parsed"] = round(data["pct_content_parsed"], 2)
    return out
