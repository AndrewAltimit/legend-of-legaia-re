#!/usr/bin/env python3
"""Price each field frame's work apart from its CD interrupt time.

Reads `cd.csv` from `scripts/pcsx-redux/autorun_field_cd_trace.lua` (PCSX-Redux
breakpoint core). A frame is the window the frame-cost words measure, from the
frame end's `VSync(n)` return (`0x800171C8`) to the CPU-done `VSync(1)`
(`0x800174CC`); inside it every CD interrupt is priced from the exception vector
`0x80000080` to the response dispatcher's exit (`0x8005DB74`), and the rest is the
frame's work. Over a voice line the ratio of the frame's CPU time to its work is
what `field_xa_loaded_frame_cost` multiplies a field frame's priced work by.

    scripts/ci/field-cd-frames.py captures/<dir>/cd.csv --from 450 --to 980

Numbers only: it changes nothing and gates nothing. The recordings hold retail RAM
and live under the gitignored `captures/`. See docs/subsystems/cutscene.md,
"Record pacing - the 60 Hz sub-clock".
"""

from __future__ import annotations

import argparse
import csv
import statistics

# CPU cycles a scanline: 3413 video cycles * 7 / 11.
HBLANK_CPU = 3413 * 7 / 11


def frames(path: str) -> list[tuple[int, float, float]]:
    """`(vsync, cpu hblanks, CD interrupt hblanks)` per frame."""
    out, cur, done, cd, exc = [], None, None, 0, None
    with open(path) as f:
        for r in csv.DictReader(f):
            kind, cyc = r["kind"], int(r["cycle"])
            if kind == "vsyncret":
                if cur is not None and done is not None:
                    out.append((cur[0], (done - cur[1]) / HBLANK_CPU, cd / HBLANK_CPU))
                cur, done, cd = (int(r["vsync"]), cyc), None, 0
            elif kind == "exc":
                exc = cyc
            elif kind == "irq_end" and exc is not None:
                cd += cyc - exc
                exc = None
            elif kind == "cpudone":
                done = cyc
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("csv")
    ap.add_argument("--from", dest="lo", type=int, default=0)
    ap.add_argument("--to", dest="hi", type=int, default=1 << 30)
    a = ap.parse_args()
    fs = [f for f in frames(a.csv) if a.lo <= f[0] <= a.hi]
    if not fs:
        print("no frames in range")
        return 1
    for v, cpu, cd in fs:
        print(f"v{v:6d}  cpu {cpu:7.1f}h  cd {cd:7.1f}h  work {cpu - cd:6.1f}h")
    ratios = [cpu / (cpu - cd) for _, cpu, cd in fs if cpu > cd]
    pooled = sum(c for _, c, _ in fs) / sum(c - d for _, c, d in fs)
    print(f"{len(fs)} frames: work median {statistics.median(c - d for _, c, d in fs):.1f}h, "
          f"cpu/work median {statistics.median(ratios):.3f}, pooled {pooled:.3f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
