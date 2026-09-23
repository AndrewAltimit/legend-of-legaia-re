#!/usr/bin/env python3
"""Offline half of `autorun_w1a_audio_clock.lua`.

Three questions the per-vsync audio trace can be asked once the capture
carries a wall-clock stamp per frame:

``hist``
    Per-voice envelope-level histogram on one trace, plus the note statistics
    that do *not* depend on how long a voice rings: key-on edges per frame and
    the run length each key-on produces.  The histogram is what separates
    "drained voices are being counted as sounding" (mass at the bottom decile)
    from "the voices really are sounding" (mass at the top).

``clock``
    Regress the per-frame envelope decay against the measured wall-clock gap
    between captures.  PCSX-Redux's SPU runs on its own thread, paced by the
    audio device rather than by the emulated CPU, so a capture that writes a
    ~19 MiB save state per vsync advances the envelope by several real frames
    per captured frame.  Under that model the decay grows with the gap; under
    "the envelope is on emulated time" it is flat.

``twoway``
    Compare two captures of the SAME save state with no pad input.  The
    emulated CPU state at vsync *i* is identical by construction, so every SPU
    *register* write is the same; whatever differs at the same vsync belongs to
    the host-side SPU thread.  This is the assumption-free version of the
    question ``clock`` answers by regression.

Inputs are the JSONL files `extract_audio_trace_from_sstates.py` writes and
the `<out>.clock.csv` sidecar the probe writes beside its binary stream.

Usage:
    python3 scripts/pcsx-redux/analyze_audio_clock.py hist    TRACE.jsonl
    python3 scripts/pcsx-redux/analyze_audio_clock.py clock   TRACE.jsonl CLOCK.csv
    python3 scripts/pcsx-redux/analyze_audio_clock.py twoway  A.jsonl B.jsonl
"""

from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
from collections import defaultdict
from pathlib import Path

NUM_VOICES = 24
ENV_FULL = 32767
# Exponential-decrease step for an ADSR release at shift 12: one step of
# -8 * level / 32768 every two samples. Used only to turn an observed decay
# into an "implied samples per captured frame" figure.
SHIFT12_PER_SAMPLE = 8.0 / 32768.0 / 2.0


def load(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.open() if line.strip()]


def sounding(frame: dict) -> list[dict]:
    return [v for v in frame.get("voices", []) if v.get("active")]


def cmd_hist(args: argparse.Namespace) -> int:
    frames = load(args.trace)
    levels: list[int] = []
    per_frame: list[int] = []
    buckets: dict[int, int] = defaultdict(int)
    for f in frames:
        s = sounding(f)
        if not s:
            continue
        per_frame.append(len(s))
        for v in s:
            e = int(v.get("env_level", 0))
            levels.append(e)
            buckets[min(int(e / ENV_FULL * 10), 9)] += 1

    # Key-on edges and the run length each produces.
    prev = [False] * NUM_VOICES
    cur = [0] * NUM_VOICES
    runs: list[int] = []
    onsets = 0
    for f in frames:
        vs = f.get("voices", [])
        for i in range(min(NUM_VOICES, len(vs))):
            a = bool(vs[i].get("active"))
            if a and not prev[i]:
                onsets += 1
                cur[i] = 1
            elif a:
                cur[i] += 1
            elif prev[i]:
                runs.append(cur[i])
                cur[i] = 0
            prev[i] = a
    runs.extend(c for c in cur if c)

    print(f"{args.trace.name}: {len(frames)} frames, {len(per_frame)} with voices sounding")
    if per_frame:
        print(f"  sounding voices: mean {statistics.mean(per_frame):.3f} max {max(per_frame)}")
    print(f"  key-ons: {onsets} ({onsets / max(len(frames), 1):.3f}/frame)")
    if runs:
        print(f"  frames sounding per key-on: mean {statistics.mean(runs):.2f} "
              f"median {statistics.median(runs)} max {max(runs)}")
    print(f"  voice-frames: {len(levels)}")
    for b in range(10):
        n = buckets.get(b, 0)
        print(f"    env in [{b / 10:.1f},{(b + 1) / 10:.1f}) : {n:6d}"
              f"  {100.0 * n / max(len(levels), 1):5.1f}%")
    return 0


def read_clock(path: Path) -> dict[int, int]:
    out: dict[int, int] = {}
    for i, line in enumerate(path.open()):
        if i == 0:
            continue
        vsync, _mono, delta = line.strip().split(",")
        out[int(vsync)] = int(delta)
    return out


def cmd_clock(args: argparse.Namespace) -> int:
    frames = load(args.trace)
    delta = read_clock(args.clock)
    pairs: list[tuple[int, float]] = []
    for a, b in zip(frames, frames[1:]):
        d = delta.get(b.get("frame", -1), 0)
        if d <= 0:
            continue
        for va, vb in zip(a.get("voices", []), b.get("voices", [])):
            if not (va.get("active") and vb.get("active")):
                continue
            if va.get("start_addr") != vb.get("start_addr"):
                continue
            if va.get("pitch") != vb.get("pitch"):
                continue
            ea, eb = int(va.get("env_level", 0)), int(vb.get("env_level", 0))
            # Well away from the floor: the `>> 15` in the hardware's
            # exponential decrease turns the tail linear at -1 per step, which
            # is not a ratio any more.
            if ea < 4000 or eb <= 0 or eb >= ea:
                continue
            pairs.append((d, -math.log(eb / ea)))

    if not pairs:
        print("no decaying voice-frame pairs")
        return 0
    gaps = [p[0] / 1e6 for p in pairs]
    decay = [p[1] for p in pairs]
    print(f"{len(pairs)} decaying voice-frame pairs")
    print(f"  wall-clock gap: mean {statistics.mean(gaps):.1f} ms "
          f"(emulated frame = 16.67 ms)")
    pairs.sort()
    q = len(pairs) // 4
    for k in range(4):
        chunk = pairs[k * q:(k + 1) * q if k < 3 else len(pairs)]
        gm = statistics.mean(p[0] for p in chunk) / 1e6
        dm = statistics.mean(p[1] for p in chunk)
        print(f"  gap quartile {k + 1}: {gm:7.1f} ms -> mean decay {dm:.4f}"
              f"  (shift-12 tone would imply {dm / SHIFT12_PER_SAMPLE:7.0f} samples/frame)")
    mg, md = statistics.mean(gaps), statistics.mean(decay)
    cov = sum((x - mg) * (y - md) for x, y in zip(gaps, decay))
    vg = sum((x - mg) ** 2 for x in gaps)
    vd = sum((y - md) ** 2 for y in decay)
    r = cov / math.sqrt(vg * vd) if vg and vd else 0.0
    print(f"  Pearson r(gap, decay) = {r:.3f}   (emulated-time model predicts ~0)")
    return 0


def cmd_twoway(args: argparse.Namespace) -> int:
    a = {f.get("frame"): f for f in load(args.a)}
    b = {f.get("frame"): f for f in load(args.b)}
    common = sorted(set(a) & set(b))
    print(f"{len(a)} vs {len(b)} frames, {len(common)} shared vsync indices")
    sa = [len(sounding(a[i])) for i in common]
    sb = [len(sounding(b[i])) for i in common]
    nza = [x for x in sa if x]
    nzb = [x for x in sb if x]
    if nza and nzb:
        print(f"  mean sounding voices: A {statistics.mean(nza):.3f}"
              f"  B {statistics.mean(nzb):.3f}")
    same_mask = sum(1 for i in common
                    if a[i].get("active_voice_mask") == b[i].get("active_voice_mask"))
    print(f"  identical active_voice_mask: {same_mask}/{len(common)} vsyncs")
    pitch_same = pitch_tot = env_same = env_tot = 0
    diffs: list[int] = []
    for i in common:
        for va, vb in zip(a[i].get("voices", []), b[i].get("voices", [])):
            if va.get("pitch") is not None and vb.get("pitch") is not None:
                pitch_tot += 1
                pitch_same += int(va["pitch"] == vb["pitch"])
            ea, eb = va.get("env_level"), vb.get("env_level")
            if ea is not None and eb is not None and (ea or eb):
                env_tot += 1
                if ea == eb:
                    env_same += 1
                else:
                    diffs.append(abs(ea - eb))
    print(f"  identical voice pitch register: {pitch_same}/{pitch_tot}"
          "   (written by the emulated CPU)")
    print(f"  identical env_level: {env_same}/{env_tot}"
          f"   median |diff| {statistics.median(diffs) if diffs else 0} of {ENV_FULL}")
    return 0


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    h = sub.add_parser("hist", help="envelope histogram + key-on statistics")
    h.add_argument("trace", type=Path)
    h.set_defaults(fn=cmd_hist)
    c = sub.add_parser("clock", help="regress envelope decay against the wall-clock gap")
    c.add_argument("trace", type=Path)
    c.add_argument("clock", type=Path)
    c.set_defaults(fn=cmd_clock)
    t = sub.add_parser("twoway", help="two captures of the same state, no input")
    t.add_argument("a", type=Path)
    t.add_argument("b", type=Path)
    t.set_defaults(fn=cmd_twoway)
    args = ap.parse_args(argv)
    return args.fn(args)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
