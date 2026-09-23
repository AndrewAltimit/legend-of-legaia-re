"""Where an overlay image stops being its own content.

A PROT entry's extent is sector-granular, and the packer that built the disc
wrote each overlay into a buffer it did **not** clear first. So a module shorter
than the buffer flushes its own bytes followed by whatever the previous, longer
module left there - and the extraction hands that residue back inside
`content_bytes`.

That residue is measurable rather than inferred. Two images at the same link
base are compared byte for byte at the SAME file offset: where a strictly longer
sibling reproduces this image's bytes from some offset all the way to the end of
its content, the run from that offset is the shorter image's **inherited tail**.
It is the longer image's code, sitting at the same offset it sits at there.

Why this matters to two instruments at once:

* `scripts/ci/disc-coverage.py` measures un-dumped code against
  `content_bytes`. Counting a tail inflates the image's own denominator with
  another module's bytes, which no dump of *this* module can ever close.
* `scripts/ghidra-analysis/attribute-dump-extents.py` asks which image's own
  content reproduces a dump's opening window. Without the tail cut, a dump that
  lies wholly inside one image's tail reads as `identical` - two images hold
  these bytes - when only one of them owns them. `overlay_cast_curse_0943`'s
  dump at `0x801F7D34` is the worked case: PROT 0943's own content ends at file
  `0x1037` (VA `0x801F7A0F`) and 0x801F7D34 is 0x32D bytes past it, in PROT
  0942's residue.

The minimum length keeps a two-word coincidence from cutting an image short: at
`MIN_TAIL_BYTES` the match is sixteen instructions long and runs to the end of
the file, which no shared library routine does unless it is the last thing
linked.

The `own_ends` measurement below and the cut are mutually recursive - the cut
needs the measurement, and the measurement is wrong until the cut is applied -
so `tail_starts_fixpoint` iterates the pair instead of taking the first
estimate. See its docstring for which images that matters on.

Which siblings may be donors
----------------------------

The buffer the residue comes out of is indexed by FILE OFFSET, not by link
address, so a donor need not be at this image's `base_va` - and it need not be
longer either, only longer *in its own content*. Two earlier restrictions
encoded the opposite and each lost real residue:

* **Same `base_va`.** Dropped. The five images whose donor is at another base
  are the loudest cases in the whole set: PROT 0904 / 0912 / 0917 / 0918 / 0922
  end in PROT 0899's menu code, and `gameover`'s tail is `world_map_render`'s.
  A 2000-byte byte-identical run at the same file offset in two unrelated
  modules is residue whatever either one is linked at.
* **Strictly longer.** Kept as the *default* leg, and joined by a second one
  for the equal-extent case rather than dropped. `content_bytes` is the PROT
  entry's sector extent, so two modules that round to the same number of
  sectors both read as "equal length" while one of them really does hold more
  content. That case is settled by a measurement, not by the extent: the
  `own_ends` argument carries each image's structural own-content end (for a
  slot-B module, the top of its spawn-record chain -
  `legaia_asset::slot_b_module::content_end`), and an equal-extent donor is
  admitted only when its own content reaches ABOVE the shared suffix while the
  recipient's does not. That is what breaks the symmetry the old rule refused
  to break: on PROT 0917 / 0918 it names 0917 the donor, because 0917's record
  chain runs past the shared start and 0918's stops below it.

A figure belongs to whichever rule it was measured under. Same base + strictly
longer reports 66 of the 83 mapped images and 61,597 bytes; any base + strictly
longer reports 79 and 76,916; adding the gated equal-extent leg reports 79 and
88,150. The unguarded "any base, any length >=" variant reports 79 and 104,700
and is NOT what this module does - it names a donor wherever a suffix matches,
including the pairs where neither image can be shown to own the bytes.
"""

import os

MIN_TAIL_BYTES = 0x40


def _suffix_start(a: bytes, b: bytes) -> int:
    """First offset from which `a` and `b` agree through the end of `a`."""
    n = len(a)
    i = n - 1
    while i >= 0 and a[i] == b[i]:
        i -= 1
    return i + 1


def tail_starts(images, min_tail=MIN_TAIL_BYTES, own_ends=None):
    """`{key: (offset, owner_key)}` for every image with an inherited tail.

    `images` is an iterable of `(key, base_va, content_bytes)` triples; the
    `base_va` is carried for the caller's convenience and is NOT used to
    restrict donors (see the module docs - the mastering buffer is indexed by
    file offset).

    `own_ends` is `{key: structural own-content end in bytes}`. It gates the
    equal-extent leg only: without it, equal-extent donors are not considered
    and the result is the "any base, strictly longer" figure.
    """
    imgs = [(key, data) for key, _base, data in images]
    own = own_ends or {}
    out = {}
    for key, data in imgs:
        best = None
        for other_key, other in imgs:
            if other_key == key or len(other) < len(data):
                continue
            equal_extent = len(other) == len(data)
            if equal_extent and not own_ends:
                continue
            start = _suffix_start(data, other[: len(data)])
            if len(data) - start < min_tail:
                continue
            # An equal-extent sibling is a donor only where the bytes can say
            # so: its own content must reach above the shared suffix and this
            # image's must not. Absent either measurement, decline.
            if equal_extent and not (
                own.get(other_key, 0) > start and own.get(key, len(data)) <= start
            ):
                continue
            if best is None or start < best[0]:
                best = (start, other_key)
        if best:
            out[key] = best
    return out


def tail_starts_fixpoint(images, own_end, min_tail=MIN_TAIL_BYTES,
                         max_rounds=8):
    """`tail_starts` iterated until the cuts stop moving.

    `own_end(key, base, data)` returns an image's structural own-content end in
    bytes, and it is the input `tail_starts` gates its equal-extent leg on. The
    first round has to measure it over the WHOLE image, because no cut is known
    yet - and that is exactly where it overshoots: a slot-B module's spawn-record
    chain walks straight on into its donor's residue and reports an own-content
    end above the tail. Five images in the band do that (PROT 0908 / 0910 / 0920
    / 0943 / 0961), and the overshoot is a claim about the donor question the
    figure is there to answer.

    So each round re-measures `own_end` over the image CUT at the tail the
    previous round found. An overshoot that existed only because the residue was
    still attached disappears; a record chain that really does reach that far is
    unaffected, because its records lie below the cut.

    Returns the same `{key: (offset, owner_key)}` mapping. The iteration is
    bounded rather than trusted to converge: the two legs move in opposite
    directions (cutting a recipient makes it more recipient-shaped, cutting a
    donor less donor-shaped), so a pathological pair could alternate. Eight
    rounds is far past the two the retail band needs; the last state is
    returned either way, and `tail_starts_fixpoint_rounds` says how many were
    spent so a caller can report a non-convergence instead of printing a number
    that is really round eight of an oscillation.
    """
    global tail_starts_fixpoint_rounds
    cuts = {}
    for round_no in range(1, max_rounds + 1):
        own = {}
        for key, base, data in images:
            cut = cuts.get(key)
            own[key] = own_end(key, base, data[:cut[0]] if cut else data)
        nxt = tail_starts(images, min_tail=min_tail, own_ends=own)
        tail_starts_fixpoint_rounds = round_no
        if nxt == cuts:
            return cuts
        cuts = nxt
    return cuts


# How many rounds the last `tail_starts_fixpoint` call spent. Equal to
# `max_rounds` means it did not converge.
tail_starts_fixpoint_rounds = 0


# ---------------------------------------------------------------------------
# The packer's buffer, predicted in TOC order
# ---------------------------------------------------------------------------
#
# The sibling comparison above finds a tail by matching another MAPPED OVERLAY.
# The buffer the residue comes out of is one buffer for the whole of PROT.DAT,
# filled in extraction order and never cleared, so the residue at file offset
# `k` of an entry's last sector is the byte the NEAREST EARLIER ENTRY whose
# extent reaches `k` holds there - an overlay or not. That prediction has no
# free parameter (the donor is fixed by the TOC order and the entry lengths),
# and it is the Python side of `legaia_asset::inherited_tail::buffer_run` /
# `buffer_suffix_start`.
#
# It is what cuts the two images no sibling can: PROT 0898's and 0895's last
# sectors are PROT 0894's bytes, and 0894 is not an overlay. And where the two
# rules disagree it is the one a consumer settles for (PROT 0901, see
# `tail_cuts`).

def prot_entry_index(prot_dir):
    """`{idx: (path, length)}` for every `NNNN_*` entry under `prot_dir`.

    First name (sorted) wins on a duplicate index, the tie-break the Rust side
    applies."""
    out = {}
    for name in sorted(os.listdir(prot_dir)):
        head = name[:4]
        if len(name) < 5 or not head.isdigit() or name[4] != "_":
            continue
        idx = int(head)
        if idx in out:
            continue
        path = os.path.join(prot_dir, name)
        out[idx] = (path, os.path.getsize(path))
    return out


def buffer_predict(entries, idx, length, start):
    """`(pieces, bytes)`: the buffer's bytes at `[start, length)` just before
    entry `idx` was written. `pieces` is `[(start, end, donor_idx_or_None)]`;
    `None` means no earlier entry reached that far and the buffer is zero."""
    if idx not in entries or entries[idx][1] != length or start >= length:
        return None
    earlier = sorted((i for i in entries if i < idx), reverse=True)
    pieces, out, k = [], bytearray(), start
    while k < length:
        donor = next((i for i in earlier if entries[i][1] > k), None)
        end = length if donor is None else min(entries[donor][1], length)
        if donor is None:
            out += bytes(end - k)
        else:
            with open(entries[donor][0], "rb") as fh:
                fh.seek(k)
                out += fh.read(end - k)
        pieces.append((k, end, donor))
        k = end
    return pieces, bytes(out)


def buffer_suffix(entries, idx, data, min_tail=MIN_TAIL_BYTES):
    """`(offset, donor)`: the lowest offset from which the buffer prediction
    reproduces `data` through its end, and the entry the buffer held there.
    `None` under `min_tail` bytes, or where the run opens on never-written
    (zero) buffer.

    The search is NOT confined to the last sector. PROT 0976's run starts
    `0x98C` bytes below its end, and it is PROT 0970's code at the same file
    offsets - the packer's extent for an overlay can exceed its content by more
    than a sector, so a sector bound only hides such a run."""
    got = buffer_predict(entries, idx, len(data), 0)
    if got is None:
        return None
    pieces, predicted = got
    i = len(data)
    while i > 0 and data[i - 1] == predicted[i - 1]:
        i -= 1
    if len(data) - i < min_tail:
        return None
    donor = next(d for a, b, d in pieces if a <= i < b)
    return None if donor is None else (i, donor)


def tail_cuts(images, own_end, prot_dir=None, min_tail=MIN_TAIL_BYTES):
    """The one tail rule both instruments use: `{key: (offset, donor_key)}`.

    `images` is `(prot_index, base_va, data)` triples - keyed by PROT index, so
    the buffer leg can find each image's place in the TOC. The sibling
    fixpoint runs first; then, where `prot_dir` is given, the packer-buffer
    suffix wins wherever it cuts LOWER than the sibling or where no sibling
    cuts at all. Where both cut at the same offset the sibling's donor is kept
    (it names the image that first wrote the bytes; the buffer names the one it
    last held them from).

    On the retail disc the buffer reproduces 82 of the sibling rule's 83 cuts
    offset for offset, and adds or moves exactly three:

    * PROT 0898 and PROT 0895 have no sibling donor: their runs are PROT
      0894's bytes, and 0894 is not an overlay.
    * PROT 0901, where the sibling rule stops short. Its equal-extent donor
      0900 is declined by the own-content gate because a 0900 routine
      frame-matches inside 0901's residue. The consumer settles it: 0901's own
      code ends at file `0x24E4` with a zero run above it, and the run from
      `0x252A` (two zero bytes coincide below the first word) opens mid-routine on the epilogue of 0900's routine at
      `0x801F8E6C` (prologue at 0900 file `0x2494`, whose `beqz` at `0x24D4`
      targets `0x801F8F0C`); no instruction of 0901 below the run forms any
      address in `0x801F8F04..0x801F9088`.
    """
    cuts = dict(tail_starts_fixpoint(images, own_end, min_tail=min_tail))
    if prot_dir is None or not os.path.isdir(prot_dir):
        return cuts
    entries = prot_entry_index(prot_dir)
    for key, _base, data in images:
        got = buffer_suffix(entries, key, data, min_tail=min_tail)
        if got is None:
            continue
        have = cuts.get(key)
        if have is None or got[0] < have[0]:
            cuts[key] = got
    return cuts
