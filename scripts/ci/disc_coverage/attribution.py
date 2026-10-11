"""The dump-extent attribution table: which image a dump's bytes belong to,
and who may be credited for them.
"""

from __future__ import annotations

import csv
import os

from disc_coverage.paths import ATTRIBUTION

# Extent classes in `dump-extent-attribution.csv` whose verdict is that the
# bytes belong to no mapped image at all - a mis-based print, a gapped stream,
# or a region that does not disassemble. Crediting them to the image whose span
# happens to contain the printed VA is exactly the fiction this filter removes.
# `zero_window` joins them for a reason of its own: `nop` encodes
# `0x00000000`, so an all-`nop` window reproduces inside any image's zero fill
# at any base, at any VA. Such an extent has been credited to an image whose
# own content is shorter than the extent, and read as byte-identical across two
# images that agreed about nothing but being empty.
#
# `unresolved` and `no_holder` join them on evidence of their own: the sweep
# compared every measured image's OWN content with the dump's window at the
# printed VA and none of them reproduces it. That is a mismatch at a fixed
# offset, which needs no window length to be believed - the length floors guard
# a positive match against coincidence. Counted as residue instead, such an
# extent joined the upper bound of every span containing it, and since the code
# denominator is `covered + code_gap` it inflated each of those images'
# denominators with bytes their own content says are something else. On the
# slot-A band it was one batch of mis-based field-overlay prints landing in the
# data and zero fill of the FMV player, the boot image and the dev modules. An
# extent of an overlay with no extracted image lands here too, and is credited
# to that image once an extraction gives the sweep something to match.
#
# `no_disassembly` is the same law at its limit: a dump that carries decompiled
# C and no instruction stream has no bytes at its entry at all, so nothing
# corroborates the base its header prints (the `zero_window_head` rule below,
# with zero words of evidence instead of zero-valued ones).
CREDIT_NOBODY = {"misbased", "data", "gapped", "zero_window", "unresolved",
                 "no_holder", "no_disassembly"}


class ResidueAmong(frozenset):
    """A residue extent the bytes narrowed to a candidate set but could not
    sign. A `short` window (below the at-VA floor) carries the images whose own
    content reproduces it; every other image's content disagrees at that VA,
    so the extent stays ambiguous for the candidates only."""


def owners_exclude(owners, name):
    """Does an attribution verdict take this extent away from image `name`?"""
    if owners == "residue":
        return False
    if isinstance(owners, ResidueAmong):
        return name not in owners
    return owners is None or name not in owners


def owners_name(owners, name):
    """Does an attribution verdict NAME image `name` (floor credit)?"""
    return (owners != "residue" and not isinstance(owners, ResidueAmong)
            and owners is not None and name in owners)
# Classes that name the owning image(s) by bytes. `identical` names several
# because they hold byte-identical code there, and each of them really does
# contain those bytes, so each is credited. `divergent` names several for the
# opposite reason and is credited for the same one: it is several DUMPS at one
# extent key, each of which the bytes place in a different image, so every named
# image really does have that byte range dumped - from its own dump. Reading it
# as residue withheld `battle_action`'s largest un-credited run
# (`0x801DABA4..0x801DB124`) from its own floor and kept it on the worklist.
CREDIT_NAMED = {"unique", "identical", "divergent", "resolved_by_table"}
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
            elif klass == "short":
                cands = {n.split("(")[0]
                         for n in (row.get("image") or "").split("|")
                         if n and n != "-"}
                # A `short` row with no candidates is a window with no tokens
                # to compare: it stays full residue.
                if cands:
                    out[(entry, entry + nbytes)] = ResidueAmong(cands)
    return out
