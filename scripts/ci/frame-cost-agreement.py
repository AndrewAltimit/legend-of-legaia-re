#!/usr/bin/env python3
"""Score the GPU-inclusive frame-cost model against recorded frame steps.

Reads per-frame recordings of retail's frame cost on either emulator and runs the frame
driver's step law (`FUN_80016B6C`: a sixteen-entry ring, a raw cost of `0x2D0` or more
stored as `0x2BC`, its maximum binned at `0xF1` / `0x1FF` / `0x2D1`) over three costs per
pass, scoring each against the step the emulator stored:

- `cpu`: `gp+0x3A8`, the hblanks to the CPU's end of frame;
- `cpu+gpu`: `max(cpu, draw + dma)`, the CPU's word against the GPU draw-time model of the
  table the GPU drew during the pass (`scripts/mednafen/gpu_cost.py`, which nothing fits);
- `entry`: `gp+0x3AC`, the emulator's own ring entry (the pairing's ceiling).

On mednafen, whose GPU takes time, the passes the GPU bounds (`3AC` over `3A8`) also report
`3AC - (draw + dma)`. On PCSX-Redux, whose GPU is free, `entry` is `cpu` and the `cpu+gpu`
column is a prediction of what the GPU's share would move.

    scripts/ci/frame-cost-agreement.py --mednafen zenoir.csv --redux captures/x/retail/gpu.csv

Mednafen CSVs come from `scripts/mednafen/frame-cost-film.py`, PCSX-Redux ones from a
cast-sequence film run with `--gpu-cost` (`gpu.csv`). Report only: it gates nothing. See
docs/subsystems/battle-stage-camera.md, "What a real console's frame costs".
"""

import argparse
import csv
import statistics
import sys
from pathlib import Path


def step_of(cost: float) -> int:
    return 1 if cost < 0xF1 else 2 if cost < 0x1FF else 3 if cost < 0x2D1 else 4


def ring_steps(entries):
    out, ring = [], []
    for e in entries:
        ring = (ring + [e if e < 0x2D0 else 0x2BC])[-16:]
        out.append(step_of(e if e >= 0x2D0 else max(ring)))
    return out


def load(path: Path, mednafen: bool):
    """One record per pass end: the row where the cost words change. The table the GPU
    waited on is the one the row before showed as drawn."""
    rows = list(csv.DictReader(path.open()))
    draw_key, dma_key = ("gpu_drawn", "dma_drawn") if mednafen else ("draw", "dma")
    out, prev = [], None
    for r in rows:
        if prev is not None and (r["cpu"], r["tot"]) != (prev["cpu"], prev["tot"]) and \
                (mednafen or r["frame"] != prev["frame"]):
            out.append(dict(cpu=float(r["cpu"]), tot=float(r["tot"]), step=int(float(r["step"])),
                            gpu=float(prev[draw_key]) + float(prev[dma_key])))
        prev = r
    return out


def score(passes, cost, warm=16):
    pred = ring_steps([cost(p) for p in passes])
    pairs = list(zip(pred, (p["step"] for p in passes)))[warm:]
    return (sum(a == b for a, b in pairs) / len(pairs), len(pairs)) if pairs else (float("nan"), 0)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--mednafen", nargs="*", default=[], type=Path)
    ap.add_argument("--redux", nargs="*", default=[], type=Path)
    a = ap.parse_args()
    films = [(p, True) for p in a.mednafen] + [(p, False) for p in a.redux]
    if not films:
        ap.error("no recordings")
    print("| Recording | Emulator | Passes | `cpu` | `cpu+gpu` | `entry` | GPU-bound | `3AC - model` median |")
    print("|---|---|---:|---:|---:|---:|---:|---:|")
    for path, med in films:
        passes = load(path, med)
        cols = [score(passes, c)[0] for c in (lambda p: p["cpu"],
                                              lambda p: max(p["cpu"], p["gpu"]),
                                              lambda p: p["tot"])]
        bound = [p for p in passes if p["tot"] - p["cpu"] > 3]
        err = f"{statistics.median(p['tot'] - p['gpu'] for p in bound):+.1f}" if bound else "-"
        name = path.parent.parent.name if path.name == "gpu.csv" else path.stem
        print(f"| `{name}` | {'mednafen' if med else 'PCSX-Redux'} | {len(passes)} | "
              f"{cols[0]:.3f} | {cols[1]:.3f} | {cols[2]:.3f} | {len(bound)} | {err} |")
    return 0


if __name__ == "__main__":
    sys.exit(main())
