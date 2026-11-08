#!/usr/bin/env python3
"""Rank cast-sequence films by camera error instead of image score.

Reads each film directory's `camera.csv` (written by `legaia-engine
cast-sequence-score`, every retail vsync paired with its engine tick) and prints
one row per film, worst first: the first row the camera's on-screen error passes
`--px`, and the worst error it holds for `--sustain` rows, per channel and as
pixels at the GTE's own projection.

    scripts/ci/cast-camera-rank.py captures/cast-batch
    scripts/ci/cast-camera-rank.py captures/before captures/after --filter caruban

Two directories compare: the second's films pair with the first's by name, and
the table gains a before / after column. "law px" is the
sustained error with each row taking the best of the engine ticks within two of its
pair, so a pass boundary a vsync apart does not read as a framing error; "raw px"
is the pair's own. A film's pixels are the mean displacement
of six world probes around retail's focus under each side's pitch, yaw, TR trio and
focus. See `docs/tooling/cast-sequence-oracle.md`, "The camera channel".
"""

import argparse
import csv
import math
import sys
from pathlib import Path

GTE_H = 256.0
RADIUS = 700.0
PROBES = [(0, 0, 0), (RADIUS, 0, 0), (-RADIUS, 0, 0), (0, 0, RADIUS), (0, 0, -RADIUS), (0, -500, 0)]


def circ(a: int, b: int) -> int:
    return (a - b + 2048) % 4096 - 2048


def project(p, pitch, yaw, tr, focus):
    """`screen = H * (R * (v - focus) + tr) / z`, R = Rx(pitch) Ry(yaw)."""
    cy, sy = math.cos(yaw * math.tau / 4096), math.sin(yaw * math.tau / 4096)
    cp, sp = math.cos(pitch * math.tau / 4096), math.sin(pitch * math.tau / 4096)
    dx, dy, dz = p[0], p[1], p[2]
    x = cy * dx + sy * dz
    z0 = -sy * dx + cy * dz
    y = cp * dy - sp * z0
    z = sp * dy + cp * z0
    z = max(z + tr[2], 64.0)
    return GTE_H * (x + tr[0]) / z, GTE_H * (y + tr[1]) / z


def read(film: Path):
    rows = []
    # The engine's own camera per tick: a row's error is also read one or two
    # ticks either side, so a frame boundary that fell a vsync apart is not
    # scored as a framing error ("phase-robust" below).
    eng = {}
    ecam = film / "engine" / "cam.csv"
    if ecam.exists():
        with open(ecam) as f:
            for r in csv.DictReader(f):
                eng[int(r["tick"])] = (int(r["pitch"]), int(r["yaw"]), int(r["trx"]), int(r["try"]), int(r["trz"]),
                                       int(r["fx"]), int(r["fz"]))
    with open(film / "camera.csv") as f:
        for r in csv.DictReader(f):
            g = lambda k: int(r[k]) if r[k] != "" else None
            rp, ry = g("r_pitch"), g("r_yaw")
            ep, ey = g("e_pitch"), g("e_yaw")
            rt, et = (g("r_trx"), g("r_try"), g("r_trz")), (g("e_trx"), g("e_try"), g("e_trz"))
            rf = (g("r_fx"), g("r_fz"))
            ef = (g("e_fx"), g("e_fz"))
            has_focus = None not in rf and None not in ef
            d_focus = (ef[0] - rf[0], ef[1] - rf[1]) if has_focus else (0, 0)
            def probe_px(ep_, ey_, et_, ef_):
                # A shot that dollies through the eye (TR z near zero on a
                # module's fly-by) projects to nothing meaningful.
                if rt[2] < 800 or et_[2] < 800:
                    return 0.0
                total = 0.0
                d = (ef_[0] - rf[0], ef_[1] - rf[1]) if has_focus else (0, 0)
                for p in PROBES:
                    ax, ay = project(p, rp, ry, rt, (0, 0))
                    # The engine's probe is the same world point, so its offset
                    # from the engine's own focus moves by the focus error.
                    q = (p[0] - d[0], p[1], p[2] - d[1])
                    bx, by = project(q, ep_, ey_, et_, (0, 0))
                    total += math.hypot(bx - ax, by - ay)
                return total / len(PROBES)
            px = probe_px(ep, ey, et, ef)
            # Phase-robust: the smallest error within two ticks of the pair.
            robust = px
            for off in (-2, -1, 1, 2):
                c = eng.get(g("tick") + off)
                if c:
                    robust = min(robust, probe_px(c[0], c[1], (c[2], c[3], c[4]), (c[5], c[6])))
            rows.append({
                "v": g("vsync"), "t": g("tick"), "key": r["key"], "into": g("into"),
                "dyaw": circ(ey, ry), "dpitch": circ(ep, rp), "dtrz": et[2] - rt[2],
                "dtry": et[1] - rt[1], "dtrx": et[0] - rt[0],
                "dfocus": max(abs(d_focus[0]), abs(d_focus[1])), "px": px, "robust": robust,
                "rstep": g("r_step"), "estep": g("e_step"),
            })
    return rows


def sustained(values, window):
    """The largest error held for `window` consecutive rows."""
    if len(values) < window:
        return max(values, default=0)
    return max(min(values[i:i + window]) for i in range(len(values) - window + 1))


def summary(film: Path, px_limit: float, window: int):
    rows = read(film)
    if not rows:
        return None
    first = next((r for r in rows if r["px"] > px_limit), None)
    worst = max(rows, key=lambda r: r["px"])
    out = {
        "film": film.name, "rows": len(rows),
        "mean_px": sum(r["px"] for r in rows) / len(rows),
        "sus_px": sustained([r["px"] for r in rows], window),
        "max_px": worst["px"],
        "sus_robust": sustained([r["robust"] for r in rows], window),
        "mean_robust": sum(r["robust"] for r in rows) / len(rows),
        "first": first, "worst": worst,
    }
    for c in ("dyaw", "dpitch", "dtrz", "dtry", "dtrx", "dfocus"):
        out["sus_" + c] = sustained([abs(r[c]) for r in rows], window)
    return out


def films(root: Path):
    if (root / "camera.csv").exists():
        return [root]
    return sorted(p.parent for p in root.glob("*/camera.csv"))


def where(r):
    return f"{r['key']}+{r['into']}" if r else "-"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("dirs", type=Path, nargs="+", help="a directory of film directories; two compare before / after")
    ap.add_argument("--px", type=float, default=2.0, help="first-divergence threshold, pixels")
    ap.add_argument("--sustain", type=int, default=8, help="rows an error must hold to count as sustained")
    ap.add_argument("--filter", default="", help="substring of the film name")
    ap.add_argument("--top", type=int, default=0, help="rows to print (0 = all)")
    a = ap.parse_args()
    base = {}
    for f in films(a.dirs[0]):
        if a.filter in f.name and (s := summary(f, a.px, a.sustain)):
            base[f.name] = s
    after = {}
    if len(a.dirs) > 1:
        for f in films(a.dirs[1]):
            if a.filter in f.name and (s := summary(f, a.px, a.sustain)):
                after[f.name] = s
    ranked = sorted(base.values(), key=lambda s: -s["sus_robust"])
    if a.top:
        ranked = ranked[:a.top]
    hdr = f"{'law px':>7} {'raw px':>7} {'mean':>6} {'max':>6}  {'yaw':>5} {'pit':>4} {'trz':>5} {'foc':>4}  {'first divergence':<24} film"
    if after:
        hdr = f"{'after':>7} " + hdr
    print(hdr)
    for s in ranked:
        line = (f"{s['sus_robust']:7.2f} {s['sus_px']:7.2f} {s['mean_px']:6.2f} {s['max_px']:6.1f}  {s['sus_dyaw']:5d} {s['sus_dpitch']:4d} "
                f"{s['sus_dtrz']:5d} {s['sus_dfocus']:4d}  {where(s['first']):<24} {s['film']}")
        if after:
            b = after.get(s["film"])
            line = f"{(format(b['sus_px'], '7.2f') if b else '      -')} " + line
        print(line)
    if after:
        common = [n for n in base if n in after]
        n = max(len(common), 1)
        print(f"# mean sustained law px over {len(common)} films: "
              f"{sum(base[f]['sus_robust'] for f in common) / n:.3f} -> {sum(after[f]['sus_robust'] for f in common) / n:.3f}")
    else:
        print(f"# {len(base)} films, mean sustained law px {sum(s['sus_robust'] for s in base.values()) / max(len(base), 1):.3f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
