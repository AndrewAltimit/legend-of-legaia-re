#!/usr/bin/env python3
"""Summarise a `state_poll.csv` capture into a progression timeline.

The community poll probe (`autorun_state_poll.lua`) writes one CSV per run with
the schema:

    tick,kind,idx,value,delta,mode,scene,note

`kind` is one of: flagset / flagclr (story-flag bank 0x80085758, `idx` == bit
number), item (0x80084648 inventory; `idx` == item id, `value` == count),
gold (0x8008459C), party (0x80084594 count + 0x80084598 ids), scene / mode
(0x8007050C name + 0x8007B83C game-mode transitions). See the probe header for
the exact source addresses.

This tool turns that raw event log into the things a reverse-engineer actually
wants out of a playthrough capture:

  * a **scene timeline** (contiguous occupancy windows),
  * **battle windows** (game-mode dips into the battle-orbit modes),
  * a **story-flag census** that separates one-off story beats from the bulk
    flag dumps that a save-load / scene-init writes in a single frame, and
  * **item / gold / party** change lists.

Pure analysis lives in importable functions (`parse_rows`, `scene_timeline`,
`battle_windows`, `flag_census`, ...) so `test_analyze_state_poll.py` can drive
them on synthetic rows with no capture on disk. The CLI is a thin wrapper.

No Sony bytes are involved: this reads only the derived CSV event log.
"""
from __future__ import annotations

import argparse
import csv
import json
import sys
from dataclasses import dataclass, field
from pathlib import Path

# game-mode byte (0x8007B83C) values that mean "in the battle orbit". 0x14/0x15
# are the battle-scene load + active-battle modes; the surrounding 0x08/0x09 and
# 0x16/0x17 are field<->battle transition shims, not the fight itself.
BATTLE_MODES = {0x14, 0x15}
# a single tick that flips at least this many story flags is a bulk write (a
# save-file load or scene-init flag dump), not a story beat. Real beats move a
# handful of flags; a load moves 100+.
DEFAULT_BULK_THRESHOLD = 20
# scene label the probe emits before a real save/new-game is resolved.
BOOT_SCENE = "?"


@dataclass
class Row:
    tick: int
    kind: str
    idx: int
    value: int
    delta: int
    mode: int  # game-mode byte at the time of the row
    scene: str
    note: str


@dataclass
class SceneWindow:
    scene: str
    enter_tick: int
    exit_tick: int  # tick of the row that ended the window (or last tick seen)

    @property
    def duration(self) -> int:
        return self.exit_tick - self.enter_tick


@dataclass
class BattleWindow:
    enter_tick: int
    exit_tick: int
    scene: str

    @property
    def duration(self) -> int:
        return self.exit_tick - self.enter_tick


@dataclass
class BattleStart:
    tick: int
    scene: str
    staging_id: int  # best-effort 0x8007B7FC (often 0; consumed sub-vsync)
    formation: list[int]  # DAT_8007BD0C[0..3] first-monster ids
    enter_mode: int

    @property
    def is_lone(self) -> bool:
        """A single non-zero formation slot = a solo enemy = almost always a
        scripted boss (or a solo-strong random)."""
        return self.formation[0] != 0 and all(f == 0 for f in self.formation[1:])


@dataclass
class FlagBeat:
    idx: int
    set_tick: int
    scene: str
    churn: int  # total set+clr events for this flag across the gameplay window
    sticky: bool  # last event was a set (flag ends up set)


@dataclass
class FlagCensus:
    beats: list[FlagBeat] = field(default_factory=list)
    bulk_ticks: list[tuple[int, str, int]] = field(default_factory=list)  # (tick, scene, n_flags)


def parse_rows(lines) -> list[Row]:
    """Parse CSV lines (any iterable of strings, incl. an open file) into Rows.

    Tolerates the header line and blank/short lines. `idx`/`value`/`delta` are
    signed decimal; `mode` is a `0x..` hex byte.
    """
    out: list[Row] = []
    reader = csv.reader(lines)
    for parts in reader:
        if len(parts) < 7:
            continue
        if parts[0] == "tick":  # header
            continue
        try:
            tick = int(parts[0])
        except ValueError:
            continue
        kind = parts[1]
        try:
            idx = int(parts[2])
            value = int(parts[3])
            delta = int(parts[4])
        except ValueError:
            idx = value = delta = 0
        mode_s = parts[5]
        try:
            mode = int(mode_s, 16) if mode_s.lower().startswith("0x") else int(mode_s)
        except ValueError:
            mode = -1
        scene = parts[6]
        note = parts[7] if len(parts) > 7 else ""
        out.append(Row(tick, kind, idx, value, delta, mode, scene, note))
    return out


def load_csv(path: Path) -> list[Row]:
    with open(path, newline="") as fh:
        return parse_rows(fh)


def scene_timeline(rows: list[Row]) -> list[SceneWindow]:
    """Contiguous scene-occupancy windows.

    The `scene` column is present on every row, so we collapse runs of equal
    scene into windows. Boot-noise scene ('?') windows are kept but are easy to
    filter downstream.
    """
    windows: list[SceneWindow] = []
    cur: SceneWindow | None = None
    for r in rows:
        if cur is None or r.scene != cur.scene:
            if cur is not None:
                cur.exit_tick = r.tick
                windows.append(cur)
            cur = SceneWindow(scene=r.scene, enter_tick=r.tick, exit_tick=r.tick)
        else:
            cur.exit_tick = r.tick
    if cur is not None:
        windows.append(cur)
    return windows


def battle_windows(rows: list[Row]) -> list[BattleWindow]:
    """Windows where the game-mode byte was in a battle-orbit mode.

    Reads the `mode` column (stamped on every row) rather than only `kind==mode`
    rows, so a battle that spans many flag/item rows is still bracketed.
    """
    windows: list[BattleWindow] = []
    cur: BattleWindow | None = None
    for r in rows:
        in_battle = r.mode in BATTLE_MODES
        if in_battle and cur is None:
            cur = BattleWindow(enter_tick=r.tick, exit_tick=r.tick, scene=r.scene)
        elif in_battle and cur is not None:
            cur.exit_tick = r.tick
        elif not in_battle and cur is not None:
            cur.exit_tick = r.tick
            windows.append(cur)
            cur = None
    if cur is not None:
        windows.append(cur)
    return windows


def _parse_battle_note(note: str) -> tuple[list[int], int]:
    """Parse a `battle` row note `form=XXYYZZWW enter=0xMM` into (formation, mode).

    Tolerant of missing fields: returns ([0,0,0,0], -1) when unparseable.
    """
    formation = [0, 0, 0, 0]
    enter_mode = -1
    for tok in note.split():
        if tok.startswith("form=") and len(tok) >= 5 + 8:
            hexs = tok[5 : 5 + 8]
            try:
                formation = [int(hexs[i : i + 2], 16) for i in range(0, 8, 2)]
            except ValueError:
                pass
        elif tok.startswith("enter="):
            v = tok[len("enter=") :]
            try:
                enter_mode = int(v, 16) if v.lower().startswith("0x") else int(v)
            except ValueError:
                pass
    return formation, enter_mode


def battle_starts(rows: list[Row]) -> list[BattleStart]:
    """One `BattleStart` per `battle` row (the per-fight identity row).

    `idx` carries the best-effort staging id, `value` the first formation slot,
    and the note the full 4-id formation + enter mode.
    """
    out: list[BattleStart] = []
    for r in rows:
        if r.kind != "battle":
            continue
        formation, enter_mode = _parse_battle_note(r.note)
        # value column duplicates formation[0]; prefer the note-parsed one but
        # fall back to value if the note was malformed.
        if formation[0] == 0 and r.value != 0:
            formation[0] = r.value
        out.append(
            BattleStart(
                tick=r.tick,
                scene=r.scene,
                staging_id=r.idx,
                formation=formation,
                enter_mode=enter_mode,
            )
        )
    return out


def flag_census(rows: list[Row], bulk_threshold: int = DEFAULT_BULK_THRESHOLD) -> FlagCensus:
    """Separate one-off story-flag beats from bulk (load/init) flag dumps.

    A tick whose number of flag events (>= bulk_threshold) is treated as a bulk
    write (save-load / scene-init / new-game zero) and excluded from the beat
    list; it is reported separately. Remaining flag events are aggregated per
    flag idx: churn count, the last SET tick+scene, and whether the flag ends up
    set (sticky). Only sticky flags are returned as beats (a flag that ends
    cleared is not a durable progression marker).
    """
    # count flag events per tick to find bulk frames
    per_tick: dict[int, list[Row]] = {}
    for r in rows:
        if r.kind in ("flagset", "flagclr"):
            per_tick.setdefault(r.tick, []).append(r)

    bulk_ticks_set: set[int] = set()
    bulk_ticks: list[tuple[int, str, int]] = []
    for tick, evs in sorted(per_tick.items()):
        if len(evs) >= bulk_threshold:
            bulk_ticks_set.add(tick)
            bulk_ticks.append((tick, evs[0].scene, len(evs)))

    churn: dict[int, int] = {}
    last_set_tick: dict[int, int] = {}
    last_set_scene: dict[int, str] = {}
    last_kind: dict[int, str] = {}
    for r in rows:
        if r.kind not in ("flagset", "flagclr"):
            continue
        if r.tick in bulk_ticks_set:
            continue
        churn[r.idx] = churn.get(r.idx, 0) + 1
        last_kind[r.idx] = r.kind
        if r.kind == "flagset":
            last_set_tick[r.idx] = r.tick
            last_set_scene[r.idx] = r.scene

    beats: list[FlagBeat] = []
    for idx, n in churn.items():
        sticky = last_kind.get(idx) == "flagset"
        if not sticky:
            continue
        beats.append(
            FlagBeat(
                idx=idx,
                set_tick=last_set_tick[idx],
                scene=last_set_scene[idx],
                churn=n,
                sticky=True,
            )
        )
    beats.sort(key=lambda b: b.set_tick)
    return FlagCensus(beats=beats, bulk_ticks=bulk_ticks)


def item_changes(rows: list[Row]) -> list[Row]:
    """Item rows with a non-zero delta (pickups gain count, uses lose count)."""
    return [r for r in rows if r.kind == "item" and r.delta != 0]


def gold_changes(rows: list[Row], min_abs: int = 1) -> list[Row]:
    return [r for r in rows if r.kind == "gold" and abs(r.delta) >= min_abs]


def party_changes(rows: list[Row]) -> list[Row]:
    return [r for r in rows if r.kind == "party"]


def _fmt_flag(idx: int) -> str:
    return f"0x{idx:X} ({idx})"


def render_report(rows: list[Row], bulk_threshold: int, want: set[str]) -> str:
    lines: list[str] = []
    span = (rows[0].tick, rows[-1].tick) if rows else (0, 0)
    lines.append(f"# state_poll analysis  rows={len(rows)}  ticks {span[0]}..{span[1]}")

    if "scenes" in want:
        lines.append("\n## scene timeline")
        for w in scene_timeline(rows):
            if w.scene == BOOT_SCENE:
                continue
            lines.append(f"  {w.enter_tick:>7}..{w.exit_tick:<7} ({w.duration:>6}f)  {w.scene}")

    if "battles" in want:
        lines.append("\n## battle windows (mode in {0x14,0x15})")
        for b in battle_windows(rows):
            lines.append(f"  {b.enter_tick:>7}..{b.exit_tick:<7} ({b.duration:>5}f)  {b.scene}")
        starts = battle_starts(rows)
        if starts:
            lines.append("\n## battle starts (formation identity; * = lone enemy / likely boss)")
            for s in starts:
                form = "".join(f"{f:02X}" for f in s.formation)
                boss = " *" if s.is_lone else ""
                stage = f"  staged=0x{s.staging_id:02X}" if s.staging_id else ""
                lines.append(
                    f"  tick {s.tick:>7}  {s.scene:<8}  form={form}  enter=0x{s.enter_mode:02X}{stage}{boss}"
                )

    if "flags" in want:
        cen = flag_census(rows, bulk_threshold)
        lines.append(f"\n## bulk flag frames (>= {bulk_threshold} flags/tick = load/init)")
        for tick, scene, n in cen.bulk_ticks:
            lines.append(f"  tick {tick:>7}  scene {scene:<8}  {n} flags")
        lines.append("\n## story-flag beats (sticky, per-frame, load frames excluded)")
        for b in cen.beats:
            churn = "" if b.churn == 1 else f"  churn={b.churn}"
            lines.append(f"  tick {b.set_tick:>7}  {b.scene:<8}  flag {_fmt_flag(b.idx)}{churn}")

    if "items" in want:
        lines.append("\n## item changes (non-zero delta)")
        for r in item_changes(rows):
            sign = "+" if r.delta > 0 else ""
            lines.append(
                f"  tick {r.tick:>7}  {r.scene:<8}  id 0x{r.idx:02X}  count={r.value} ({sign}{r.delta})  {r.note}"
            )

    if "gold" in want:
        lines.append("\n## gold changes")
        for r in gold_changes(rows):
            sign = "+" if r.delta > 0 else ""
            lines.append(f"  tick {r.tick:>7}  {r.scene:<8}  gold={r.value} ({sign}{r.delta})")

    if "party" in want:
        lines.append("\n## party changes")
        for r in party_changes(rows):
            lines.append(f"  tick {r.tick:>7}  {r.scene:<8}  count={r.value}  {r.note}")

    return "\n".join(lines)


def build_json(rows: list[Row], bulk_threshold: int) -> dict:
    cen = flag_census(rows, bulk_threshold)
    return {
        "rows": len(rows),
        "tick_span": [rows[0].tick, rows[-1].tick] if rows else [0, 0],
        "scenes": [
            {"scene": w.scene, "enter": w.enter_tick, "exit": w.exit_tick, "duration": w.duration}
            for w in scene_timeline(rows)
            if w.scene != BOOT_SCENE
        ],
        "battles": [
            {"enter": b.enter_tick, "exit": b.exit_tick, "duration": b.duration, "scene": b.scene}
            for b in battle_windows(rows)
        ],
        "battle_starts": [
            {
                "tick": s.tick,
                "scene": s.scene,
                "staging_id": s.staging_id,
                "formation": s.formation,
                "enter_mode": s.enter_mode,
                "is_lone": s.is_lone,
            }
            for s in battle_starts(rows)
        ],
        "flag_bulk_frames": [
            {"tick": t, "scene": s, "flags": n} for (t, s, n) in cen.bulk_ticks
        ],
        "flag_beats": [
            {"flag": b.idx, "flag_hex": f"0x{b.idx:X}", "tick": b.set_tick, "scene": b.scene, "churn": b.churn}
            for b in cen.beats
        ],
        "items": [
            {"tick": r.tick, "scene": r.scene, "id": r.idx, "count": r.value, "delta": r.delta, "note": r.note}
            for r in item_changes(rows)
        ],
        "gold": [
            {"tick": r.tick, "scene": r.scene, "gold": r.value, "delta": r.delta} for r in gold_changes(rows)
        ],
        "party": [
            {"tick": r.tick, "scene": r.scene, "count": r.value, "note": r.note} for r in party_changes(rows)
        ],
    }


def _resolve_csv(arg: str) -> Path:
    p = Path(arg)
    if p.is_dir():
        cand = p / "state_poll.csv"
        if cand.exists():
            return cand
    return p


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("csv", help="state_poll.csv, or a capture dir containing one")
    ap.add_argument("--json", action="store_true", help="emit JSON instead of a text report")
    ap.add_argument(
        "--bulk-threshold",
        type=int,
        default=DEFAULT_BULK_THRESHOLD,
        help=f"flags/tick that marks a bulk load frame (default {DEFAULT_BULK_THRESHOLD})",
    )
    ap.add_argument(
        "--only",
        default="scenes,battles,flags,items,gold,party",
        help="comma list of sections: scenes,battles,flags,items,gold,party",
    )
    args = ap.parse_args(argv)

    path = _resolve_csv(args.csv)
    if not path.exists():
        print(f"no such CSV: {path}", file=sys.stderr)
        return 2
    rows = load_csv(path)

    if args.json:
        print(json.dumps(build_json(rows, args.bulk_threshold), indent=2))
    else:
        want = {s.strip() for s in args.only.split(",") if s.strip()}
        print(render_report(rows, args.bulk_threshold, want))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
