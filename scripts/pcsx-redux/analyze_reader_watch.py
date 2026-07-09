#!/usr/bin/env python3
"""Summarize a flag_reader_watch.csv provenance capture.

The probe (autorun_flag_reader_watch.lua) records every story-flag helper
hit - test/set/clear with the caller ra, plus direct byte reads of target
flags - deduped by (kind, flag, ra) with a running count column. This
analyzer turns that stream into the deliverable: per-flag reader/writer
site tables, with each pc/ra labeled against the already-cataloged
functions so a NEW (uncataloged) site is visible at a glance instead of a
hand-grep through docs/reference/functions.md.

Counts: the probe emits a row for the first N hits of a key and then only
every Mth, so the max `count` seen per site is a LOWER BOUND on the true
total (printed as ">=N" when the last row was a suppressed-interval one).

Usage:
  python3 analyze_reader_watch.py captures/flag_reader_watch/<ts>/flag_reader_watch.csv
  ... --only targets          # just the target-flag site tables
  ... --only background       # just the all-flag provenance summary
  ... --labels my_labels.txt  # extend site labels ("0xADDR free text" lines)
  ... --json                  # machine-readable dump

Dependency-free (stdlib only); tests in test_analyze_reader_watch.py.
"""
from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass, field
from pathlib import Path

# Cataloged sites (docs/reference/functions.md + subsystem docs). pc and ra
# values share one namespace: a hit matches on either. Extend per-run with
# --labels rather than editing here unless the site is durably documented.
KNOWN_SITES: dict[int, str] = {
    0x8003CE08: "FUN_8003CE08 flag SET helper",
    0x8003CE34: "FUN_8003CE34 flag CLEAR helper",
    0x8003CE64: "FUN_8003CE64 flag TEST helper",
    0x801E35E8: "field-VM op-0x70 TEST handler return (overlay; FUN_801DE840 dispatch)",
    0x801D218C: "walk-on tile-trigger dispatch (FUN_801D1EC4 -> FUN_801D5630 -> FUN_8003BDE0)",
    0x8003BF78: "FUN_8003BDE0 internal gate-bit read (walk-on trigger gate check)",
    0x8003C008: "FUN_8003BDE0 internal gate-bit read (walk-on trigger gate check)",
    0x8001A8BC: "bulk lbu;sb memcpy loop (save/transfer scan of the flag bank - discard)",
}

HELPER_PCS = {0x8003CE08, 0x8003CE34, 0x8003CE64}
CONTEXT_KINDS = {"scene", "mode", "snap"}


def classify_region(addr: int) -> str:
    """Coarse home of an address: SCUS-resident vs runtime overlay."""
    if 0x80010000 <= addr < 0x80080000:
        return "SCUS-resident"
    if addr >= 0x801C0000:
        return "overlay (attribute by containment: attribute_overlay_hits.py)"
    return "other-RAM"


@dataclass
class Row:
    tick: int
    kind: str
    flag: int
    pc: int
    ra: int
    mode: int
    scene: str
    count: int
    note: str


@dataclass
class Site:
    kind: str
    flag: int
    pc: int
    ra: int
    total: int = 0          # max running count seen (lower bound)
    exact: bool = True      # False once a suppressed-interval row is last
    first_tick: int = 0
    first_scene: str = ""
    scenes: set[str] = field(default_factory=set)
    tiles: set[str] = field(default_factory=set)
    target: bool = False


def parse_rows(lines) -> list[Row]:
    rows: list[Row] = []
    for line in lines:
        line = line.strip()
        if not line or line.startswith("tick,"):
            continue
        parts = line.split(",")
        if len(parts) < 8:
            continue
        note = parts[8] if len(parts) > 8 else ""
        try:
            rows.append(Row(
                tick=int(parts[0]), kind=parts[1], flag=int(parts[2]),
                pc=int(parts[3], 16), ra=int(parts[4], 16),
                mode=int(parts[5], 16), scene=parts[6],
                count=int(parts[7]), note=note))
        except ValueError:
            continue
    return rows


def collect_sites(rows: list[Row]) -> dict[tuple, Site]:
    """Aggregate hit rows into per-(kind,flag,pc,ra) sites."""
    sites: dict[tuple, Site] = {}
    for r in rows:
        if r.kind in CONTEXT_KINDS:
            continue
        key = (r.kind, r.flag, r.pc, r.ra)
        s = sites.get(key)
        if s is None:
            s = Site(kind=r.kind, flag=r.flag, pc=r.pc, ra=r.ra,
                     first_tick=r.tick, first_scene=r.scene)
            sites[key] = s
        s.total = max(s.total, r.count)
        s.scenes.add(r.scene)
        for tok in r.note.split():
            if tok.startswith("t") and ";" in tok:
                s.tiles.add(tok)
            if tok == "tgt":
                s.target = True
    # The probe logs every hit up to a per-class prefix (targets 8,
    # background 4) and then only every Nth - so a max count inside the
    # prefix is exact, anything past it is a lower bound.
    for s in sites.values():
        s.exact = s.total <= (8 if s.target else 4)
    return sites


def label_for(addr: int, labels: dict[int, str]) -> str:
    if addr in labels:
        return labels[addr]
    return f"[NEW] uncataloged, {classify_region(addr)}"


def site_line(s: Site, labels: dict[int, str]) -> str:
    # For helper hits the pc IS the helper; the caller ra is the news.
    who = s.ra if s.pc in HELPER_PCS else s.pc
    cnt = f"{s.total}" if s.exact else f">={s.total}"
    tiles = f" tiles={','.join(sorted(s.tiles))}" if s.tiles else ""
    return (f"    {s.kind:<9} pc=0x{s.pc:08X} ra=0x{s.ra:08X} n={cnt:<7} "
            f"first@{s.first_tick}/{s.first_scene}{tiles}\n"
            f"              -> {label_for(who, labels)}")


def load_labels(path: str | None) -> dict[int, str]:
    labels = dict(KNOWN_SITES)
    if path:
        for line in Path(path).read_text().splitlines():
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            addr, _, text = line.partition(" ")
            try:
                labels[int(addr, 16)] = text.strip() or "(labeled)"
            except ValueError:
                continue
    return labels


def render(rows: list[Row], labels: dict[int, str], only: str | None) -> str:
    sites = collect_sites(rows)
    out: list[str] = []
    scenes = []
    for r in rows:
        if r.kind == "scene" and (not scenes or scenes[-1] != r.scene):
            scenes.append(r.scene)
    span = f"{rows[0].tick}..{rows[-1].tick}" if rows else "-"
    out.append(f"ticks {span}; scenes: {' > '.join(scenes) or '-'}")
    totals: dict[str, int] = {}
    for s in sites.values():
        totals[s.kind] = totals.get(s.kind, 0) + s.total
    out.append("totals (lower bounds): " + " ".join(
        f"{k}={v}" for k, v in sorted(totals.items())) or "-")

    target_flags = sorted({s.flag for s in sites.values() if s.target}
                          | {s.flag for s in sites.values() if s.kind == "byteread"})

    if only in (None, "targets"):
        out.append("\n== TARGET FLAGS ==")
        if not target_flags:
            out.append("  (no target-flag hits)")
        for f in target_flags:
            out.append(f"  flag 0x{f:X} ({f}):")
            fs = sorted((s for s in sites.values() if s.flag == f),
                        key=lambda s: (s.kind, s.first_tick))
            for s in fs:
                out.append(site_line(s, labels))
                if s.kind == "byteread":
                    out.append("              byteread covers 8 flags - verify the code at pc masks this bit")

    if only in (None, "background"):
        out.append("\n== ALL-FLAG PROVENANCE (background, deduped) ==")
        bg_flags = sorted({s.flag for s in sites.values()} - set(target_flags))
        if not bg_flags:
            out.append("  (none)")
        for f in bg_flags:
            fs = sorted((s for s in sites.values() if s.flag == f),
                        key=lambda s: (s.kind, s.first_tick))
            kinds = {}
            news = []
            for s in fs:
                who = s.ra if s.pc in HELPER_PCS else s.pc
                kinds.setdefault(s.kind, set()).add(who)
                if who not in labels:
                    news.append(who)
            desc = " ".join(
                f"{k}[{','.join(f'0x{a:08X}' for a in sorted(v))}]"
                for k, v in sorted(kinds.items()))
            mark = "  [NEW ra]" if news else ""
            out.append(f"  0x{f:<5X} {desc}{mark}")

    if only in (None, "snaps"):
        snaps = [r for r in rows if r.kind == "snap"]
        if snaps:
            out.append("\n== SNAPSHOTS ==")
            for r in snaps:
                out.append(f"  tick {r.tick} scene {r.scene}: {r.note}")
    return "\n".join(out)


def to_json(rows: list[Row], labels: dict[int, str]) -> str:
    sites = collect_sites(rows)
    payload = []
    for s in sites.values():
        who = s.ra if s.pc in HELPER_PCS else s.pc
        payload.append({
            "kind": s.kind, "flag": s.flag, "pc": f"0x{s.pc:08X}",
            "ra": f"0x{s.ra:08X}", "total_min": s.total, "exact": s.exact,
            "first_tick": s.first_tick, "first_scene": s.first_scene,
            "scenes": sorted(s.scenes), "tiles": sorted(s.tiles),
            "target": s.target, "label": label_for(who, labels),
            "new": who not in labels,
        })
    payload.sort(key=lambda d: (d["flag"], d["kind"], d["first_tick"]))
    return json.dumps(payload, indent=2)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("csv", help="flag_reader_watch.csv path")
    ap.add_argument("--only", choices=["targets", "background", "snaps"])
    ap.add_argument("--labels", help="extra site labels: '0xADDR text' lines")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args(argv)
    rows = parse_rows(Path(args.csv).read_text().splitlines())
    labels = load_labels(args.labels)
    if args.json:
        print(to_json(rows, labels))
    else:
        print(render(rows, labels, args.only))
    return 0


if __name__ == "__main__":
    sys.exit(main())
