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

The rule is deliberately asymmetric. A *strictly longer* sibling is a candidate
writer for the bytes; two images of equal length that share a suffix are both
carrying somebody else's residue and neither can be named as its owner, so the
run is left in both denominators rather than silently dropped from both. The
minimum length keeps a two-word coincidence from cutting an image short: at
`MIN_TAIL_BYTES` the match is sixteen instructions long and runs to the end of
the file, which no shared library routine does unless it is the last thing
linked.
"""

MIN_TAIL_BYTES = 0x40


def _suffix_start(a: bytes, b: bytes) -> int:
    """First offset from which `a` and `b` agree through the end of `a`."""
    n = len(a)
    i = n - 1
    while i >= 0 and a[i] == b[i]:
        i -= 1
    return i + 1


def tail_starts(images, min_tail=MIN_TAIL_BYTES):
    """`{key: (offset, owner_key)}` for every image with an inherited tail.

    `images` is an iterable of `(key, base_va, content_bytes)` triples. Only
    images sharing a `base_va` are compared, because only those are candidates
    to have been written into one another's buffer.
    """
    by_base = {}
    for key, base, data in images:
        by_base.setdefault(base, []).append((key, data))
    out = {}
    for group in by_base.values():
        for key, data in group:
            best = None
            for other_key, other in group:
                if other_key == key or len(other) <= len(data):
                    continue
                start = _suffix_start(data, other[: len(data)])
                if len(data) - start < min_tail:
                    continue
                if best is None or start < best[0]:
                    best = (start, other_key)
            if best:
                out[key] = best
    return out
