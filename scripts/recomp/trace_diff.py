#!/usr/bin/env python3
"""Align two canonical state-trace JSONL files and report first divergences.

Both inputs use the canonical shape emitted by ``trace_capture.py`` (recomp
side) and ``legaia-engine sim-trace`` (engine side): one JSON object per
line, ``frame`` required, everything else optional per line.

Alignment: trace B's frame ``f`` is compared against trace A's frame
``f - offset``. The offset comes from ``--offset`` or, by default, from
auto-alignment on the first camera change in each trace (the first frame
whose ``cam`` tuple differs from that trace's initial ``cam`` tuple) -
scene entries start both sides from an arbitrary frame counter, but the
first scripted camera cut is the same event on both.

Comparison: per-channel, over the aligned overlap. Channels are dotted
paths (``cam.yaw``, ``player.x``, ``actors[2].heading``, ``scene``,
``mode``). Angle channels compare with 4096-wraparound distance; position
channels with absolute distance; ``scene`` / ``mode`` exactly. The FIRST
divergent frame per channel is reported with a +/-5-frame context window.
Exit status: non-zero when any channel diverged.
"""

from __future__ import annotations

import argparse
import json
import sys

ANGLE_CHANNELS = {"cam.pitch", "cam.yaw", "cam.roll", "player.heading"}
ANGLE_MOD = 4096
CONTEXT = 5


def load_trace(path: str) -> dict[int, dict]:
    """Read a JSONL trace into ``{frame: record}``. Later duplicates of a
    frame win (a re-capture appended to the same file)."""
    frames: dict[int, dict] = {}
    with open(path) as f:
        for lineno, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            try:
                rec = json.loads(line)
            except json.JSONDecodeError as e:
                raise SystemExit(f"{path}:{lineno}: bad JSON: {e}")
            if "frame" not in rec:
                raise SystemExit(f"{path}:{lineno}: record missing 'frame'")
            frames[int(rec["frame"])] = rec
    if not frames:
        raise SystemExit(f"{path}: empty trace")
    return frames


def flatten(rec: dict) -> dict[str, object]:
    """Flatten one record into dotted channels."""
    out: dict[str, object] = {}
    if "scene" in rec:
        out["scene"] = rec["scene"]
    if "mode" in rec:
        out["mode"] = rec["mode"]
    cam = rec.get("cam") or {}
    for k in ("pitch", "yaw", "roll", "h"):
        if k in cam:
            out[f"cam.{k}"] = cam[k]
    for trio in ("eye", "focus"):
        if trio in cam:
            for axis, v in zip("xyz", cam[trio]):
                out[f"cam.{trio}.{axis}"] = v
    player = rec.get("player") or {}
    for k in ("x", "z", "heading"):
        if k in player:
            out[f"player.{k}"] = player[k]
    for actor in rec.get("actors") or []:
        i = actor.get("i")
        for k in ("x", "z", "heading"):
            if k in actor:
                out[f"actors[{i}].{k}"] = actor[k]
    return out


def is_angle(channel: str) -> bool:
    return channel in ANGLE_CHANNELS or channel.endswith(".heading")


def channel_distance(channel: str, a, b):
    """Numeric distance for tolerance channels; ``None`` for exact-match
    channels (returns 0/1 mismatch instead)."""
    if isinstance(a, str) or isinstance(b, str) or channel in ("scene",):
        return None if a == b else float("inf")
    if channel == "mode":
        return None if a == b else float("inf")
    d = abs(a - b)
    if is_angle(channel):
        d = min(d, ANGLE_MOD - d)
    return d


def tolerance_for(channel: str, args) -> float:
    if channel in ("scene", "mode"):
        return 0.0
    if is_angle(channel):
        return args.tol_angle
    if channel == "cam.h":
        return args.tol_h
    return args.tol_pos


def first_cam_change_frame(frames: dict[int, dict]) -> int | None:
    """Frame of the first change in the cam tuple, or None when the trace
    has no cam channel / never changes."""
    order = sorted(frames)
    initial = None
    for f in order:
        cam = frames[f].get("cam")
        if cam is None:
            continue
        key = json.dumps(cam, sort_keys=True)
        if initial is None:
            initial = key
        elif key != initial:
            return f
    return None


def auto_offset(a: dict[int, dict], b: dict[int, dict]) -> int | None:
    fa = first_cam_change_frame(a)
    fb = first_cam_change_frame(b)
    if fa is None or fb is None:
        return None
    return fb - fa


def fmt_val(v) -> str:
    return json.dumps(v)


def diff_traces(a, b, offset: int, args, out=sys.stdout) -> int:
    """Compare aligned traces; print report; return count of divergent
    channels."""
    # Aligned overlap: frames f in B such that (f - offset) in A.
    overlap = sorted(f for f in b if (f - offset) in a)
    if not overlap:
        print(f"no aligned overlap at offset {offset}", file=out)
        return 1

    flat_a = {f: flatten(a[f - offset]) for f in overlap}
    flat_b = {f: flatten(b[f]) for f in overlap}

    channels = sorted(
        set.union(*(set(v.keys()) for v in flat_a.values()))
        & set.union(*(set(v.keys()) for v in flat_b.values()))
    )
    only_a = sorted(
        set.union(*(set(v.keys()) for v in flat_a.values())) - set(channels)
    )
    only_b = sorted(
        set.union(*(set(v.keys()) for v in flat_b.values())) - set(channels)
    )

    print(
        f"aligned {len(overlap)} frames (offset {offset:+d}; "
        f"A frame = B frame {-offset:+d})",
        file=out,
    )
    if only_a:
        print(f"channels only in trace A (skipped): {', '.join(only_a)}", file=out)
    if only_b:
        print(f"channels only in trace B (skipped): {', '.join(only_b)}", file=out)

    divergent = 0
    for ch in channels:
        tol = tolerance_for(ch, args)
        first_bad = None
        for f in overlap:
            va = flat_a[f].get(ch)
            vb = flat_b[f].get(ch)
            if va is None or vb is None:
                continue  # channel absent on this line on one side
            d = channel_distance(ch, va, vb)
            bad = (va != vb) if d is None else (d > tol)
            if bad:
                first_bad = f
                break
        if first_bad is None:
            print(f"  OK   {ch} (tol {tol:g})", file=out)
            continue
        divergent += 1
        print(f"  DIVERGE {ch} at B frame {first_bad} (tol {tol:g}):", file=out)
        lo = first_bad - CONTEXT
        hi = first_bad + CONTEXT
        for f in overlap:
            if f < lo or f > hi:
                continue
            va = flat_a[f].get(ch)
            vb = flat_b[f].get(ch)
            marker = " <-- first divergence" if f == first_bad else ""
            print(
                f"    B frame {f} (A frame {f - offset}): "
                f"A={fmt_val(va)} B={fmt_val(vb)}{marker}",
                file=out,
            )
    print(
        f"{divergent}/{len(channels)} shared channels diverged",
        file=out,
    )
    return divergent


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("trace_a", help="reference trace JSONL (e.g. recomp capture)")
    ap.add_argument("trace_b", help="candidate trace JSONL (e.g. engine sim-trace)")
    ap.add_argument(
        "--offset",
        type=int,
        help="explicit frame offset (B frame = A frame + offset); default: "
        "auto-align on the first camera change, falling back to aligning "
        "the two traces' first frames",
    )
    ap.add_argument(
        "--tol-angle",
        type=float,
        default=0.0,
        help="tolerance for 12-bit angle channels, with 4096 wraparound (default 0)",
    )
    ap.add_argument(
        "--tol-pos",
        type=float,
        default=0.0,
        help="tolerance for position channels, world units (default 0)",
    )
    ap.add_argument(
        "--tol-h", type=float, default=0.0, help="tolerance for cam.h (default 0)"
    )
    args = ap.parse_args(argv)

    a = load_trace(args.trace_a)
    b = load_trace(args.trace_b)

    if args.offset is not None:
        offset = args.offset
        how = "explicit"
    else:
        offset = auto_offset(a, b)
        how = "auto (first camera change)"
        if offset is None:
            offset = min(b) - min(a)
            how = "fallback (first frames aligned)"
    print(f"offset {offset:+d} [{how}]")

    divergent = diff_traces(a, b, offset, args)
    return 1 if divergent else 0


if __name__ == "__main__":
    sys.exit(main())
