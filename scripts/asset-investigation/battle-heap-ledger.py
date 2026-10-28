#!/usr/bin/env python3
"""Net out a battle-heap allocator trace into an attributed ledger.

Input: the heap_ledger.csv written by
scripts/pcsx-redux/autorun_battle_heap_ledger.lua. Every alloc / free /
drain event after the formation install is replayed against a live map of
pointers; each node on the allocated rings at the final dump (`settled`, or
`main` when the run ended before the settle) is then attributed to the
event that produced it. A node with no alloc event in the trace was already
resident before the install.

Usage: battle-heap-ledger.py heap_ledger.csv [--dump settled|main] [--events]
"""

import argparse
import csv
import sys
from collections import defaultdict


def h(v):
    return int(v, 16) if v.startswith("0x") else int(v)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("csv")
    ap.add_argument("--dump", default=None)
    ap.add_argument("--events", action="store_true", help="print every event")
    a = ap.parse_args()

    rows = list(csv.DictReader(open(a.csv)))
    live = {}
    pre = {}
    dumps = defaultdict(list)
    last_load_alloc = None
    events = 0
    for r in rows:
        ev = r["event"]
        if ev.endswith("-alloc") or ev.endswith("-free") or ev.endswith("-pool") or ev.endswith("-freesum"):
            tag = ev.split("-")[0]
            dumps[tag].append(r)
            if tag == "pre" and ev.endswith("-alloc"):
                pre[h(r["ptr"])] = h(r["size"])
            continue
        if ev in ("alloc", "alloc-fail"):
            events += 1
            if a.events:
                print(f"v{r['vsync']} m{r['mode']} ALLOC p{r['pool']} {r['size']} -> {r['ptr']} "
                      f"caller {r['caller']} ({r['caller_fn']}) outer {r.get('outer')} ({r.get('outer_fn')}) "
                      f"outer2 {r.get('outer2')} ({r.get('outer2_fn')}) "
                      f"free {r['free_total']}")
            if ev == "alloc":
                live[h(r["ptr"])] = r
                last_load_alloc = r
        elif ev in ("free", "drain-free"):
            events += 1
            if a.events:
                print(f"v{r['vsync']} m{r['mode']} {ev.upper()} {r['size']} @ {r['ptr']} "
                      f"caller {r['caller']} ({r['caller_fn']}) free {r['free_total']}")
            live.pop(h(r["ptr"]), None)
        elif a.events:
            print(f"v{r['vsync']} m{r['mode']} {ev} {r['note']}")

    tag = a.dump or ("settled" if dumps.get("settled") else "main")
    nodes = [r for r in dumps[tag] if r["event"].endswith("-alloc")]
    print(f"# dump '{tag}': {len(nodes)} allocated nodes, {events} traced events")
    groups = defaultdict(lambda: [0, 0, []])
    total = 0
    for r in nodes:
        ptr, size = h(r["ptr"]), h(r["size"])
        total += size + 12
        src = live.get(ptr)
        if src is not None:
            key = (src["caller_fn"], src["caller"], src.get("outer") or "-")
        elif ptr in pre:
            key = ("pre-existing", "-", "-")
        else:
            key = ("unattributed", "-", "-")
        g = groups[key]
        g[0] += 1
        g[1] += size
        g[2].append(size)
    print(f"{'caller_fn':>12} {'call ret':>12} {'outer ret':>12} {'n':>3} {'bytes':>9} {'KiB':>7}  sizes")
    for key, (n, b, sizes) in sorted(groups.items(), key=lambda kv: -kv[1][1]):
        sz = ", ".join(f"0x{s:X}" for s in sizes[:8]) + (" ..." if len(sizes) > 8 else "")
        print(f"{key[0]:>12} {key[1]:>12} {key[2]:>12} {n:>3} {b:>9} {b / 1024:>7.2f}  {sz}")
    print(f"payload total {sum(g[1] for g in groups.values())} bytes, "
          f"with 12-byte headers {total} bytes")
    for r in dumps[tag]:
        if r["event"].endswith("-freesum"):
            print(f"free ring at '{tag}': {h(r['size'])} bytes ({h(r['size']) / 1024:.2f} KiB) "
                  f"in {r['free_nodes']} nodes, {r['note']}")
    if last_load_alloc is not None:
        f = h(last_load_alloc["free_total"])
        print(f"after the last traced allocation (v{last_load_alloc['vsync']}, "
              f"{last_load_alloc['size']} by {last_load_alloc['caller_fn']}): "
              f"free {f} bytes ({f / 1024:.2f} KiB)")


if __name__ == "__main__":
    sys.exit(main())
