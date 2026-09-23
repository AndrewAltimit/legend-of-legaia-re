"""The slot-B module band's own structure: frame partition and spawn records.

PROT `0903..=0966` load at one base and carry no internal `jal`, so neither
Ghidra's flow following nor an address sweep finds their function entries, and
nothing tells their data apart from their code. Two rules do, and both are
named by instructions rather than by a statistic:

* **Frame matching** - a body opens at `addiu sp, sp, -F` and closes at the
  first `jr ra` whose delay slot restores the same `F`. Mirrors
  `ghidra/scripts/dump_static_overlay.py`, whose committed `RANGES` rows it
  reproduces.
* **Spawn records** - the module forms each record's address with a
  `lui`/`addiu` pair and hands it to `FUN_80021B04` / `FUN_80050ED4` in `$a2`,
  so a record's start is an address the module's own code computes. The highest
  record has no pointer above it; its **program** bounds it instead, via
  `move_program_end`.

`legaia_asset::slot_b_module` is the authority for all of this and carries the
disc-gated test (`crates/asset/tests/slot_b_record_bounds_real.rs`). This module
is its mirror for the host-side gates, which must run without a cargo build. It
lives here rather than inside `scripts/ci/disc-coverage.py` because
`attribute-dump-extents.py` and `inherited_tail.py` need the same answers and a
second copy would drift.

Format page: `docs/formats/slot-b-module-layout.md`.
"""

import functools
import struct

MIPS_JR_RA = 0x03E00008
_ADDIU_SP_NEG = 0x27BD0000

SLOT_B_LINK_BASE = 0x801F69D8
# `FUN_80021B04` (the SCUS actor-spawn helper) and `FUN_80050ED4` (its
# pool-tracked wrapper). Both take the record pointer in `$a2`.
SPAWN_HELPERS = (0x80021B04, 0x80050ED4)
# Instructions of `$a2` context scanned back from a spawn call - the window
# `legaia_asset::summon_overlay::parse` uses.
A2_WINDOW_INSNS = 22
# `FUN_80021B04` dispatches `model_sel` as: < 0 transform node, `0x4000` /
# `0x4001` render-mode nodes, otherwise an effect-model-library index. A first
# word outside that set is not a record the helper would seat.
_LIBRARY_MESH_SEL_MAX = 0x100
_RENDER_NODE_SELS = (0x4000, 0x4001)


# Width in HALFWORDS of every move-VM opcode 0x00..=0x46 (`FUN_80023070`'s
# per-arm `param_3`), and of every `0x2F` OVERLAY_EXT sub-opcode. Mirror of
# `legaia_asset::slot_b_module`'s tables - that parser is the authority and this
# copy exists so the gate needs no cargo build. `0` marks the widths that are
# not a constant of the opcode alone, handled by name in `move_program_end`.
MOVE_OP_HALFWORDS = (
    4, 4, 2, 2, 4, 4, 2, 4,
    0, 2, 0, 0, 6, 2, 2, 2,
    2, 2, 2, 16, 5, 2, 2, 2,
    2, 1, 2, 1, 2, 2, 8, 8,
    3, 7, 1, 13, 3, 2, 5, 3,
    2, 2, 2, 4, 5, 4, 4, 0,
    1, 2, 2, 1, 9, 3, 3, 3,
    2, 4, 1, 1, 0, 0, 2, 2,
    7, 2, 15, 1, 4, 8, 4,
)
MOVE_EXT_HALFWORDS = (
    16, 2, 2, 2, 3, 5, 7, 7,
    2, 2, 3, 3, 3, 3, 11, 2,
    2, 2, 8, 4, 4, 2, 2, 8,
    5, 8, 8, 5, 3, 3, 4, 5,
    5, 5, 5, 6, 8, 3, 3, 3,
    5, 5, 8, 6, 7, 6, 13, 3,
    5, 3, 3, 6, 3, 3, 4, 4,
    4, 4, 3, 4, 6,
)
MOVE_OP_HALT = 0x08
MOVE_OP_WAIT = 0x09
MOVE_WAIT_FOREVER = 0x0FFF
MOVE_LOOP_FOREVER = 0x4000
MOVE_WALK_MAX_STEPS = 4096


def move_program_end(image, start):
    """Word-aligned end of the move-VM program at byte `start`, or `None`.

    A record's payload is a move-VM program, and a program ends where nothing
    above it can execute: `0x08` HALT, or an armed `0x19` / `0x1B` idle loop
    (its `0x18` / `0x1A` counter carrying bit `0x4000` makes the branch back
    unconditional) that is not immediately followed by the record's own HALT.
    The end rounds up to 4 because the records are word-aligned.

    Where neither turns up, a `0x09` WAIT carrying `MOVE_WAIT_FOREVER` bounds
    the record instead (the LAST such WAIT the walk passed): a layout argument,
    not a VM one, and the Rust walker's third outcome.

    `None` when the walk meets a halfword that is not a dispatchable opcode
    before any of the three - then nothing here bounds the record.
    """
    pc, loop_a, loop_b = start, 0, 0
    last_forever_wait = None

    def stalled():
        if last_forever_wait is None:
            return None
        return (last_forever_wait + 4 + 3) & ~3

    for _ in range(MOVE_WALK_MAX_STEPS):
        if pc + 2 > len(image):
            return stalled()
        op = struct.unpack_from("<H", image, pc)[0]
        if op > 0x46:
            return stalled()

        def arg(i, _pc=pc):
            o = _pc + i * 2
            return struct.unpack_from("<H", image, o)[0] if o + 2 <= len(image) else 0

        if op == MOVE_OP_HALT:
            return (pc + 2 + 3) & ~3
        if op == 0x18:
            loop_a = arg(1)
        elif op == 0x1A:
            loop_b = arg(1)
        elif ((op == 0x19 and loop_a & MOVE_LOOP_FOREVER)
              or (op == 0x1B and loop_b & MOVE_LOOP_FOREVER)):
            if arg(1) != MOVE_OP_HALT:
                return (pc + 2 + 3) & ~3
        elif op == MOVE_OP_WAIT and arg(1) == MOVE_WAIT_FOREVER:
            last_forever_wait = pc
        if op == 0x0A:
            n = 3 + 3 * arg(2)
        elif op == 0x2F:
            sub = arg(1)
            if sub >= len(MOVE_EXT_HALFWORDS):
                return stalled()
            n = MOVE_EXT_HALFWORDS[sub]
        elif op == 0x3C:
            n = 2 + 6 * max(struct.unpack_from("<h", image, pc + 2)[0]
                            if pc + 4 <= len(image) else 0, 0)
        elif op == 0x3D:
            n = 3 + 6 * max(struct.unpack_from("<h", image, pc + 4)[0]
                            if pc + 6 <= len(image) else 0, 0)
        else:
            n = MOVE_OP_HALFWORDS[op]
        if n == 0:
            return stalled()
        pc += n * 2
    return stalled()


def _jal_word(addr):
    return 0x0C000000 | ((addr >> 2) & 0x03FFFFFF)


def framed_functions(image):
    """Frame-matched function partition: `addiu sp, sp, -F` through the first
    `jr ra` whose delay slot restores the same `F`.

    Mirrors `ghidra/scripts/dump_static_overlay.py`, whose committed `RANGES`
    rows this reproduces. Unlike a count-and-interleave rule it survives a
    frameless leaf, an early `jr ra` inside a body, and a `jr ra` word that is
    data in the image's tail.
    """
    n = len(image) // 4
    w = struct.unpack_from("<%dI" % n, image, 0)
    out = []
    i = 0
    while i < n:
        x = w[i]
        if (x & 0xFFFF0000) == _ADDIU_SP_NEG and (x & 0x8000):
            want = _ADDIU_SP_NEG | (0x10000 - (x & 0xFFFF))
            j = i + 1
            while j + 1 < n:
                if w[j] == MIPS_JR_RA and w[j + 1] == want:
                    out.append((i * 4, (j + 2) * 4))
                    i = j + 1
                    break
                j += 1
        i += 1
    return out


def _resolve_a2(w, site):
    """`$a2` at word index `site`, from the `lui`/`addiu` writes before it.

    `None` when the last write is one the static window cannot follow, or when
    another `jal` intervenes: `$a2` is caller-saved, so a value formed across a
    call is not the one the consumer reads.
    """
    a2 = None
    for j in range(max(0, site - A2_WINDOW_INSNS), site):
        y = w[j]
        op, rs, rt, imm = y >> 26, (y >> 21) & 31, (y >> 16) & 31, y & 0xFFFF
        if op == 3:
            a2 = None
            continue
        if rt != 6:
            continue
        if op == 0x0F:
            a2 = imm << 16
        elif op == 0x09 and rs == 6 and a2 is not None:
            a2 = (a2 + (imm - 0x10000 if imm & 0x8000 else imm)) & 0xFFFFFFFF
        elif op == 0x09 and rs == 0:
            a2 = (imm - 0x10000 if imm & 0x8000 else imm) & 0xFFFFFFFF
        else:
            a2 = None
    return a2


@functools.lru_cache(maxsize=None)
def spawn_record_band(image, base_va):
    """`[(lo_va, hi_va)]` - the image's bounded spawn records.

    Empty for anything but a slot-B module image. Four filters keep a spurious
    pointer out: the CALL SITE must be inside a framed body of this image (an
    image's tail is a same-offset copy of a sibling's bytes, and an inherited
    fragment's calls name the sibling's records), the resolved address must land
    in the image, an intervening `jal` voids the value (above), and the
    `model_sel` must be one the spawn helper dispatches. The first two fire on
    retail; the last two are guards that do not.

    The image's HIGHEST record has no pointer above it, so the band cannot
    bound it - but its PROGRAM can, and `move_program_end` does: the walk ends
    at the record's terminator and the end rounds up to the word the records
    are laid out on. The same walk then chains `[header][program]` records
    above it while the bytes keep reading as records, and stops at eight zero
    bytes - padding, never a record header. Where the walk does not terminate
    the record is left unbounded and claimed by nothing.
    """
    if base_va != SLOT_B_LINK_BASE or len(image) < 8:
        return ()
    n = len(image) // 4
    w = struct.unpack_from("<%dI" % n, image, 0)
    fns = framed_functions(image)
    calls = {_jal_word(a) for a in SPAWN_HELPERS}
    offs = set()
    for i, x in enumerate(w):
        if x not in calls:
            continue
        # The call must be one this image's own code issues: a call word
        # outside every framed body here is an inherited fragment of a sibling
        # module's routine, whose record pointer belongs to that sibling's load.
        if not any(s <= i * 4 < e for s, e in fns):
            continue
        a2 = _resolve_a2(w, i)
        if a2 is None:
            continue
        f = (a2 - base_va) & 0xFFFFFFFF
        if f + 4 > len(image):
            continue
        if any(s <= f < e for s, e in fns):
            continue
        sel = struct.unpack_from("<h", image, f)[0]
        if not (sel == -1 or 0 <= sel < _LIBRARY_MESH_SEL_MAX
                or sel in _RENDER_NODE_SELS):
            continue
        offs.add(f)
    offs = sorted(offs)
    if not offs:
        return ()
    bounds = sorted(set(offs) | {s for s, _ in fns} | {len(image)})
    out = []
    for f in offs[:-1]:
        end = min(x for x in bounds if x > f)
        out.append((base_va + f, base_va + end))
    # The top record, and the chain above it.
    top = offs[-1]
    cap = min([x for x in bounds if x > top] or [len(image)])
    end = move_program_end(image, top + 4)
    if end is not None and top < end <= cap:
        out.append((base_va + top, base_va + end))
        p = end
        while p + 4 <= cap:
            # Eight zero bytes are the module's padding, not a record: no
            # pointer-credited record on the disc opens with a zero header AND
            # a zero first opcode word, and every chained one that did was the
            # gap between an image's last record and its inherited tail.
            if image[p:p + 8] == bytes(8):
                break
            sel = struct.unpack_from("<h", image, p)[0]
            if not (sel == -1 or 0 <= sel < _LIBRARY_MESH_SEL_MAX
                    or sel in _RENDER_NODE_SELS):
                break
            q = move_program_end(image, p + 4)
            if q is None or not (p < q <= cap):
                break
            out.append((base_va + p, base_va + q))
            p = q
    return tuple(out)


def content_end(image, base_va):
    """Structural end of this image's OWN content, in file bytes.

    The top of the spawn-record chain where the image has records, the end of
    the frame-matched code partition otherwise. Everything above is the
    module's padding or a longer image's residue, and this is the figure that
    decides which of two equal-extent siblings owns a shared suffix - see
    `inherited_tail.py`.

    Deliberately not the frame partition alone: a donor's whole function can sit
    in the tail and frame-match there, so PROT 0949's code partition reaches
    file `0x1B8C` while its own content stops at `0x1828`.

    This walks whatever slice it is handed. Over a WHOLE image the chain runs on
    into the donor's residue and the figure overshoots - on 10 of the 83 mapped
    images (PROT 0908 / 0910 / 0919 / 0920 / 0932 / 0943 / 0960 / 0961, and the
    slot-A pair 0974 / 0980 whose figure is the frame partition rather than a
    record chain). On five of those - 0908 / 0910 / 0920 / 0943 / 0961 - the
    overshoot also credits a spawn pointer belonging to the donor.

    The cut and this measurement are mutually recursive, so callers run
    `inherited_tail.tail_starts_fixpoint` rather than measuring once. It settles
    in two rounds and moves no tail cut, which turns the old asymmetry argument
    (an overshoot can only make an image look less like a recipient) into a
    measurement.
    """
    band = spawn_record_band(image, base_va) if base_va == SLOT_B_LINK_BASE else ()
    if band:
        return band[-1][1] - base_va
    fns = framed_functions(image)
    return fns[-1][1] if fns else 0
