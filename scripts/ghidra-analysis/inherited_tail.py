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
