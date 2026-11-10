"""A retail-kernel differential as provenance evidence.

Every other input to this checker ranks suspicion. A `differential!`
declaration in `crates/retail-exec/tests/` is a different kind of claim: when
the test passes with the disc present, the routine at that entry was run off
the named image and a Rust kernel computed what it wrote (the declaration's
guard fails the test otherwise - see `differential_map.py`). So a tag whose
address a differential runs is *verified*, not merely unflagged.

The credit is per address. The declaration proves which routine the test ran
and the comparison proves the kernel it calls; it does not prove that every
tag site carrying the address sits on that kernel. A tag that names an image
(`overlay_<label>_<NNNN>_<addr>`) is credited only by a declaration of that
PROT entry, since the overlay band holds different code at one VA.
"""

from __future__ import annotations

import re

import differential_map
from port_provenance.tags import Tag

# The signals a differential settles: both read the tag against the dump
# corpus and rank how unlikely the pairing looks. `non-entry` is not here - it
# is a contradiction between two committed files, which a test run does not
# resolve - and the doc-side signals are about prose, not tags.
SETTLED_SIGNALS = frozenset({"module-orphan", "absent-citation"})


def declared() -> dict[str, dict[int | None, list[str]]]:
    """`{addr: {image: [test paths]}}`, image `None` for SCUS."""
    out: dict[str, dict[int | None, list[str]]] = {}
    for t in differential_map.scan().tests:
        for img, va in t.routines:
            out.setdefault(f"{va:08x}", {}).setdefault(img, []).append(t.path)
    return out


def _tag_entry(image: str | None) -> int | None:
    if not image:
        return None
    m = re.search(r"_(\d{4})$", image)
    return int(m.group(1)) if m else None


def verifying_tests(tag: Tag, decl: dict[str, dict[int | None, list[str]]]) -> list[str]:
    """The tests that verify this tag site, empty when none does."""
    by_img = decl.get(tag.addr)
    if not by_img:
        return []
    want = _tag_entry(tag.image)
    if want is not None:
        return sorted(by_img.get(want, []))
    return sorted({p for paths in by_img.values() for p in paths})
