#!/usr/bin/env python3
"""Pair a retail-compare run's image children with its headless seeds, tick by tick.

Reads one `LEGAIA_DIAG_TICK_DIGEST` file - the parent `legaia-engine
retail-compare` and every `play-window` child it spawned append to the same
file - splits it into sessions (a process change, or the session frame counter
restarting at `0`), and for each child session names the headless session it
agrees with longest and the first line where the two part. A child that
matches a headless run up to its capture replayed the fight the channels
scored; a `PART` names the tick and the channel where it stopped doing so.

    LEGAIA_DIAG_TICK_DIGEST=/tmp/d.txt scripts/ci/retail-compare.py --images --filter =STATE
    scripts/ci/tick-digest-diff.py /tmp/d.txt

The field compass (`az=`) is left out of the comparison: the child settles its
field before its window assembles the camera's render bias, so the two read
different compasses through the settle and agree again from the first window
tick, before any fight is armed. See `docs/tooling/retail-compare.md`
("The image child plays the same fight").
"""

import re
import sys


def sessions(path):
    out = []
    for line in open(path, encoding="utf-8"):
        line = re.sub(r" az=\d+", "", line.rstrip("\n"))
        pid, frame, rest = line.split(" ", 2)
        if frame == "0" or not out or out[-1][0] != pid:
            out.append((pid, []))
        out[-1][1].append(rest)
    return out


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.split("\n")[0])
        print("usage: tick-digest-diff.py DIGEST")
        return 2
    sess = sessions(sys.argv[1])
    if not sess:
        print("no sessions")
        return 1
    parent = sess[0][0]
    heads = [s for p, s in sess if p == parent]
    kids = [s for p, s in sess if p != parent]
    parted = 0
    for ci, c in enumerate(kids):
        best = (-1, 0)
        for hi, h in enumerate(heads):
            n = 0
            while n < min(len(h), len(c)) and h[n] == c[n]:
                n += 1
            if n > best[0]:
                best = (n, hi)
        n, hi = best
        h = heads[hi]
        ok = n == min(len(h), len(c))
        parted += not ok
        print(f"child {ci} ({len(c)} ticks) vs headless {hi} ({len(h)} ticks): "
              f"{'MATCH' if ok else 'PART'} agree {n}")
        if not ok:
            print("  H", h[n][:300])
            print("  C", c[n][:300])
    return 1 if parted else 0


if __name__ == "__main__":
    sys.exit(main())
