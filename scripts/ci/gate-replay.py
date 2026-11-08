#!/usr/bin/env python3
"""Gate replay: one engine run's pad on retail, resynchronised at state gates.

The engine side is a `LEGAIA_DIAG_TICK_DIGEST` run (full_game_ladder with
LEGAIA_FGL_ONLY, or any `BootSession`). The run is cut into beats at gates: the
player engaged / released, scene changes, warps, battle entry and exit, dialog
page ends, the walk's turns (position waypoints) and, inside a battle, the
round prompt and each action's start. `autorun_gate_replay.lua` replays each
beat's pad relative to the beat's opening gate on the state named by
`scenario`, and waits for retail at the next gate rather than holding one
clock. Report only; output under captures/ (retail-derived, never committed).
See docs/tooling/gate-replay.md.
"""
import argparse
import csv
import os
import re
import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
WARP_DIST = 300
WAYPOINT_MIN_TRAVEL = 96
WAYPOINT_SETTLE = 8
DPAD = 0x00F0
AXES = ("+x", "-x", "+z", "-z")
PAGE_WAIT = "19"
ACT_GATES = ("0a",)
FLOW_GATES = {"TurnPrompt": "1e"}
LCG_A, LCG_C, LCG_MAX = 0x41C64E6D, 0x3039, 200_000
FIELD_KV = re.compile(rb" (\w+)=(\S+)")


def session_offsets(path):
    """Byte offset where each session starts (the frame counter restarting)."""
    offs, prev = [], None
    with open(path, "rb") as f:
        pos = 0
        for line in f:
            _, frame, _ = line.split(b" ", 2)
            frame = int(frame)
            if prev is None or frame < prev:
                offs.append(pos)
            prev, pos = frame, pos + len(line)
    return offs


def load_session(path, index):
    """The rows of session `index` (default the last) as (tick, kv dict)."""
    offs = session_offsets(path)
    start = offs[index]
    end = offs[index + 1] if index % len(offs) != len(offs) - 1 else None
    rows = []
    with open(path, "rb") as f:
        f.seek(start)
        pos = start
        for line in f:
            if end is not None and pos >= end:
                break
            pos += len(line)
            head, _, _ = line.partition(b" |")
            parts = head.split(b" ")
            rows.append(
                {
                    "mode": parts[2].decode(),
                    **{k.decode(): v.decode() for k, v in FIELD_KV.findall(head)},
                }
            )
    return rows, len(offs)


def tick_int(hexstr):
    return int(hexstr, 16)


def cut_gates(rows, min_dwell=3):
    """(tick, kind, value) in order. Field state edges are ignored in battle, and
    an engaged run shorter than `min_dwell` ticks (a one-tick script pulse) is not
    a gate: retail's bit moves on its pass grid and those never pair."""
    gates = []
    prev = rows[0]
    for t, row in enumerate(rows[1:], 1):
        was_battle, is_battle = prev["mode"] == "Battle", row["mode"] == "Battle"
        if is_battle != was_battle:
            gates.append((t, "battle", "1" if is_battle else "0"))
        if is_battle and was_battle:
            if row["st"] in ACT_GATES and row["st"] != prev["st"]:
                gates.append((t, "act", row["st"]))
            if row["flow"] in FLOW_GATES and row["flow"] != prev["flow"]:
                gates.append((t, "flow", FLOW_GATES[row["flow"]]))
        if not is_battle and not was_battle:
            if row["pg"] == PAGE_WAIT and prev["pg"] != PAGE_WAIT:
                gates.append((t, "page", ""))
            if row["eng"] != prev["eng"]:
                gates.append((t, "engaged", row["eng"]))
            if row["sc"] != prev["sc"]:
                gates.append((t, "scene", row["sc"]))
            if row["pl"] != "-" and prev["pl"] != "-":
                ax, _, az = map(int, row["pl"].split(","))
                bx, _, bz = map(int, prev["pl"].split(","))
                if abs(ax - bx) + abs(az - bz) > WARP_DIST:
                    gates.append((t, "warp", "-"))
        prev = row
    return number_pages(drop_pulses(gates, min_dwell))


def number_pages(gates):
    """Each page gate's value: its ordinal since the last state gate. An act or
    flow gate's: its ordinal within this battle, as `XX:N`."""
    out, n, acts = [], 0, {}
    for t, kind, value in gates:
        if kind == "page":
            n += 1
            value = str(n)
        elif kind in ("act", "flow"):
            acts[kind, value] = acts.get((kind, value), 0) + 1
            value = f"{value}:{acts[kind, value]}"
        else:
            n = 0
            if kind == "battle":
                acts = {}
        out.append((t, kind, value))
    return out


def drop_pulses(gates, min_dwell):
    """Drop consecutive engaged edges closer than `min_dwell`, pairwise."""
    out = []
    for g in gates:
        if g[1] == "engaged":
            eng = [j for j, h in enumerate(out) if h[1] == "engaged"]
            if eng and g[0] - out[eng[-1]][0] < min_dwell and out[eng[-1]][2] != g[2]:
                out.pop(eng[-1])
                continue
        out.append(g)
    return out


def waypoints(rows, gates, until):
    """`pos` gates at the walk's turns: a d-pad change to another held direction
    that stays WAYPOINT_SETTLE ticks, WAYPOINT_MIN_TRAVEL from the last turn.
    Only in stretches the player is free to walk; pad timing alone cannot hold
    a walk on the engine's path, so the turns are gated on position."""
    closed = sorted({g[0] for g in gates} | {until})
    out = []
    last_pos, last_dpad = None, 0
    gate_i = 0
    for t in range(until):
        row = rows[t]
        while gate_i < len(closed) - 1 and closed[gate_i] <= t:
            gate_i += 1
            last_pos = None
        if row["mode"] != "Field" or row["eng"] != "0" or row["pl"] == "-":
            last_dpad = 0
            continue
        x, _, z = map(int, row["pl"].split(","))
        if last_pos is None:
            last_pos = (x, z)
        dpad = tick_int(row["pad"]) & DPAD
        if dpad != last_dpad:
            settled = t + WAYPOINT_SETTLE < until and all(
                tick_int(rows[t + k]["pad"]) & DPAD == dpad for k in range(WAYPOINT_SETTLE)
            )
            near_gate = closed[gate_i] - t < WAYPOINT_SETTLE
            moved = abs(x - last_pos[0]) + abs(z - last_pos[1]) >= WAYPOINT_MIN_TRAVEL
            if settled and moved and not near_gate and last_dpad != 0 and dpad != 0:
                out.append((t, "pos", f"{x}/{z}"))
                last_pos = (x, z)
            last_dpad = dpad
    return sorted([*gates, *out], key=lambda g: g[0])


def dpad_axes(rows, start, end):
    """Per world axis direction, the d-pad bit the engine walked it with (the
    camera remaps the pad scene by scene, so this is read off the run)."""
    votes = {}
    for t in range(max(start, 1), min(end, len(rows))):
        row = rows[t]
        if row["mode"] != "Field" or row["pl"] == "-" or rows[t - 1]["pl"] == "-":
            continue
        bit = tick_int(row["pad"]) & DPAD
        if bit not in (0x10, 0x20, 0x40, 0x80):
            continue
        ax, _, az = map(int, rows[t - 1]["pl"].split(","))
        bx, _, bz = map(int, row["pl"].split(","))
        dx, dz = bx - ax, bz - az
        if 0 < abs(dx) + abs(dz) < 64:
            axis = ("+x" if dx > 0 else "-x") if abs(dx) > abs(dz) else ("+z" if dz > 0 else "-z")
            votes.setdefault(axis, {}).setdefault(bit, 0)
            votes[axis][bit] += 1
    return {k: max(votes[k], key=votes[k].get) if k in votes else 0 for k in AXES}


def seek_spot(rows, kind, value, t):
    """The engine's standing spot when a gate a walk triggers fired, or None."""
    if kind not in ("engaged", "warp", "scene", "battle", "page") or (kind == "engaged" and value == "0"):
        return None
    prev = rows[t - 1]
    if prev["mode"] != "Field" or prev["eng"] != "0" or prev["pl"] == "-":
        return None
    x, _, z = prev["pl"].split(",")
    return x, z


def stretched_masks(rows, until, min_press):
    """The per-tick pad words, each button press held at least `min_press` ticks
    (a press one tick long can fall between two of retail's game passes)."""
    masks = [tick_int(r["pad"]) for r in rows[:until]]
    if min_press <= 1:
        return masks
    out = masks[:]
    for bit in range(16):
        b = 1 << bit
        for t, m in enumerate(masks):
            if m & b and (t == 0 or not masks[t - 1] & b):
                run = 1
                while t + run < until and masks[t + run] & b:
                    run += 1
                for k in range(run, min(min_press, until - t)):
                    out[t + k] |= b
    return out


def write_plan(path, rows, gates, until, min_press=1):
    """The Lua plan: BEAT lines, each followed by its PAD rows."""
    gates = [g for g in gates if g[0] < until]
    axes = dpad_axes(rows, 0, until)
    masks = stretched_masks(rows, until, min_press)
    beats = []
    start = 0
    with open(path, "w") as f:
        for i, (t, kind, value) in enumerate([*gates, (until, "end", "-")]):
            f.write(f"BEAT {i} {kind} {value} {t - start}\n")
            seek = seek_spot(rows, kind, value, t)
            if seek:
                near = dpad_axes(rows, start, t)
                f.write(f"SEEK {seek[0]}/{seek[1]} {' '.join(str(near[k] or axes[k]) for k in AXES)}\n")
            prev = None
            for tick in range(start, t):
                mask = masks[tick]
                if mask != prev:
                    f.write(f"PAD {tick - start} {mask:#06x}\n")
                prev = mask
            beats.append((start, t, kind, value))
            start = t
    return beats


def lcg_distance(a, b):
    n = 0
    while a != b and n < LCG_MAX:
        a = (a * LCG_A + LCG_C) & 0xFFFFFFFF
        n += 1
    return n if a == b else None


def draws(seq):
    """Per adjacent pair of a rng-value sequence: draws, or a count of reseeds."""
    total, jumps = 0, 0
    for a, b in zip(seq, seq[1:]):
        if a == b:
            continue
        n = lcg_distance(a, b)
        if n is None:
            jumps += 1
        else:
            total += n
    return f"{total}" + (f"+{jumps}j" if jumps else "")


def read_csv(path):
    with open(path) as f:
        return list(csv.DictReader(f))


def table(rows, beats, gates_csv, events_csv, retail_rows):
    by_vsync = {int(r["vsync"]): r for r in retail_rows}
    hdr = ["beat", "gate", "E", "R", "gap", "wait", "drop", "pos diff", "E rng", "R rng", "synth", "status"]
    out = [hdr]
    for b in gates_csv:
        i = int(b["beat"])
        start, end, kind, value = beats[i]
        el, vs, ve = end - start, int(b["vsync_start"]), int(b["vsync_end"])
        erng = draws([tick_int(rows[t]["rng"]) for t in range(start, min(end + 1, len(rows)))])
        rrng = draws([int(by_vsync[v]["rng"], 16) for v in range(vs, ve + 1) if v in by_vsync])
        rp = (int(b["x"]), int(b["z"]))
        ep = rows[end]["pl"] if end < len(rows) else "-"
        in_battle = end < len(rows) and rows[end]["mode"] == "Battle"
        if rp[0] == -32768 or ep == "-" or in_battle or b["mode"] != "3":
            diff = "-"
        else:
            ex, _, ez = map(int, ep.split(","))
            diff = f"{rp[0] - ex},{rp[1] - ez}"
        gate = f"{kind}={value}" if value != "-" else kind
        out.append([str(i), gate, str(el), str(ve - vs), f"{ve - vs - el:+d}", b["waited"],
                    b["dropped_presses"], diff, erng, rrng, b["synthetic"], b["status"]])
    widths = [max(len(r[c]) for r in out) for c in range(len(hdr))]
    for r in out:
        print("  ".join(v.rjust(w) for v, w in zip(r, widths)))
    if events_csv:
        print("events (synthetic input and timeouts):")
        for e in events_csv:
            print(f"  beat {e['beat']} vsync {e['vsync']}: {e['event']} {e['detail']}")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("scenario", help="retail state (scripts/scenarios.toml label)")
    ap.add_argument("--digest", required=True, help="LEGAIA_DIAG_TICK_DIGEST file")
    ap.add_argument("--session", type=int, default=-1, help="session in the digest (default the last)")
    ap.add_argument("--until", type=int, help="engine tick the replay ends on (default the session's end)")
    ap.add_argument("--out", help="output directory (default captures/gate-replay/<scenario>)")
    ap.add_argument("--iso", default=os.environ.get("LEGAIA_DISC_BIN"))
    ap.add_argument("--max-wait", type=int, default=1800, help="vsyncs a beat may wait")
    ap.add_argument("--synth-after", type=int, default=90, help="vsyncs of waiting before a synthetic Cross, 0 for none")
    ap.add_argument("--min-dwell", type=int, default=3, help="engaged runs shorter than this are not gates")
    ap.add_argument("--min-press", type=int, default=2, help="hold every press at least N ticks (1 = the log as written)")
    ap.add_argument("--no-waypoints", action="store_true", help="beat only on state gates, not the walk's turns")
    ap.add_argument("--no-retail", action="store_true", help="reuse the retail side")
    a = ap.parse_args()
    out = Path(a.out or REPO / "captures" / "gate-replay" / a.scenario)
    out.mkdir(parents=True, exist_ok=True)
    rows, n_sessions = load_session(a.digest, a.session)
    until = min(a.until or len(rows), len(rows))
    gates = cut_gates(rows[:until], a.min_dwell)
    gates = gates if a.no_waypoints else waypoints(rows, gates, until)
    plan = out / "plan.txt"
    beats = write_plan(plan, rows, gates, until, a.min_press)
    print(f"{a.scenario}: session {a.session} of {n_sessions}, {until} engine ticks, {len(gates)} gates, presses held >= {a.min_press} ticks")
    if not a.no_retail:
        frames = int(until * 1.5) + a.max_wait * 2
        timeout = max(180, frames // 25)
        env = dict(os.environ, LEGAIA_GATE_PLAN=str(plan), LEGAIA_GATE_MAX_WAIT=str(a.max_wait),
                   LEGAIA_GATE_SYNTH_AFTER=str(a.synth_after))
        subprocess.run(
            ["timeout", "--kill-after=5", str(timeout), "bash", str(REPO / "scripts/pcsx-redux/run_probe.sh"),
             "--fast", "--headless", "--iso", a.iso, "--scenario", a.scenario,
             "--lua", str(REPO / "scripts/pcsx-redux/autorun_gate_replay.lua"),
             "--frames", str(frames), "--out-dir", str(out)],
            cwd=REPO, env=env, check=False,
        )
    beat_rows = read_csv(out / "gate_beats.csv")
    retail_rows = read_csv(out / "gate_replay.csv")
    events = read_csv(out / "gate_events.csv")
    table(rows, beats, beat_rows, events, retail_rows)
    if len(beat_rows) < len(beats):
        print(f"retail closed {len(beat_rows)} of {len(beats)} beats")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
