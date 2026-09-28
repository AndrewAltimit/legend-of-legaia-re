#!/usr/bin/env python3
"""Check autorun_gated_clip_step.lua's CSV against FUN_800204F8's step rule.

For every captured clip-tick call that neither rebound the clip (+0x5C ==
+0x5E), held it (+0x62 & 2), restarted it (+0x62 & 0x200) nor ran it in
reverse (+0x62 & 0x80), predict the cursor after the call:

    step = (rate*2 + div - 1) // div   if the clip's gate (byte +1 bit 0)
         = rate                        otherwise
    cursor += step * frame_step        (frame_step = DAT_1F800393)
    at or past frames*16 - 1: 0 (loop) or frames*16 - 1 (clamp, +0x62 & 8)

and count matches per (gate, divisor, rate, frame step).

    python3 scripts/pcsx-redux/analyze_gated_clip_step.py <gated_clip_step.csv>
"""
import collections
import csv
import sys


def main(path):
    rows = list(csv.DictReader(open(path)))
    stat = collections.Counter()
    for r in rows:
        flags = int(r["flags62"], 16)
        if r["id5c"] != r["id5e"] or int(r["id5c"]) <= 0 or r["gate"] == "-1":
            continue
        if flags & (0x2 | 0x200 | 0x80):
            continue
        cin, cout = int(r["cursor_in"]), int(r["cursor_out"])
        span = int(r["frames"]) * 16
        gate, div, rate = int(r["gate"]), int(r["div"]), int(r["rate"])
        mult = int(r["step393"])
        step = (rate * 2 + div - 1) // div if gate and div else rate
        want = cin + step * mult
        if want >= span - 1:
            want = span - 1 if flags & 0x8 else 0
        key = (gate, div if gate else "-", rate, mult)
        stat[(key, want == cout)] += 1
    print("actors", len({r["actor"] for r in rows}), "calls", len(rows))
    for (key, ok), n in sorted(stat.items(), key=str):
        gate, div, rate, mult = key
        print(f"gate={gate} div={div} rate={rate} frame_step={mult} "
              f"{'match' if ok else 'MISMATCH'}: {n}")


if __name__ == "__main__":
    main(sys.argv[1])
