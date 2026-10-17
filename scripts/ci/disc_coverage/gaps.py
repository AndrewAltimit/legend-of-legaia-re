"""What an undumped run of an image is: record band, data segment, code with
no function boundary, or a real gap to dump.
"""

from __future__ import annotations

import struct

import slot_b_band

from disc_coverage.thresholds import (
    CODE_PLAUSIBLE_MIN,
    CODE_PTR_MAX,
    PLAUSIBLE_OPS,
    TINY_GAP_WORDS,
)

# The band's structure has ONE implementation on the host side
# (`scripts/ghidra-analysis/slot_b_band.py`), mirroring the authority
# `legaia_overlay_images::slot_b_module`. This file used to carry its own copy of the
# frame scan and the record walk, which is how the shape and the attribution
# sweep came to answer differently about the same image.
spawn_record_band = slot_b_band.spawn_record_band
SLOT_B_LINK_BASE = slot_b_band.SLOT_B_LINK_BASE


def merge(intervals):
    merged = []
    for a, b in sorted(intervals):
        if merged and a <= merged[-1][1]:
            merged[-1][1] = max(merged[-1][1], b)
        else:
            merged.append([a, b])
    return merged


MIPS_JR_RA = 0x03E00008
# Floor for the `no_exit` shape (see `gap_shape`). Known code carries a `jr ra`
# every ~500-750 bytes, so a run twice that long with none is a data table; the
# floor is set above one function's worth so a gap holding the interior of one
# long body is not demoted by it. A demoted run is not hidden - it stays in
# `undumped-runs.csv` under its shape, it only leaves the ranked worklist.
NO_EXIT_MIN_BYTES = 1024
# `addiu $sp, $sp, -F` - the only way a MIPS I routine opens a stack frame, and
# therefore the word a function ENTRY is recognised by. Paired with `jr ra`
# (the only way one ends) it bounds every framed body in the corpus; the
# frame-matched partition in `ghidra/scripts/dump/dump_static_overlay.py` is built
# out of exactly these two words. See the `no_boundary` shape.
_ADDIU_SP_NEG = 0x27BD0000
# `addiu $t2, $zero, imm` - the register a PSX BIOS-call thunk loads its jump
# vector into before `jr $t2`.
_ADDIU_T2_ZERO = 0x240A0000
BIOS_VECTORS = (0xA0, 0xB0, 0xC0)

# `lui $rt, 0x8001..0x801F` - the upper half of a PSX RAM address, and the only
# way MIPS I can materialise one. Every routine that touches a global or takes
# the address of anything issues one; a table of records, a texture, or a string
# pool does not contain the word at instruction alignment. This is the second
# leg of the `data_segment` shape below, and it is a decode of one word rather
# than a statistic over many.
_LUI_OP = 0x0F
RAM_PAGE_LO, RAM_PAGE_HI = 0x8001, 0x801F

# --- the slot-B module band's spawn records -------------------------------
#
# Every image in PROT 0903..0966 loads at this base. Their bulk residue is not
# un-dumped code and not a shape any statistic below can name: it is the
# module's own spawn-record band, and the evidence for that is an INSTRUCTION,
# not a distribution. The module forms each record's address with a
# `lui`/`addiu` pair and passes it in `$a2` to a `jal` into one of the two
# spawn helpers, so the record's start is an address the module computes and
# its end is the address the module computes for the next one. That is the
# `spawn_record_band` shape, and `legaia_overlay_images::slot_b_module` is the parser
# that names it - see docs/formats/slot-b-module-layout.md. The walk itself
# lives in `scripts/ghidra-analysis/slot_b_band.py`.


def _band(image, base_va):
    """`spawn_record_band` with the base test in front of the cache.

    `lru_cache` hashes its key, and hashing is O(len) on `bytes` - so calling
    the cached function per gap on `SCUS_942.54` would hash 440 KB thousands of
    times to learn each time that the image is not a slot-B module.
    """
    if base_va != SLOT_B_LINK_BASE:
        return ()
    return spawn_record_band(image, base_va)


def in_record_band(image, base_va, a, b):
    """True when `[a, b)` lies wholly inside one bounded spawn record."""
    return any(lo <= a and b <= hi for lo, hi in _band(image, base_va))


def band_bytes_in(image, base_va, a, b):
    """Bytes of `[a, b)` that lie inside a bounded spawn record."""
    return sum(max(0, min(b, hi) - max(a, lo))
               for lo, hi in _band(image, base_va))


def split_on_band(image, base_va, a, b):
    """`[(start, end, in_band)]` for `[a, b)`, cut at every record edge.

    Used only to LABEL a run that has already been classified, never to
    classify one. Re-classifying the pieces would be the trap `split_gap`'s
    own comment warns about in the other direction: `classify_gap`'s
    data-segment test and its tiny-gap fiat both answer differently for a
    piece than for the gap it came from, so cutting first would flip
    already-correct verdicts on the bytes AROUND the band.
    """
    band = _band(image, base_va)
    if not band:
        return [(a, b, False)]
    edges = sorted({x for lo, hi in band for x in (lo, hi)})
    pts = sorted({a, b} | {c for c in edges if a < c < b})
    out = []
    for u, v in zip(pts, pts[1:]):
        out.append((u, v, any(lo <= u and v <= hi for lo, hi in band)))
    return out


def _last_return(image):
    """Word index of the image's last `jr ra`, or -1."""
    n = len(image) // 4
    words = struct.unpack_from("<%dI" % n, image, 0)
    for i in range(n - 1, -1, -1):
        if words[i] == MIPS_JR_RA:
            return i
    return -1


def data_floor(image, base_va):
    """First VA at or above which the image can hold no complete function body.

    Every MIPS body ends in `jr ra`, so the word after the last `jr ra` and its
    delay slot is the floor of the image's DATA SEGMENT. This is a structural
    fact about the instruction set, not a threshold: below it a body may sit
    un-dumped, at or above it none can end.

    Returns `None` for an image with no `jr ra` at all - nothing to anchor on.
    """
    last = _last_return(image)
    if last < 0:
        return None
    return base_va + (last + 2) * 4


def _has_ram_page_lui(image, base_va, a, b):
    """Does [a, b) contain a `lui $rt, 0x80xx` RAM-page word?"""
    n = (b - a) // 4
    start = a - base_va
    if n <= 0 or start < 0 or start + n * 4 > len(image):
        return False
    for w in struct.unpack_from("<%dI" % n, image, start):
        if (w >> 26) != _LUI_OP or ((w >> 21) & 0x1F) != 0:
            continue
        if RAM_PAGE_LO <= (w & 0xFFFF) <= RAM_PAGE_HI:
            return True
    return False


def in_data_segment(image, base_va, a, b, floor=None):
    """True when [a, b) is the image's data segment rather than un-dumped code.

    Two legs, and the second is what makes the first safe:

    1. `a` is at or above `data_floor` - past the image's last `jr ra`, so no
       COMPLETE function body reaches it.
    2. the run holds no `lui $rt, 0x80xx`. Leg 1 alone would also swallow a body
       the ENTRY BOUNDARY cut short, and two measured images end exactly that
       way: PROT 0902 (`gameover`) and PROT 0977 (`arena_init`) each stop
       mid-routine at the last byte of their sector extent, with no `jr ra` left
       to close the body. A truncated body still addresses globals; a data
       segment does not.

    Corroborated independently by the thirteen slot-B modules whose function
    partition was recovered by FRAME MATCHING in
    `ghidra/scripts/dump/dump_static_overlay.py`: in all thirteen the last range ends
    at exactly `data_floor`, i.e. the frame scan and this rule put the
    code/data boundary in the same place without sharing a test.
    """
    if floor is None:
        floor = data_floor(image, base_va)
    if floor is None or a < floor:
        return False
    return not _has_ram_page_lui(image, base_va, a, b)


def has_no_function_boundary(words):
    """True when `words` holds neither word a function body is delimited by.

    A MIPS I routine opens with `addiu $sp, $sp, -F` (or is a frameless leaf)
    and every one of them ends with `jr ra`. A run carrying **neither** word
    therefore contains no function ENTRY - the only thing a dump can be keyed
    on - and no EXIT for a dump's `size=` to be measured to. Dumping cannot
    close such a run, because a dumper is driven from an address list and this
    run supplies no address.

    This is the length-free form of `no_exit`. That shape needs its
    `NO_EXIT_MIN_BYTES` floor because it tests one word only: a *short* run
    with no `jr ra` is routinely the head of a routine whose exit is past the
    window, and that head is real work. Adding the prologue leg is what
    removes the need for the floor - a run holding a prologue stays `code` at
    any length.

    The residual case, stated rather than papered over: the INTERIOR of one
    long body carries neither word either, so a gap that falls wholly inside
    an un-dumped routine lands here. That is not a lost worklist row. The
    routine's own prologue sits in some adjacent run, which keeps its `code`
    shape and names the entry - and the entry, not its interior, is the
    address a dump is asked for. A demoted run is also not hidden: it stays in
    `undumped-runs.csv` under its shape, it only leaves the ranked worklist.
    """
    for w in words:
        if w == MIPS_JR_RA:
            return False
        if (w & 0xFFFF0000) == _ADDIU_SP_NEG and (w & 0x8000):
            return False
    return True


def gap_shape(image, base_va, a, b, floor=None, data_seg=None, tail=None):
    """Why a code gap is a gap. Six shapes, and only one of them is work.

    A gap is not automatically an un-analysed routine, and reporting the total
    as if it were makes the last percent read as a dump worklist when most of it
    can never be closed by dumping:

    | shape | what it is |
    |---|---|
    | `padding` | every word is `nop`. Inter-function alignment; no function body will ever contain it. |
    | `return_tail` | `jr ra` (+ `nop`): a routine's exit that its analysed body stops short of. |
    | `bios_thunk_slot` | the delay slot of a `jr $t2` BIOS-call thunk. The body ends at the `jr` because the target is a register, so the slot falls outside it. |
    | `psyq_lib_stamp` | an 8-byte record opening with ASCII `Ps`: the PSY-Q librarian's version stamp between link modules. Data the linker left in the text segment. |
    | `constant_table` | every word one repeated non-`nop` constant: a data table resident in text (`crt0`'s stack-pointer table at `0x80026CD4`, four words of the 2 MB RAM size). |
    | `mostly_padding` | at least half the words are zero. A word of zeros is a plausible opcode with no pointer density, so the statistical test scores a zero-dominated region as code; a function body is not half `nop`. |
    | `data_segment` | at or above the image's last `jr ra` and holding no `lui $rt, 0x80xx`. See `in_data_segment`: leg one is that no complete body can end there, leg two is what keeps a body the entry boundary cut short out of the shape. |
    | `spawn_record_band` | one bounded spawn record of a slot-B module image - see `spawn_record_band`. |
    | `no_exit` | `NO_EXIT_MIN_BYTES` or more with no `jr ra` in them. Every MIPS body ends in one, and known code carries one per ~500-750 bytes, so a run this long with none is a data table the opcode statistic scored as code. |
    | `no_boundary` | neither boundary word: no `addiu $sp, $sp, -F` and no `jr ra`, at any length. See `has_no_function_boundary`. |
    | `code` | genuinely un-dumped instructions. |

    The non-`code` shapes are properties of where a function *body* ends or of
    data the linker placed between bodies, not of what has been analysed, so
    they persist however much is dumped. The two data shapes exist because the
    tiny-gap fiat in `classify_gap` counts every short gap as code, and the
    text segment carries linker data records short enough to ride it - each
    instance is documented in `docs/reference/functions/runtime-libs.md`.
    """
    if tail is not None and a >= tail:
        return "inherited_tail"
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
    # PSY-Q librarian version stamp: 8 bytes, ASCII "Ps" + 2 id bytes + a word.
    # Ten word-aligned instances exist in SCUS_942.54; the three in the code
    # band sit exactly between link modules' function bodies.
    if n == 2 and image[start:start + 2] == b"Ps":
        return "psyq_lib_stamp"
    # A run of one repeated non-nop constant is a data table, not reachable
    # code: nothing enters it, and the one code-band instance is crt0's
    # documented stack-pointer table (4 x 0x00200000).
    if n >= 2 and words[0] != 0 and all(w == words[0] for w in words):
        return "constant_table"
    # `spawn_record_band`: the run is one of the slot-B module band's spawn
    # records, both of whose ends the module's own code computes. This is the
    # one shape below `padding` that is named by a PARSER rather than by a
    # property of MIPS - `legaia_overlay_images::slot_b_module` - so it is tested above
    # every statistic and below the shapes that are exact by construction.
    if in_record_band(image, base_va, a, b):
        return "spawn_record_band"
    # Two shapes the opcode statistic cannot see, because both of them look
    # like plausible opcodes to it. Neither is a calibrated threshold on a
    # statistic - both are structural facts about MIPS.
    #
    # `mostly_padding`: a word of zeros decodes to `nop`, which is a plausible
    # primary opcode with no pointer density, so a region that is majority
    # zeros passes the code test with room to spare. A function body is not
    # half `nop`. The menu overlay's whole 0x801E43E8 tail is 82% zeros and the
    # gap classifier called all 62512 bytes of it code.
    if sum(1 for w in words if w == 0) * 2 >= n:
        return "mostly_padding"
    # `data_segment`: the run sits past the image's last `jr ra`, so no complete
    # body reaches it, and it addresses no global, so it is not a body the entry
    # boundary cut short either. This is the shape that names the SCUS static
    # tables (equipment / spell / steal / new-game, all above 0x8006F180), the
    # menu overlay's casino prize table, and the `init.pak` publisher-logo TIMs.
    if data_seg is None:
        data_seg = in_data_segment(image, base_va, a, b, floor)
    if data_seg:
        return "data_segment"
    # `no_exit`: every MIPS function body ends in `jr ra`. Known code carries
    # one per ~500-750 bytes (measured over SCUS's text head and the menu
    # overlay's code region); a data table carries none. A run this long with
    # none is not one or more function bodies - the SCUS sound-effect
    # descriptor table at 0x8006F198 is the worked example, 5120 bytes of
    # small-integer records the opcode statistic scores as code. The floor is
    # long enough that a gap holding one partial body is not caught by it.
    if n * 4 >= NO_EXIT_MIN_BYTES and not any(w == MIPS_JR_RA for w in words):
        return "no_exit"
    # `no_boundary`: neither delimiter word, at any length - see
    # `has_no_function_boundary`. Tested last so every shape above keeps its
    # own, more specific name.
    if has_no_function_boundary(words):
        return "no_boundary"
    return "code"


def classify_gap(image, base_va, a, b, floor=None, data_seg=None, tail=None):
    """True when the bytes in [a, b) look like code rather than data.

    `data_seg` overrides the `in_data_segment` verdict. `split_gap` passes one
    taken over a whole gap, because that test must not be re-run per window.

    `tail` is the VA at which this image stops being its own content - the
    inherited tail (`scripts/ghidra-analysis/inherited_tail.py`). Bytes there
    are a longer sibling's code sitting at the same file offset, so no dump of
    THIS module can ever close them and they are not this module's denominator.
    Tested first, because every other verdict below is a statement about the
    image's own text segment.
    """
    if tail is not None and a >= tail:
        return False
    n = (b - a) // 4
    start = a - base_va
    in_bounds = n > 0 and start >= 0 and start + n * 4 <= len(image)
    # The data segment is tested BEFORE the tiny-gap fiat below. "A short gap is
    # inter-function alignment" is a statement about the code region; above the
    # data floor there are no function bodies for a gap to sit between, so the
    # fiat has nothing to be true of there.
    if data_seg is None:
        data_seg = in_bounds and in_data_segment(image, base_va, a, b, floor)
    if data_seg:
        return False
    # A slot-B spawn record is data by its CONSUMER's pointer-forming
    # instruction, which no statistic over the bytes can see: the record body
    # is move-VM bytecode, and arbitrary bytecode carries `lui $rt, 0x80xx`
    # words, so `in_data_segment`'s second leg rejects the band and the
    # opcode statistic then scores it as code. The test requires FULL
    # containment, so a gap straddling the band's edge falls through to the
    # statistic exactly as before - `cover_image` subtracts the record bytes
    # from that gap's contribution instead of re-cutting the gap.
    if in_bounds and in_record_band(image, base_va, a, b):
        return False
    if n < TINY_GAP_WORDS:
        return True
    if not in_bounds:
        return False
    words = struct.unpack_from("<%dI" % n, image, start)
    # A zero-dominated run is padding, and padding is not the denominator's
    # business. The statistic cannot see that on its own: a word of zeros
    # decodes to `nop`, a plausible primary opcode carrying no pointer, so an
    # all-zero region scores a PERFECT code score and any gap holding enough of
    # one is dragged over the line with it. `gap_shape` has named that case
    # `mostly_padding` all along while the denominator still counted its bytes
    # as un-dumped code - the menu overlay's largest such run is 82% zeros.
    # This is not a calibrated threshold on a statistic: no function body is
    # half `nop`, and an all-zero gap is inter-function alignment by
    # construction.
    if sum(1 for w in words if w == 0) * 2 >= n:
        return False
    plausible = sum(1 for w in words if (w >> 26) in PLAUSIBLE_OPS) / n
    ptrs = sum(1 for w in words if 0x80000000 <= w < 0x80200000) / n
    return plausible >= CODE_PLAUSIBLE_MIN and ptrs < CODE_PTR_MAX


# The classifier is a statistic, so it needs a window it can be a statistic ABOUT.
# Run over a whole multi-kilobyte gap it answers for the mixture rather than for
# any part of it, and the mixture is dominated by whichever component is larger.
# A word of zeros decodes to `nop`, which is a plausible opcode with zero pointer
# density, so a long alignment/padding region drags any gap containing it to
# "code": the menu overlay's largest reported un-dumped run opened with hundreds
# of `nop`s and then non-code, and the whole-gap test called all 28756 bytes
# code. Classifying in windows and merging adjacent same-class windows keeps the
# same test and gives it a scale it can answer at.
GAP_WINDOW_WORDS = 64


def split_gap(image, base_va, a, b, floor=None, tail=None):
    """`[(start, end, is_code)]` for one gap, classified window by window.

    Adjacent windows of the same class are merged, so a real function is one run
    rather than a dozen, and a padding region between two functions separates
    them instead of joining them.
    """
    # The data-segment verdict is taken over the WHOLE above-floor part of the
    # gap, never window by window. A body the entry boundary cut short is a
    # mixture: some of its windows address a global and some do not, and a
    # per-window probe calls the ones that do not "data" and slices the routine
    # into pieces. `gameover` (PROT 0902) is the worked example - the GTE
    # transform block at the head of its truncated tail carries no `lui` at all.
    seg = (floor is not None and b > floor
           and in_data_segment(image, base_va, max(a, floor), b, floor))
    if (b - a) // 4 <= GAP_WINDOW_WORDS:
        return [(a, b, classify_gap(image, base_va, a, b, floor,
                                    data_seg=seg and a >= floor, tail=tail))]
    out = []
    step = GAP_WINDOW_WORDS * 4
    pos = a
    while pos < b:
        end = min(pos + step, b)
        is_code = classify_gap(image, base_va, pos, end, floor,
                               data_seg=seg and pos >= floor, tail=tail)
        if out and out[-1][2] == is_code:
            out[-1] = (out[-1][0], end, is_code)
        else:
            out.append((pos, end, is_code))
        pos = end
    return out
