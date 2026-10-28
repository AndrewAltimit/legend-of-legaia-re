#!/usr/bin/env python3
"""Plan a pad route through field scenes for autorun_walk_route.lua.

Reads the navigation JSON `asset scene-nav <scene>...` writes (the .MAP
collision grid and trigger tables, the MAN's scene-change sites and its
partition-1 placements) and writes the route file the probe walks.

Each leg is `scene:start:goal[:allow[:ignore]]`:

    start   "x,z"      world units
            "t=tx,tz"  a tile's centre (a door's entry tile)
            "-"        where the previous leg ended (same scene)
    goal    "tile:tx,tz"   any sub-cell of that tile (a door's trigger tile)
            "world:x,z"    the sub-cell holding that point
            "far:n"        the first open sub-cell n steps from the start
    allow   comma list of kind-1 trigger records the leg may cross (the door
            it is walking onto)
    ignore  comma list of partition-1 placements not to treat as obstacles

and a leg `LINE:<text>` copies <text> into the route verbatim (an ACT, TALK
or WAIT line; see the probe's header). Each leg opens with an AVOID line
naming the scene's other door tiles, so the probe's sidesteps cannot walk
through one.

The search runs over the 64-unit wall sub-cells (docs/subsystems/
field-locomotion.md, "Collision byte": world sub-cell (i, j) is the grid's
(xc = i, zc = j + 2), the wall probe's Z bias). A kind-0 trigger tile is a
teleport edge to its landing (docs/formats/field-map.md); a kind-1 gate-1
tile whose record carries a scene change is a wall unless allowed, any other
gate-1 tile costs 200 steps (an event, crossed only when nothing else
connects); a placement (model < 0xF0) blocks the 2 x 2 sub-cells around its
spawn point. The path is smoothed by line of sight with a 28-unit clearance.

    asset scene-nav kor5 kor4 > nav.json
    plan_walk_route.py nav.json route.txt \\
        "kor5:3674,4214:tile:36,33:6" "kor4:t=15,64:tile:18,39:0"

The JSON and the route are disc-derived: keep them in a scratch directory.
"""

import heapq
import json
import sys


def wall_fn(grid):
    def wall(i, j):
        xc, zc = i, j + 2
        if xc < 0 or zc < 0 or xc >= 256 or zc >= 256:
            return True
        b = grid[(zc >> 1) * 128 + (xc >> 1)]
        return bool((b >> 4) & (1 << (((zc & 1) << 1) | (xc & 1))))

    return wall


def door_tiles(s, allow=()):
    """Trigger tiles of the scene's door records outside `allow`."""
    door_slots = {d["slot"] for d in s.get("doors", []) if d["slot"] is not None}
    out = set()
    for t in s.get("trig_primary", []) + s.get("trig_fallback", []):
        r = t["rec"]
        if t["kind"] == 1 and r[3] == 1 and r[0] < 0x80 and r[1] < 0x80:
            if r[2] in door_slots and r[2] not in allow:
                out.add((r[0], r[1]))
    return sorted(out)


def plan(s, sx, sz, goal, allow=(), ignore=()):
    """Waypoints [((x, z), is_teleport)] from (sx, sz) to `goal`, or None."""
    wall = wall_fn(s["grid"])
    blocked, soft, tp = set(), set(), {}
    door_slots = {d["slot"] for d in s.get("doors", []) if d["slot"] is not None}
    for t in s.get("trig_primary", []) + s.get("trig_fallback", []):
        r = t["rec"]
        if r[0] >= 0x80 or r[1] >= 0x80:
            continue
        if t["kind"] == 0:
            # Landing in half-tiles: world (dest_x * 64 + 64, (dest_z + 1) * 64).
            tp[(r[0], r[1])] = (r[2] + 1, r[3] + 1)
        elif t["kind"] == 1 and r[3] == 1 and r[2] not in allow:
            dst = blocked if r[2] in door_slots else soft
            for a in (0, 1):
                for b in (0, 1):
                    dst.add((2 * r[0] + a, 2 * r[1] + b))
    for p in s.get("p1", []):
        if p["x"] < 16000 and p["model"] < 0xF0 and p["i"] not in ignore:
            ci, cj = p["x"] // 64, p["z"] // 64
            for a in (-1, 0):
                for b in (-1, 0):
                    blocked.add((ci + a, cj + b))
    far = None
    if goal.startswith("far:"):
        far, goals = int(goal[4:]), set()
    elif goal.startswith("tile:"):
        tx, tz = map(int, goal[5:].split(","))
        goals = {(2 * tx + a, 2 * tz + b) for a in (0, 1) for b in (0, 1)}
    elif goal.startswith("world:"):
        gx, gz = map(int, goal[6:].split(","))
        goals = {(gx // 64, gz // 64)}
    else:
        raise SystemExit(f"bad goal {goal}")
    start = (sx // 64, sz // 64)

    def open_(i, j):
        if not (0 <= i < 256 and 0 <= j < 256):
            return False
        if (i, j) in goals:
            return True
        return not wall(i, j) and (i, j) not in blocked

    def is_tp(c):
        return (c[0] >> 1, c[1] >> 1) in tp and c not in goals

    prev, dist, q, end = {start: None}, {start: 0}, [(0, start)], None
    while q:
        dc, c = heapq.heappop(q)
        if dc > dist.get(c, 1e18):
            continue
        if c in goals or (far is not None and dc >= far and c not in soft and not is_tp(c)):
            end = c
            break
        if is_tp(c) and c != start:
            n = tp[(c[0] >> 1, c[1] >> 1)]
            if dc + 1 < dist.get(n, 1e18):
                dist[n], prev[n] = dc + 1, c
                heapq.heappush(q, (dc + 1, n))
            continue
        for d in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            n = (c[0] + d[0], c[1] + d[1])
            if not open_(*n):
                continue
            nd = dc + 1 + (200 if n in soft else 0)
            if nd < dist.get(n, 1e18):
                dist[n], prev[n] = nd, c
                heapq.heappush(q, (nd, n))
    if end is None:
        return None
    path, c = [], end
    while c is not None:
        path.append(c)
        c = prev[c]
    path.reverse()

    def los(a, b):
        ax, az = a[0] * 64 + 32, a[1] * 64 + 32
        bx, bz = b[0] * 64 + 32, b[1] * 64 + 32
        n = max(1, int(max(abs(bx - ax), abs(bz - az)) / 8))
        for k in range(n + 1):
            x, z = ax + (bx - ax) * k / n, az + (bz - az) * k / n
            for ox in (-28, 0, 28):
                for oz in (-28, 0, 28):
                    c = (int((x + ox) // 64), int((z + oz) // 64))
                    if c in (a, b):
                        continue
                    if not open_(*c) or is_tp(c) or (c in soft and a not in soft and b not in soft):
                        return False
        return True

    segs, cur = [], [path[0]]
    for k in range(1, len(path)):
        a, b = path[k - 1], path[k]
        if abs(a[0] - b[0]) + abs(a[1] - b[1]) > 1:
            segs.append((cur, True))
            cur = [b]
        else:
            cur.append(b)
    segs.append((cur, False))
    out = []
    for seg, ends_tp in segs:
        if len(seg) == 1 and ends_tp:
            out.append((seg[0], True))
            continue
        k = 0
        while k < len(seg) - 1:
            m = len(seg) - 1
            while m > k + 1 and not los(seg[k], seg[m]):
                m -= 1
            out.append((seg[m], ends_tp and m == len(seg) - 1))
            k = m
    return [((i * 64 + 32, j * 64 + 32), t) for (i, j), t in out]


def main():
    if len(sys.argv) < 4:
        raise SystemExit(__doc__)
    nav = json.load(open(sys.argv[1]))
    lines, last = [], None
    for leg in sys.argv[3:]:
        if leg.startswith("LINE:"):
            lines.append(leg[5:])
            continue
        parts = leg.split(":")
        scene, st, goal = parts[0], parts[1], parts[2] + ":" + parts[3]
        allow = tuple(int(x) for x in parts[4].split(",")) if len(parts) > 4 and parts[4] else ()
        ignore = tuple(int(x) for x in parts[5].split(",")) if len(parts) > 5 and parts[5] else ()
        if st == "-":
            sx, sz = last
        elif st.startswith("t="):
            tx, tz = map(int, st[2:].split(","))
            sx, sz = tx * 128 + 64, tz * 128 + 64
        else:
            sx, sz = map(int, st.split(","))
        wps = plan(nav[scene], sx, sz, goal, allow, ignore)
        if wps is None:
            raise SystemExit(f"no path: {leg}")
        avoid = door_tiles(nav[scene], allow)
        if avoid:
            lines.append(f"{scene} AVOID " + ";".join(f"{x},{z}" for x, z in avoid))
        lines += [f"{scene} {x} {z}{' tp' if t else ''}" for (x, z), t in wps]
        last = wps[-1][0] if wps else (sx, sz)
        print(f"{leg}: {len(wps)} waypoints, ends at {last}", file=sys.stderr)
    with open(sys.argv[2], "w") as fh:
        fh.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
