"""Signal `non-entry`: a tag whose address is not a function entry in the
image it names.
"""

from __future__ import annotations

import csv
from collections import defaultdict
from pathlib import Path

try:  # py311+
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - py<3.11
    tomllib = None  # type: ignore[assignment]

from port_provenance.call_sites import images
from port_provenance.findings import Finding
from port_provenance.rows import IMAGE_ENTRY_ACCEPTED
from port_provenance.tags import Tag
from port_provenance.thresholds import ATTRIBUTION_CSV, OVERLAY_BASE

# `non-entry`: the port-catalog ignore list files an address under a section
# whose whole claim is "no routine starts here" - an interior word, a shared
# tail, a Ghidra label promoted to a fake FUN_, a misbased print, data - so a
# `PORT:` tag on that address and the ignore row contradict each other, and
# one of them is wrong. Two cheaper forms of the same claim need no ignore row:
# an overlay-band VA below the lowest slot-A base (`0x801CE818`), which no
# overlay image can contain, and an address every one of whose
# `dump-extent-attribution.csv` rows is `misbased`.
IGNORE_TOML = Path(__file__).resolve().parent.parent.parent / "port-catalog-ignore.toml"
NOT_A_FUNCTION_SECTIONS = frozenset({
    "worklist_interior",
    "worklist_shared_tail",
    "worklist_phantom",
    "worklist_data",
    "worklist_misbased_print",
    "worklist_va_aliased",
    "ghidra_phantoms",
    "field_vm_labels",
    "overlay_epilogue_labels",
    "worklist_duplicate",
})
SLOT_A_FLOOR = 0x801CE818


def _is_prologue(w: int | None) -> bool:
    """`addiu sp, sp, -N` - the frame allocation that opens a non-leaf routine."""
    return w is not None and w >> 16 == 0x27BD and w & 0x8000 != 0


def image_names_entry(image: str | None, va: int) -> bool:
    """The tag's own image opens a routine at `va`.

    Every verdict the `non-entry` test reads is about a **bare** VA, and several
    slot-B images (and the slot-A siblings) hold different code at one VA. A
    claim written `overlay_<label>_<NNNN>_<addr>` names which of them it ports,
    so the bare-VA verdict is not a contradiction of it when that image, read
    at the base the static-overlay map records, has a frame-allocating prologue
    at the address. A leaf routine (no frame) is not accepted this way; it
    stays a finding for a reader to settle.
    """
    if not image:
        return False
    for img in images():
        if img.label.lower() == image:
            return _is_prologue(img.word(va))
    return False


def find_non_entry_tags(tags: list[Tag]) -> list[Finding]:
    """PORT-tagged addresses the rest of the repo says are not function entries.

    The shape this catches is the one a tag audit found most often among
    outright wrong addresses: a phantom VA (a routine printed `0xE818` low),
    a misbased print of an arm inside another routine, an interior prologue
    word cited instead of the entry. Each already had a verdict somewhere -
    an ignore row or an attribution row - that the tag never met.

    A `worklist_va_aliased` row can be a true contradiction or a benign one
    (distinct code at one VA in several images, the tag naming one of them);
    the row prints the ignore reason so the reader can tell which.
    """
    ignored: dict[str, tuple[str, str]] = {}
    if tomllib is not None and IGNORE_TOML.is_file():
        try:
            with IGNORE_TOML.open("rb") as fh:
                doc = tomllib.load(fh)
        except (OSError, ValueError):
            doc = {}
        for sec, rows in doc.items():
            if sec in NOT_A_FUNCTION_SECTIONS and isinstance(rows, dict):
                for k, v in rows.items():
                    ignored[str(k).lower()] = (sec, str(v))
    classes: dict[str, set[str]] = defaultdict(set)
    if ATTRIBUTION_CSV.is_file():
        try:
            with ATTRIBUTION_CSV.open(newline="") as fh:
                for row in csv.DictReader(fh):
                    entry = (row.get("entry") or "").strip().lower()
                    classes[entry].add((row.get("class") or "").strip())
        except OSError:
            pass
    sites: dict[str, list[Tag]] = defaultdict(list)
    for t in tags:
        sites[t.addr].append(t)
    out: list[Finding] = []
    for addr, ts in sorted(sites.items()):
        why: list[str] = []
        va = int(addr, 16)
        if addr in ignored:
            sec, reason = ignored[addr]
            why.append(f"port-catalog-ignore.toml [{sec}]: {reason[:200]}")
        if OVERLAY_BASE <= va < SLOT_A_FLOOR:
            why.append(
                f"0x{va:08X} is below 0x{SLOT_A_FLOOR:08X}, the lowest slot-A "
                "base, so no overlay image can hold it (a phantom print)"
            )
        if classes.get(addr) == {"misbased"}:
            why.append(
                "dump-extent-attribution.csv: every row for this VA is "
                "`misbased` - no image holds these bytes here"
            )
        if not why:
            continue
        # Per tag line: accepted when a claim on that line names an image whose
        # bytes open a routine here. A file is clear when all its lines are.
        accepted_lines = {
            (t.file, t.line) for t in ts if image_names_entry(t.image, va)
        }
        for t in ts:
            if (t.file, t.line) in accepted_lines:
                img = next(
                    u.image for u in ts
                    if (u.file, u.line) == (t.file, t.line)
                    and image_names_entry(u.image, va)
                )
                IMAGE_ENTRY_ACCEPTED.append(
                    (addr, f"{t.file}:{t.line}", str(img))
                )
        ts = [t for t in ts if (t.file, t.line) not in accepted_lines]
        if not ts:
            continue
        where = ", ".join(sorted({f"{t.file}:{t.line}" for t in ts}))
        for t in ts:
            out.append(
                Finding(
                    "non-entry",
                    f"non-entry:{t.module}:{addr}",
                    addr,
                    f"FUN_{addr} tagged at {where}",
                    3.0,
                    why,
                )
            )
    # One finding per (file, address): several tag lines in one file name one
    # claim.
    seen: set[str] = set()
    uniq: list[Finding] = []
    for f in out:
        if f.key not in seen:
            seen.add(f.key)
            uniq.append(f)
    return uniq
