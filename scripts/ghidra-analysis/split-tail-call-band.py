#!/usr/bin/env python3
"""Split a frameless tail-call band into leaves, from the bytes.

Some emitter families in this game are written as a run of leaves that share
one epilogue. They carry NO stack frame and NO `jr ra`: each leaf finishes by
tail-jumping (`j`) into a common exit, and the next leaf starts immediately
after that jump's delay slot. Neither of the two partitions the rest of the
corpus uses can cut such a band:

* the frame partition `dump_static_overlay.py` uses needs an
  `addiu sp, sp, -F` prologue and a matching `jr ra`, and there is neither;
* cutting at every `jr ra` (`walk_range`) finds none and gives up on the range.

So the band ends up dumped as ONE function, which is a claim about the code
that its own control flow contradicts.

The cut this makes is the `j` whose target is OUTSIDE the band - the shared
exit. A `j` whose target is inside the band is a local early-out inside one
leaf, and cutting on it over-splits: PROT 0901's middle band has 20 `j` sites
and only 8 leaves, because 9 of the 12 remaining jumps are one leaf's own
forward branch to its tail and 4 are the trailing routine's.

The exit target is not assumed. It is read off the band: the target that
appears more than once, outside `[lo, hi)`, is the shared exit; the tool prints
every `j` target with its count so a band that does not have that shape is
visible rather than silently mis-split.

Usage:
  scripts/ghidra-analysis/split-tail-call-band.py \
      extracted/overlays/overlay_world_map_render_0901.bin \
      --base 0x801F69D8 --range 0x801F7644 0x801F8EB4
"""

import argparse
import collections
import struct
import sys

J = 2
JAL = 3
JR_RA = 0x03E00008
ADDIU_SP_NEG = (0x27BD0000, 0x8000)


def words(data, base, lo, hi):
    for va in range(lo, hi, 4):
        off = va - base
        if off < 0 or off + 4 > len(data):
            return
        yield va, struct.unpack_from("<I", data, off)[0]


def target(word):
    return 0x80000000 | ((word & 0x03FFFFFF) << 2)


def main():
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("image")
    ap.add_argument("--base", required=True)
    ap.add_argument("--range", nargs=2, required=True, metavar=("LO", "HI"))
    ap.add_argument("--ranges-row", metavar="LABEL",
                    help="print the partition as a dump_static_overlay.py "
                         "RANGES row for this program label")
    args = ap.parse_args()

    base = int(args.base, 16)
    lo, hi = (int(x, 16) for x in args.range)
    data = open(args.image, "rb").read()

    js, jals, jrs, prologues = [], [], [], []
    for va, w in words(data, base, lo, hi):
        op = w >> 26
        if w == JR_RA:
            jrs.append(va)
        elif op == J:
            js.append((va, target(w)))
        elif op == JAL:
            jals.append((va, target(w)))
        elif (w & 0xFFFF0000) == ADDIU_SP_NEG[0] and (w & ADDIU_SP_NEG[1]):
            prologues.append(va)

    print("band 0x%08X..0x%08X  %d bytes" % (lo, hi, hi - lo))
    print("  prologues %d · jr ra %d · jal %d · j %d"
          % (len(prologues), len(jrs), len(jals), len(js)))
    if prologues or jrs:
        print("  NOTE: this band has frames or returns - the ordinary frame "
              "partition applies and this tool is the wrong instrument.")

    counts = collections.Counter(t for _, t in js)
    outside = {t: n for t, n in counts.items() if not lo <= t < hi}
    print("  j targets:")
    for t, n in counts.most_common():
        print("    0x%08X  x%-3d %s" % (t, n, "OUTSIDE band" if t in outside else "local"))
    if not outside:
        print("  no `j` leaves the band - nothing to cut on")
        return 1
    exit_va = max(outside, key=lambda t: outside[t])
    print("  shared exit: 0x%08X (%d tail jumps)" % (exit_va, outside[exit_va]))
    if len(outside) > 1:
        print("  WARNING: more than one out-of-band `j` target; the cut uses "
              "the most frequent one, check the others by hand.")

    cuts = [lo]
    for va, t in js:
        if t == exit_va and lo <= va + 8 < hi:
            cuts.append(va + 8)
    cuts = sorted(set(cuts)) + [hi]
    print("  %d leaves:" % (len(cuts) - 1))
    for a, b in zip(cuts, cuts[1:]):
        ncalls = sum(1 for va, _ in jals if a <= va < b)
        print("    0x%08X..0x%08X  %5d B  %3d insn  %d jal"
              % (a, b, b - a, (b - a) // 4, ncalls))
    if args.ranges_row:
        body = ", ".join('("%08x", "%08x")' % (a, b)
                         for a, b in zip(cuts, cuts[1:]))
        print()
        print('    "%s": [' % args.ranges_row)
        print("        %s" % body)
        print("    ],")
    return 0


if __name__ == "__main__":
    sys.exit(main())
