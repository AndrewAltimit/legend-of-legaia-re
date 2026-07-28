#!/usr/bin/env python3
"""One parser for the dump corpus's header, shared by every instrument over it.

A dump in `ghidra/scripts/funcs/` states its extent in two header lines::

    == <label> <VA> (entry=<VA>) [<image>] ==
    size=<N> bytes, <M> instructions

Every instrument that measures the corpus keys on that pair. Each one grew its
own regex for it, and the corpus spells all four fields more than one way,
because it was written by a dozen different dump scripts over a long period. So
each instrument silently rejected a different subset of real dumps.

`A parser's strictness is a claim about the corpus, and an over-strict one
manufactures a gap` - `docs/tooling/dump-corpus-integrity.md` states that as a
law and this module is what stops it recurring per instrument. Import it rather
than writing a fourth regex.

## The spellings that exist

| Field | Spellings |
|---|---|
| the printed VA | bare `801cf098`, `0x801CF098` |
| the entry | `(entry=801cf098)`, `(entry=0x801cf098)`, `(entry=801cf098, label=k10_shared)`, absent entirely, or `(entry 801cf098)` after a `--` header |
| the label | one token (`FUN_801cf098`), or several (`slot-4 handler FUN_80044434`) |
| the size line | `size=N bytes, M instructions`, `... , M instructions (short thunk)`, `... , M instructions, K refs to entry`, or `size=N bytes` with no count |

Rejecting the `0x` VA spelling alone dropped 54 real function dumps; the
`, label=` spelling dropped another 20; a `size=` line with no instruction count
dropped 6 more.

## What is deliberately NOT a dump

The corpus also stores *answers*, and three kinds of answer are not defective
dumps however a shape sweep grades them - a pointer stub is the corpus doing the
right thing with an interior address, and counting it as a defect penalises the
handling that avoids the defect. `reject` names which class a file fell in, so a
caller can report the classes separately instead of one number with a wrong
explanation attached.

No game bytes are read beyond instruction addresses and mnemonics.
"""

from __future__ import annotations

import re

# Header line 1. The label may be several tokens, so the VA is found by
# position-independent search rather than by a fixed field count.
_ENTRY_RE = re.compile(r"\bentry[= ]\s*(?:0x)?([0-9a-fA-F]{8})\b")
_VA_RE = re.compile(r"\b(?:0x)?([0-9a-fA-F]{8})\b")
_BRACKET_RE = re.compile(r"\[[^\]]*\]")

# Header line 2. The instruction count is optional and may be followed by any
# parenthetical or extra field; only the byte count is load-bearing.
_SIZE_RE = re.compile(r"^size=(\d+) bytes(?:,\s*(\d+) instructions)?")

# A second spelling of the same statement: one dumper reports Ghidra's measured
# body as `min=<VA> max=<VA>` (max INCLUSIVE) instead of a size. It emits only
# decompiled C, so the file is not evidence of the instruction text - but the
# extent came from the analysed function body and is evidence of the bytes.
_MINMAX_RE = re.compile(
    r"\bmin=(?:0x)?([0-9a-fA-F]{8})\s+max=(?:0x)?([0-9a-fA-F]{8})\b")

# A printed instruction row. Ghidra prefixes a delay-slot instruction with an
# underscore, and separates address from text by two or more spaces.
_INSN_RE = re.compile(r"^_?([0-9a-fA-F]{8})\s\s+(\S.*)$")

_DISASM_MARKERS = ("--- DISASSEMBLY", "--- RAW")
_DECOMP_MARKERS = ("--- DECOMPILED", "--- PSEUDO-DISASSEMBLY")

# Shapes that are answers rather than dumps, keyed on header-line-1 text.
_NOT_A_DUMP_MARKERS = (
    ("citation pointer", "pointer_stub"),
    ("cite of", "pointer_stub"),
    ("NOFUNC", "nofunc_record"),
    ("DATA REGION", "data_window"),
    ("DATA WINDOW", "data_window"),
)

# Headers that declare a fixed WINDOW rather than a function body. Each is an
# explicit statement by the dump script that the file's extent is whatever was
# asked for, not whatever the routine occupies, so its bytes are not evidence
# that a body was analysed there. `(len=N)` / `(N bytes)` are the hexdump
# spellings; `RAW`/`raw`/`DISASM` head a fixed address-range disassembly.
_WINDOW_RE = re.compile(
    r"\(len=\d+\)|\(\d+ bytes\)|\b(?:RAW|raw)\s+[0-9a-fA-F]{8}\.\.|"
    r"\bRAW DISASM\b|\bDISASM\b\s+[0-9a-fA-F]{8}\.\.|\btable\b|\bentries\b")

# Headers that name a listing or a survey - an analysis answer over many
# functions, whose printed addresses are table rows, not an instruction stream.
_LISTING_RE = re.compile(
    r"\b(?:survey|inventory|Callers of|jumptables|handler table|"
    r"-only functions)\b")

# How many header lines to look at for the `size=` line. Some dumpers emit a
# `requested=... (INTERIOR ...)` or `NOTE:` line between the header and the size.
_SIZE_SCAN_LINES = 4


class Dump:
    """A parsed dump. `entry` and `nbytes` define the byte extent."""

    __slots__ = ("path", "label", "entry", "nbytes", "insns", "image",
                 "printed_va", "source")

    def __init__(self, path, label, entry, nbytes, insns, image, printed_va,
                 source):
        self.path = path
        self.label = label
        self.entry = entry
        self.nbytes = nbytes
        self.insns = insns
        self.image = image
        self.printed_va = printed_va
        # "header" - the size line said so. "disassembly" - derived from the
        # printed address stream because no size line exists. The distinction
        # matters: a header extent is what the dumper measured, a derived one is
        # what the file happens to show.
        self.source = source

    @property
    def end(self):
        return self.entry + self.nbytes

    @property
    def extent(self):
        return (self.entry, self.entry + self.nbytes)


def _header_addresses(line):
    """`(printed_va, entry)` from header line 1, either possibly `None`."""
    entry = _ENTRY_RE.search(line)
    entry_va = int(entry.group(1), 16) if entry else None
    # The image name is bracketed and can itself contain a hex-looking token,
    # so strip brackets before hunting for the printed VA. `entry=` is stripped
    # too so the fallback cannot pick the entry up as the printed VA.
    stripped = _BRACKET_RE.sub(" ", line)
    if entry:
        stripped = stripped.replace(entry.group(0), " ")
    tokens = _VA_RE.findall(stripped)
    printed = int(tokens[-1], 16) if tokens else None
    return printed, entry_va


def _disassembly_addresses(text):
    """Printed instruction addresses in the disassembly section, in order."""
    section = text
    for marker in _DISASM_MARKERS:
        if marker in text:
            section = text.split(marker, 1)[1]
            break
    else:
        return []
    for marker in _DECOMP_MARKERS:
        section = section.split(marker, 1)[0]
    out = []
    for line in section.split("\n"):
        m = _INSN_RE.match(line)
        if m:
            out.append(int(m.group(1), 16))
    return out


def parse_text(text, path=""):
    """`(Dump, None)` on success, `(None, reject_class)` otherwise.

    Reject classes, all of them deliberate rather than defects:

    | class | meaning |
    |---|---|
    | `not_a_dump` | header line is not `==`-delimited: an analysis script's output whose filename happens to end `_<addr>.txt` |
    | `pointer_stub` | `== <addr> (cite of FUN_<addr>) ==` - a recorded interior citation |
    | `nofunc_record` | `== NOFUNC <addr> ==` - a recorded negative |
    | `data_window` | a fixed hex window over a data region, explicitly not a body |
    | `no_extent` | `==` header stating neither a size nor a usable disassembly |
    | `zero_bytes` | states `size=0` |
    | `zero_insns` | states a size but `0 instructions` - Ghidra could not decode one instruction, so the window is data |
    | `no_entry` | an extent but no recoverable entry address |
    """
    lines = text.split("\n")
    if not lines:
        return None, "no_extent"
    head = lines[0].strip()
    if not head.startswith("=="):
        # `-- <VA> in <image> (entry <VA>)` is a real dumper's header; anything
        # else that is not `==` is an analysis script's output.
        if not head.startswith("--"):
            return None, "not_a_dump"
    for needle, klass in _NOT_A_DUMP_MARKERS:
        if needle in head:
            return None, klass
    if head.startswith("==="):
        return None, "not_a_dump"

    printed_va, entry = _header_addresses(head)

    nbytes = insns = None
    for line in lines[1:_SIZE_SCAN_LINES]:
        m = _SIZE_RE.match(line.strip())
        if m:
            nbytes = int(m.group(1))
            insns = int(m.group(2)) if m.group(2) is not None else None
            break

    source = "header"
    if nbytes is None:
        for line in lines[1:_SIZE_SCAN_LINES]:
            m = _MINMAX_RE.search(line)
            if m:
                lo, hi = int(m.group(1), 16), int(m.group(2), 16)
                if hi >= lo:
                    entry, nbytes = lo, hi + 1 - lo
                    source = "minmax"
                break
    if nbytes is None:
        # A window / listing header states no size on purpose, so there is
        # nothing to recover and nothing defective about it. Test this only
        # AFTER the size line, because a real body dump's label can contain one
        # of these words and its own `size=` line settles the question.
        if _WINDOW_RE.search(head):
            return None, "data_window"
        if _LISTING_RE.search(head):
            return None, "not_a_dump"
        # No size line. The printed address stream can supply the extent, but
        # only when it is contiguous - a gapped stream's first and last address
        # bound a range the dump does not actually evidence, and gapped streams
        # are a documented defect of this corpus, not a rarity.
        addrs = _disassembly_addresses(text)
        if not addrs:
            # A header with no size, no disassembly section and no body at all
            # is a dump script that wrote its header and then failed. That IS a
            # defect, and the only one in this group, so it gets its own class
            # rather than sharing one with the deliberate windows.
            body = "\n".join(lines[1:]).strip()
            return None, "empty_dump" if not body else "no_extent"
        if any(addrs[i + 1] - addrs[i] != 4 for i in range(len(addrs) - 1)):
            return None, "gapped_stream"
        if entry is None:
            entry = addrs[0]
        elif entry != addrs[0]:
            # The stream does not start where the header says the body does, so
            # neither end of the range is trustworthy.
            return None, "no_extent"
        nbytes = addrs[-1] + 4 - addrs[0]
        insns = len(addrs)
        source = "disassembly"

    if nbytes <= 0:
        return None, "zero_bytes"
    if insns == 0:
        # `size=1 bytes, 0 instructions` is Ghidra's "bad instruction data":
        # the window is data being asked for as code. Crediting it as covered
        # code would be crediting a failed decode.
        return None, "zero_insns"
    if entry is None:
        entry = printed_va
    if entry is None:
        return None, "no_entry"

    label = head.lstrip("= ").split()[0] if head.lstrip("= ") else ""
    image = None
    brackets = _BRACKET_RE.findall(head)
    if brackets:
        image = brackets[-1].strip("[]")
    return Dump(path, label, entry, nbytes, insns, image, printed_va,
                source), None


def parse_file(path):
    """`parse_text` over a path. Unreadable files reject as `not_a_dump`."""
    try:
        with open(path, errors="replace") as fh:
            # Enough for the header plus a whole disassembly section; the
            # largest body in the corpus is under 5000 instructions.
            text = fh.read()
    except OSError:
        return None, "not_a_dump"
    return parse_text(text, path)
