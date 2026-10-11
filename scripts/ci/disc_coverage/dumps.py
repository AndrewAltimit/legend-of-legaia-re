"""The dump corpus as extents: which bytes of which address a dump covers.
"""

from __future__ import annotations

import glob
import os
import re

import dump_header

# How many opening instructions have to be `nop` before a dump's extent is
# withdrawn. `nop` encodes `0x00000000`, so a head of them is zero fill and
# nothing about it corroborates the printed base - the same window exists at
# thousands of VAs in every image. A dump whose head carries no evidence for
# where it is cannot be credited anywhere, however much real code follows it.
ZERO_HEAD_INSNS = 24
_HEAD_INSN_RE = re.compile(r"^\s*([0-9a-fA-F]{8})\s+(\S+)")
_DISASM_MARKER = "--- DISASSEMBLY ---"


def zero_headed(text):
    """Does the dump open with `ZERO_HEAD_INSNS` consecutive `nop`s?

    This is the byte-level sibling of `dump-corpus-integrity.md`'s rule that a
    dump's printed addresses are a property of its load base. A base is
    corroborated by the bytes AT the entry reproducing in one image; when those
    bytes are zeros, every image reproduces them, so the corroboration is
    vacuous and the extent's placement rests on the filename.
    """
    if _DISASM_MARKER not in text:
        return False
    seen = 0
    for line in text.split(_DISASM_MARKER, 1)[1].split("\n"):
        m = _HEAD_INSN_RE.match(line)
        if not m:
            if seen:
                break
            continue
        if m.group(2).lower().lstrip("_") != "nop":
            return False
        seen += 1
        if seen >= ZERO_HEAD_INSNS:
            return True
    return False


def read_dump_extents(funcs_dir):
    """Every dump's (entry_va, end_va), plus a census of what was excluded.

    Returns `(extents, rejects)` where `rejects` counts `dump_header`'s reject
    classes plus this module's own `zero_window_head`. The census is the point:
    a single "N files had no parseable header" number invites - and previously
    carried - a wrong explanation of what those files are. Most of them are the
    corpus storing an ANSWER, and only a handful are defective dumps.
    """
    out = []
    rejects = {}
    for path in sorted(glob.glob(os.path.join(funcs_dir, "*.txt"))):
        try:
            with open(path, errors="replace") as fh:
                text = fh.read()
        except OSError:
            rejects["not_a_dump"] = rejects.get("not_a_dump", 0) + 1
            continue
        dump, reject = dump_header.parse_text(text, path)
        if dump is None:
            rejects[reject] = rejects.get(reject, 0) + 1
            continue
        if zero_headed(text):
            rejects["zero_window_head"] = rejects.get("zero_window_head", 0) + 1
            continue
        out.append(dump.extent)
    return out, rejects


def dump_names_by_extent(funcs_dir):
    """`{(entry_va, end_va): [dump stem, ...]}` over the corpus.

    Built only when the ratchet has something to explain: the point of the
    report is to hand back file names a reader can open, and an extent key on
    its own is not that.
    """
    out = {}
    for path in sorted(glob.glob(os.path.join(funcs_dir, "*.txt"))):
        dump, _ = dump_header.parse_file(path)
        if dump is not None:
            out.setdefault(dump.extent, []).append(
                os.path.basename(path)[:-4])
    return out
