"""The extracted images, and whether one cited address is a call, branch or
table site of another.
"""

from __future__ import annotations

from functools import lru_cache

try:  # py311+
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - py<3.11
    tomllib = None  # type: ignore[assignment]

from port_provenance import paths
from port_provenance.paths import OVERLAY_MAP
from port_provenance.thresholds import RAM_HI, RAM_LO

# ---------------------------------------------------------------------------
# Site citations, read off the extracted images
# ---------------------------------------------------------------------------
#
# `absent-citation` / `doc-citation` ask one question: does the row cite an
# address the routine does not touch? The dump corpus can only ever answer half
# of it, because a dump holds the *callee's* bytes. A row that says "called from
# the `jal` at 0x801E4B1C" is citing evidence that by construction lives in
# somebody else's body, and the checker read every such row as unsupported -
# seven of them were waived by hand, one waiver each, and the reasons all said
# the same thing in different words.
#
# What settles those rows is not a waiver but the disc: the word at the cited
# address either is a `jal` to the subject or it is not. So the checker reads
# the extracted images directly, at the bases the static-overlay map records,
# and accepts three byte-derived relations between a cited address `c` and the
# row's subject `s`:
#
#   reaches       the word at `c` is a `jal` / `j` / conditional branch whose
#                 target is `s`, or a `lui`+`addiu`/`ori` pair at `c` forms `s`
#   returns-from  the word at `c - 8` is a `jal` to `s` - `c` is the RETURN
#                 address the call leaves in `ra`, which is what a live-probe
#                 row cites when it pins a caller by the `ra` it observed
#   adjacent-body `c` and `s` sit in the same, or in two touching, byte-derived
#                 bodies - the span between `jr ra` boundaries, the boundary
#                 words included, which is what makes "this address is interior
#                 to that body" and "this is the nearest boundary above it"
#                 checkable claims rather than prose
#   co-sited      `c` shares a body with another address the same row cites
#                 that *does* reach `s` - the shape of a row naming a caller's
#                 `jal` site and, in the same breath, the stores next to it
#   jump-table    the words at `c` hold `s`, or hold a co-cited address that
#                 reaches `s` - a row pinning a routine as arm N of a switch
#                 cites the table, and a table is data the callee never forms
#
# All three are properties of the bytes, not of a dump header, so they hold
# where the printed VAs do not (docs/tooling/dump-corpus-integrity.md). The
# cost is VA aliasing: several overlays link over one base, so a relation found
# in *an* image covering the address is reported with that image's name and is
# not by itself proof the row meant that image.
#
# Without `extracted/` the test is silent and the rows read as they did before.

# `jr ra`. The only function boundary visible in raw bytes without a symbol
# table, and the delimiter every body walk here uses.
JR_RA = 0x03E00008

# How far a body walk may run before the region stops looking like delimited
# code. Nothing keeps a `jr ra` from being absent for the length of a `.rodata`
# block, and an unbounded walk would then swallow the whole image and call
# every address in it "adjacent". The largest real body this has to admit is
# the battle SM's 0x2940-byte span at 0x801EC3DC..0x801EED1C.
BODY_SPAN_MAX = 0x4000

# How far into a cited jump table to look for the arm the row claims. A
# switch small enough to be cited by arm number is small; widening this
# only buys coincidental 32-bit matches.
TABLE_WORDS_MAX = 16

# How many words before a `jal` an argument-setup citation may sit. A row that
# pins *which value* a call is handed cites the instruction that forms the
# argument, not the call word: `0x801F1D0C` is the `lbu a2, 0x20(v0)` feeding
# the `jal 0x80055468` one word later. The whole argument block of a four-
# argument R3000 call fits in this window, and the match is additionally
# confined to the citation's own `jr ra`-delimited body, so a coincidence has
# to be a call to the exact subject a handful of instructions away.
ARG_SETUP_WORDS_MAX = 4


class Image:
    """One extracted retail image, with the base its bytes are linked at."""

    __slots__ = ("label", "base", "data", "off")

    def __init__(self, label: str, base: int, data: bytes, off: int = 0):
        self.label = label
        self.base = base
        self.data = data
        # File offset of `base`. Non-zero only for the PS-EXE, whose 0x800-byte
        # header is not part of the loaded image.
        self.off = off

    def covers(self, va: int) -> bool:
        i = va - self.base + self.off
        return 0 <= i <= len(self.data) - 4

    def word(self, va: int) -> int | None:
        if not self.covers(va):
            return None
        i = va - self.base + self.off
        return int.from_bytes(self.data[i:i + 4], "little")


_IMAGES: list[Image] | None = None


def images() -> list[Image]:
    """Every extracted image, keyed by the base `static-overlays.toml` records.

    Bases are never guessed: the PS-EXE carries its own `t_addr`, and each
    overlay's base is the statically recovered one the map holds. An overlay
    with no row in the map contributes nothing, which is the conservative
    direction - a wrong base would decode a call to a wrong target.
    """
    global _IMAGES
    if _IMAGES is not None:
        return _IMAGES
    out: list[Image] = []
    scus = paths.EXTRACTED / "SCUS_942.54"
    if scus.is_file():
        raw = scus.read_bytes()
        if raw[:8] == b"PS-X EXE" and len(raw) > 0x800:
            t_addr = int.from_bytes(raw[0x18:0x1C], "little")
            out.append(Image(scus.name, t_addr, raw, 0x800))
    if tomllib is not None and OVERLAY_MAP.is_file():
        with OVERLAY_MAP.open("rb") as fh:
            rows = tomllib.load(fh).get("overlays", [])
        for row in rows:
            label, base = row.get("label"), row.get("base_va")
            idx = row.get("prot_index")
            if not label or not base or idx is None:
                continue
            path = paths.EXTRACTED / "overlays" / f"overlay_{label}_{idx:04d}.bin"
            if path.is_file():
                out.append(Image(path.stem, int(base), path.read_bytes()))
    _IMAGES = out
    return out


def _branch_target(va: int, w: int) -> int | None:
    """The address this word transfers control to, or None if it does not."""
    op = w >> 26
    if op in (2, 3):  # j / jal
        return ((w & 0x03FFFFFF) << 2) | ((va + 4) & 0xF0000000)
    # beq / bne / blez / bgtz, REGIMM (bltz, bgez, ...) and the coprocessor
    # branches. Register-indirect transfers resolve to nothing here on purpose.
    if op in (1, 4, 5, 6, 7) or (op in (0x10, 0x11, 0x12) and ((w >> 21) & 0x1F) == 8):
        imm = w & 0xFFFF
        return va + 4 + ((imm - 0x10000 if imm & 0x8000 else imm) << 2)
    return None


def _pair_forms(img: Image, at: int, target: int) -> bool:
    """A `lui`+`addiu`/`ori` pair starting at (or ending at) `at` forms `target`."""
    for lui_at in (at, at - 4):
        w0, w1 = img.word(lui_at), img.word(lui_at + 4)
        if w0 is None or w1 is None or (w0 >> 26) != 0x0F:
            continue
        rt = (w0 >> 16) & 0x1F
        op1 = w1 >> 26
        if op1 not in (0x09, 0x0D) or ((w1 >> 21) & 0x1F) != rt:
            continue
        imm = w1 & 0xFFFF
        hi = (w0 & 0xFFFF) << 16
        if op1 == 0x09:
            val = (hi + (imm - 0x10000 if imm & 0x8000 else imm)) & 0xFFFFFFFF
        else:
            val = hi | imm
        if val == target:
            return True
    return False


# Memoised because a row cites several addresses in one body and the
# subject is walked once per citation: without it the same 16 KB window
# is re-scanned for every address on the row, in every aliased image.
@lru_cache(maxsize=None)
def _body(img: Image, va: int) -> tuple[int, int] | None:
    """The `jr ra`-delimited span `va` sits in, both boundary words included.

    Inclusive on purpose. A row that pins an interior address by naming the
    nearest boundary above it is citing the boundary *as* the evidence, and an
    exclusive walk would read that citation as unsupported. It also makes two
    consecutive bodies overlap by the `jr ra` and its delay slot, which is what
    "touching" means below.
    """
    if not img.covers(va):
        return None
    lo = va
    while img.word(lo) != JR_RA:
        lo -= 4
        if va - lo > BODY_SPAN_MAX or not img.covers(lo):
            return None
    hi = va
    while img.word(hi) != JR_RA:
        hi += 4
        if hi - va > BODY_SPAN_MAX or not img.covers(hi):
            return None
    return lo, hi + 8


def site_relation(cited: int, subject: int, co_cited: set[int]) -> str | None:
    """Why `cited` is evidence *about* `subject` rather than a claim it owns.

    Returns a one-line reason naming the image and the relation, or None when
    no extracted image puts the two in any of the three relations above.
    """
    if not (RAM_LO <= cited <= RAM_HI and RAM_LO <= subject <= RAM_HI):
        return None
    for img in images():
        w = img.word(cited)
        if w is None:
            continue
        if _branch_target(cited, w) == subject:
            kind = {2: "j", 3: "jal"}.get(w >> 26, "branch")
            return f"{kind} to 0x{subject:08x} at 0x{cited:08x} in {img.label}"
        if _pair_forms(img, cited, subject):
            return (
                f"lui/addiu pair forming 0x{subject:08x} at 0x{cited:08x} "
                f"in {img.label}"
            )
        # A live-probe row cites the RETURN address, not the `jal`. On R3000
        # `ra` is the call word plus eight (the branch and its delay slot), so
        # the caller-site test has to look there too or every `ra ...` citation
        # reads as unsupported while the bytes say plainly which routine the
        # probe was standing in.
        wj = img.word(cited - 8)
        if wj is not None and (wj >> 26) == 3 and _branch_target(cited - 8, wj) == subject:
            return (
                f"0x{cited:08x} is the return address of the jal to "
                f"0x{subject:08x} at 0x{cited - 8:08x} in {img.label}"
            )
        # A row that pins *which value* a call is handed cites the instruction
        # that forms the argument, not the call word. Without this the citation
        # misses the site test by one instruction and reads as unsupported,
        # which is how the record-`+0x20` wide-flag row surfaced as a finding.
        for k in range(1, ARG_SETUP_WORDS_MAX + 1):
            wa = img.word(cited + 4 * k)
            if wa is None or (wa >> 26) != 3:
                continue
            if _branch_target(cited + 4 * k, wa) != subject:
                continue
            if _body(img, cited) != _body(img, cited + 4 * k):
                continue
            return (
                f"0x{cited:08x} forms an argument for the jal to "
                f"0x{subject:08x} at 0x{cited + 4 * k:08x} in {img.label}"
            )
    for img in images():
        bc, bs = _body(img, cited), _body(img, subject)
        if bc and bs and bc[0] < bs[1] and bs[0] < bc[1]:
            return (
                f"0x{cited:08x} and 0x{subject:08x} sit in the same or touching "
                f"jr-ra-delimited bodies (0x{bc[0]:08x}..0x{bc[1]:08x} / "
                f"0x{bs[0]:08x}..0x{bs[1]:08x}) in {img.label}"
            )
    reaching = {
        other
        for other in co_cited
        if other != cited
        and any(
            (w := img.word(other)) is not None
            and _branch_target(other, w) == subject
            for img in images()
        )
    }
    for other in sorted(reaching):
        for img in images():
            w = img.word(other)
            if w is None or _branch_target(other, w) != subject:
                continue
            bc, bo = _body(img, cited), _body(img, other)
            if bc and bo and bc == bo:
                return (
                    f"0x{cited:08x} shares a body with 0x{other:08x}, which "
                    f"reaches 0x{subject:08x}, in {img.label}"
                )
    # A switch's jump table. `c` is data, so no body walk reaches it and the
    # callee never forms it - but the table's own words say whether the row's
    # "arm N of that switch" is true. Bounded to `TABLE_WORDS_MAX` words and to
    # targets the row itself names, so an unrelated global cannot match by
    # accident.
    for img in images():
        for k in range(TABLE_WORDS_MAX):
            w = img.word(cited + 4 * k)
            if w is None:
                break
            if w == subject or w in reaching:
                return (
                    f"the word at 0x{cited + 4 * k:08x} (0x{cited:08x} + {k}) is "
                    f"0x{w:08x}, which reaches 0x{subject:08x}, in {img.label}"
                )
    return None
