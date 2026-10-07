#!/usr/bin/env python3
"""draw-family-diff.py - compare a retail frame's display list with the engine's
draw census, texture family by texture family.

    scripts/ci/draw-family-diff.py RETAIL ENGINE.jsonl [--top N] [--all]

RETAIL is a `mednafen-state display-list --json` file, or a save state
(`.mcr` / `.mc0..9` / `.sstate`), which is run through
`scripts/mednafen/display-list.py --json` first. ENGINE.jsonl is what
`play-window` writes under `LEGAIA_DIAG_DRAWS=<path>`
(`legaia_engine_core::draw_census`): one line per texture family of the
frame it captured.

A family is `(CLUT, tpage & 0x1FF)`. For each, the report gives the
triangle count on the 320 x 240 stage (a quad is two), the screen bounds and
the mean colour word on each side, sorted by the retail count. A family one
side draws and the other does not, or one whose bounds or colour part, is
where a frame difference lives - the ground cells a crop drops, a wall drawn
through a different colour path. Untextured packets have no family and are
counted on one summary row.

Nothing here is game data: the report prints numbers only.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
W, H = 320.0, 240.0


def load_retail(path: Path) -> dict:
    if path.suffix == ".json":
        return json.loads(path.read_text())
    with tempfile.TemporaryDirectory() as td:
        out = Path(td) / "dl.json"
        subprocess.run(
            [sys.executable, str(REPO / "scripts/mednafen/display-list.py"), str(path), "--json", str(out)],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        return json.loads(out.read_text())


def retail_families(dl: dict) -> dict:
    fam: dict = {}
    for pkt in dl.get("chain", []):
        (kind, p), = pkt["prim"].items()
        if "verts" in p:
            verts = p["verts"]
        elif "pos" in p:
            x, y = p["pos"]
            size = int("".join(c for c in kind if c.isdigit()) or 8)
            verts = [[x, y], [x + size, y], [x, y + size], [x + size, y + size]]
        else:
            continue
        if not any(0 <= v[0] < W and 0 <= v[1] < H for v in verts):
            continue
        cols = p.get("colors") or [p.get("color", [128, 128, 128])]
        col = [sum(c[k] for c in cols) / len(cols) for k in range(3)]
        key = (p["clut"], p["tpage"] & 0x1FF) if "clut" in p and "tpage" in p else None
        if key is None and "clut" in p:
            key = (p["clut"], None)
        n = 2 if len(verts) == 4 else 1
        e = fam.setdefault(key, {"tris": 0, "b": [1e9, 1e9, -1e9, -1e9], "c": [0.0, 0.0, 0.0]})
        e["tris"] += n
        xs = [v[0] for v in verts]
        ys = [v[1] for v in verts]
        e["b"] = [min(e["b"][0], *xs), min(e["b"][1], *ys), max(e["b"][2], *xs), max(e["b"][3], *ys)]
        for k in range(3):
            e["c"][k] += col[k] * n
    for e in fam.values():
        e["c"] = [c / e["tris"] for c in e["c"]]
    return fam


def engine_families(path: Path) -> dict:
    fam = {}
    for line in path.read_text().splitlines():
        if not line.strip():
            continue
        r = json.loads(line)
        fam[(r["clut"], r["tpage"])] = {"tris": r["tris"], "b": r["bounds"], "c": r["color"]}
    return fam


def fmt_b(b):
    return "[%4d,%4d..%4d,%4d]" % tuple(round(v) for v in b) if b else "-"


def fmt_c(c):
    return "(%3d,%3d,%3d)" % tuple(round(v) for v in c) if c else "-"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("retail", type=Path)
    ap.add_argument("engine", type=Path)
    ap.add_argument("--top", type=int, default=40)
    ap.add_argument("--all", action="store_true", help="print every family, not only differing ones")
    a = ap.parse_args()
    rf = retail_families(load_retail(a.retail))
    ef = engine_families(a.engine)
    untex = rf.pop(None, None)
    keys = sorted(set(rf) | set(ef), key=lambda k: -max(rf.get(k, {}).get("tris", 0), ef.get(k, {}).get("tris", 0)))
    print("%-6s %-6s %7s %7s  %-24s %-24s  %-15s %-15s" % (
        "clut", "tpage", "retail", "engine", "retail bounds", "engine bounds", "retail colour", "engine colour"))
    shown = 0
    for k in keys:
        r, e = rf.get(k), ef.get(k)
        rn, en = (r or {}).get("tris", 0), (e or {}).get("tris", 0)
        differs = (rn == 0) != (en == 0) or (rn and en and (max(rn, en) > 2 * min(rn, en)))
        if not a.all and not differs and r and e:
            dc = max(abs(x - y) for x, y in zip(r["c"], e["c"]))
            if dc < 12:
                continue
        print("%04X   %-6s %7d %7d  %-24s %-24s  %-15s %-15s" % (
            k[0], "%03X" % k[1] if k[1] is not None else "-", rn, en,
            fmt_b(r and r["b"]), fmt_b(e and e["b"]), fmt_c(r and r["c"]), fmt_c(e and e["c"])))
        shown += 1
        if shown >= a.top:
            break
    if untex:
        print("untextured retail packets on stage: %d triangles" % untex["tris"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
