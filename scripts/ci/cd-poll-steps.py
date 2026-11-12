#!/usr/bin/env python3
"""Price a battle frame's CD stop-watch poll at a console's rate, arm by arm.

Reads cast-sequence recordings made with `--cd-trace` (`retail/cd.csv[.gz]`, the
PCSX-Redux breakpoint core's libcd and exception log). Each frame is the window the
frame-cost words measure, from the frame end's `VSync(n)` return (`0x800171C8`) to the
CPU-done `VSync(1)` (`0x800174CC`). Inside it, every CD interrupt is priced from the
exception vector `0x80000080` to the response dispatcher's exit (`0x8005DB74`); the rest
is the frame's own work plus the exception exits, an upper bound on the work.

The clip player's stop watch `FUN_8003D764` (the CdSync callback) issues `GetlocL` and
`GetStat` back to back from interrupt context for as long as a voice clip plays, so the
frame's length is its work over the share of the CPU the polls leave. On PCSX-Redux a
command's first response comes `0x800` cycles after it (`cdrom.cc`, `AddIrqQueue(m_cmd,
0x800)`) and the polls take about two thirds of the CPU; a console's `GetStat` answers in
`0xC4E1` cycles on average (psx-spx, CDROM response timings), which leaves the polls
`--share` of it. Per arm the script prints the step the recording's own costs read
through the ring law (`frame_clock::FrameCostRing`) and the step the same work reads at
the console's share, and with `--table` the Rust rows of the hardware arm table.

    scripts/ci/cd-poll-steps.py captures/w39-RR/seru
    scripts/ci/cd-poll-steps.py captures/w39-RR/seru --table
    scripts/ci/cd-poll-steps.py captures/w39-RR/seru --mednafen captures/w39-LL/mednafen

`--mednafen DIR` scores both readings against mednafen's per-arm steps
(`scripts/mednafen/frame-cost-film.py` CSVs named `<summon>_summon_mid_cast.csv`).
Numbers only: it changes nothing and gates nothing. The recordings hold retail RAM and
live under the gitignored `captures/`. See docs/subsystems/battle-stage-camera.md,
"The CD stop watch".
"""

from __future__ import annotations

import argparse
import collections
import csv
import gzip
import statistics
from pathlib import Path

# CPU cycles a scanline: 3413 video cycles * 7 / 11.
HBLANK_CPU = 3413 * 7 / 11
# The poll's share of a console's CPU: about 3300 cycles of interrupt work a poll
# (measured on PCSX-Redux) over a 0xC4E1-cycle response.
SHARE = 0.065
# Seru summon modules: (first action id, first PROT entry).
SERU_ENTRIES = ((0x81, 903, 0x95), (0x99, 927, 0xA0))
MEDNAFEN_SPELLS = {"theeder": 0x82, "vera": 0x83, "gizam": 0x84, "nighto": 0x85, "zenoir": 0x86,
                   "viguro": 0x87, "swordie": 0x88, "orb": 0x89, "freed": 0x8A}


def step_of_cost(raw: float) -> int:
    return 1 if raw < 0xF1 else 2 if raw < 0x1FF else 3 if raw < 0x2D1 else 4


class Ring:
    """`FUN_80016B6C`'s sixteen-frame ring: a frame of `0x2D0` or more is stored as
    the clamp `0x2BC` and compared raw; the step is the ring's maximum's rung."""

    def __init__(self) -> None:
        self.entries = [0.0] * 16

    def push(self, raw: float) -> int:
        self.entries = self.entries[1:] + [raw if raw < 0x2D0 else 0x2BC]
        return 4 if raw >= 0x2D1 else step_of_cost(max(self.entries))


def frames(trace: Path) -> list[dict]:
    """One dict a frame: `key` (action state, module phase), `cost` (hblanks),
    `poll` (hblanks of CD interrupt), `polls`."""
    opener = gzip.open if trace.suffix == ".gz" else open
    out, cur, last_exc = [], None, None
    with opener(trace, "rt") as f:
        for r in csv.DictReader(f):
            kind, cycle = r["kind"], int(r["cycle"])
            if kind == "exc":
                last_exc = cycle
            elif kind == "vsyncret":
                cur = {"start": cycle, "poll": 0, "polls": 0}
            elif cur is None:
                continue
            elif kind == "irq":
                cur["irq"] = last_exc if last_exc is not None and cycle - last_exc < 20000 else cycle
            elif kind == "irq_end" and "irq" in cur:
                cur["poll"] += cycle - cur.pop("irq")
                cur["polls"] += 1
            elif kind == "cpudone":
                cost = (cycle - cur["start"]) / HBLANK_CPU
                out.append({"key": (int(r["st7"]), int(r["phase"])), "cost": cost,
                            "poll": cur["poll"] / HBLANK_CPU, "polls": cur["polls"]})
                cur = None
    return out


def arms(film: Path, share: float) -> dict[int, dict]:
    """Per module arm of the summon band (`0x35` / `0x36`): frames, polls a frame,
    median recorded and console cost, and the modal step each reads (by vsyncs)."""
    trace = film / "retail" / "cd.csv.gz"
    if not trace.exists():
        trace = film / "retail" / "cd.csv"
    recorded, console = Ring(), Ring()
    step_r = step_c = 2
    by_arm: dict[int, list] = collections.OrderedDict()
    for f in frames(trace):
        work = f["cost"] - f["poll"]
        hw = work / (1 - share) if f["polls"] else f["cost"]
        if f["key"][0] in (0x35, 0x36):
            by_arm.setdefault(f["key"][1], []).append((f, hw, step_r, step_c))
        step_r, step_c = recorded.push(f["cost"]), console.push(hw)
    res = {}
    for arm, rows in by_arm.items():
        def modal(i: int) -> int:
            c = collections.Counter()
            for row in rows:
                c[row[i]] += row[i]
            return c.most_common(1)[0][0]
        hw_cost = statistics.median(r[1] for r in rows)
        res[arm] = {
            "frames": len(rows),
            "polls": statistics.median(r[0]["polls"] for r in rows),
            "cost": statistics.median(r[0]["cost"] for r in rows),
            "hw_cost": hw_cost,
            "recorded": modal(2),
            # A one-frame arm runs on the step the arms before it left: its own
            # cost names it.
            "console": modal(3) if len(rows) > 1 else step_of_cost(hw_cost),
        }
    return res


def spell_of(film: Path) -> int | None:
    for line in (film / "retail" / "manifest.txt").read_text().splitlines():
        if line.startswith("spell"):
            return int(line.split("=", 1)[1].strip(), 0)
    return None


def entry_of(spell: int) -> int | None:
    for first, entry, last in SERU_ENTRIES:
        if first <= spell <= last:
            return entry + spell - first
    return None


def mednafen_steps(path: Path) -> dict[int, collections.Counter]:
    seen, out = set(), collections.defaultdict(collections.Counter)
    for r in csv.DictReader(path.open()):
        if r["frame"] in seen:
            continue
        seen.add(r["frame"])
        if r["st7"] in ("53", "54"):
            out[int(r["phase"])][int(r["step"])] += int(r["step"])
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("root", type=Path, help="directory of cast-sequence --cd-trace recordings")
    ap.add_argument("--share", type=float, default=SHARE, help="the poll's share of a console's CPU")
    ap.add_argument("--table", action="store_true", help="print the hardware arm table's Rust rows")
    ap.add_argument("--mednafen", type=Path, help="directory of mednafen frame-cost-film CSVs")
    a = ap.parse_args()
    films = {}
    for film in sorted(a.root.iterdir()):
        if (film / "retail" / "manifest.txt").exists():
            films[film.name] = (spell_of(film), arms(film, a.share))
    for name, (spell, res) in films.items():
        cells = " ".join(f"{arm}:{x['recorded']}>{x['console']}{'*' if x['polls'] else ''}"
                         for arm, x in res.items() if arm != 255)
        print(f"{name} 0x{spell or 0:02X} {cells}")
    if a.table:
        print()
        for name, (spell, res) in films.items():
            entry = entry_of(spell or 0)
            if entry is None:
                continue
            top = max((arm for arm in res if arm != 255), default=0)
            steps = [res[arm]["console"] if arm in res else 0 for arm in range(top + 1)]
            print(f"    ({entry}, &{steps}),")
    if a.mednafen:
        by_spell = {spell: res for spell, res in films.values()}
        agree = collections.Counter()
        for summon, spell in MEDNAFEN_SPELLS.items():
            path = a.mednafen / f"{summon}_summon_mid_cast.csv"
            if not path.exists() or spell not in by_spell:
                continue
            for arm, c in mednafen_steps(path).items():
                x = by_spell[spell].get(arm)
                if x is None:
                    continue
                agree["vsyncs"] += sum(c.values())
                agree["recorded"] += c[x["recorded"]]
                agree["console"] += c[x["console"]]
        if agree["vsyncs"]:
            n = agree["vsyncs"]
            print(f"\nmednafen per-arm steps, {n} vsyncs: recorded {agree['recorded'] / n:.3f}, "
                  f"console {agree['console'] / n:.3f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
