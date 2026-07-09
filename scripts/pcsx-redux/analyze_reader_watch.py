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

Cross-run merge: pass MULTIPLE csvs or run DIRECTORIES (scanned recursively
for flag_reader_watch.csv) and the sites union into one cumulative
provenance map - counts sum per run, each site lists the runs that saw it.
Every trek permanently grows one database instead of answering one question:

  python3 analyze_reader_watch.py captures/flag_reader_watch/
  ... --merged-out provenance.json   # persist the merged site map

Overlay residency: the probe checksums the two overlay slots (A 0x801CE818,
B 0x801F69D8) on every scene/mode change and emits `overlay` rows. With a
checksum->label map (committed default `overlay-map.txt`; regenerate from a
disc extraction with --gen-overlay-map), each overlay-region hit is
attributed to the overlay RESIDENT when it fired - the field/menu/battle/
minigame siblings all alias the same slot-A window, so a bare address is
ambiguous without this.

Usage:
  python3 analyze_reader_watch.py captures/flag_reader_watch/<ts>/flag_reader_watch.csv
  ... --only targets          # just the target-flag site tables
  ... --only background       # just the all-flag provenance summary
  ... --labels my_labels.txt  # extend site labels ("0xADDR free text" lines)
  ... --overlay-map FILE      # override the committed checksum->label map
  ... --json                  # machine-readable dump
  python3 analyze_reader_watch.py --gen-overlay-map extracted  # rebuild map

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
    # The field/event VM's flag-op dispatch (FUN_801DE840; every slot-A
    # sibling carries the same copy at the same VAs - jals disc-verified).
    0x801E3598: "field-VM op-0x50 SET handler return (FUN_801DE840 flag-op cluster)",
    0x801E35C0: "field-VM op-0x60 CLEAR handler return (FUN_801DE840 flag-op cluster)",
    0x801E35E8: "field-VM op-0x70 TEST handler return (FUN_801DE840 flag-op cluster)",
    0x801E26BC: "field-VM secondary TEST site (FUN_801DE840; high-byte/gate op family)",
    0x801E28A8: "field-VM secondary TEST site (FUN_801DE840; high-byte/gate op family)",
    0x801E28C4: "field-VM secondary TEST site (FUN_801DE840; high-byte/gate op family)",
    0x801D218C: "walk-on tile-trigger dispatch (FUN_801D1EC4 -> FUN_801D5630 -> FUN_8003BDE0)",
    0x8003BF78: "FUN_8003BDE0 internal gate-bit read (walk-on trigger gate check)",
    0x8003C008: "FUN_8003BDE0 internal gate-bit read (walk-on trigger gate check)",
    0x8001A8BC: "bulk lbu;sb memcpy loop (save/transfer scan of the flag bank - discard)",
    0x800583C8: "FUN_800583C8 LoadImage (libgpu RAM->VRAM)",
    0x80058490: "FUN_80058490 MoveImage (libgpu VRAM->VRAM)",
}

HELPER_PCS = {0x8003CE08, 0x8003CE34, 0x8003CE64, 0x800583C8, 0x80058490}
CONTEXT_KINDS = {"scene", "mode", "snap", "battle", "overlay"}
FLAG_KINDS = {"test", "set", "clear", "byteread"}

# Overlay slot windows: past the last known base, cap the window generously
# (the largest known overlay footprint is ~0x2A000).
OVERLAY_WINDOW_MAX = 0x38000


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
    run: str = ""


@dataclass
class Site:
    kind: str
    flag: int
    pc: int
    ra: int
    total: int = 0          # sum of per-run max counts (lower bound)
    exact: bool = True      # False once a suppressed-interval row is last
    first_tick: int = 0
    first_scene: str = ""
    scenes: set[str] = field(default_factory=set)
    tiles: set[str] = field(default_factory=set)
    rects: set[str] = field(default_factory=set)   # vram "r<x>;<y>;<w>;<h>"
    values: set[str] = field(default_factory=set)  # write "pre=../now=.." pairs
    name: str = ""                                 # write watch name
    target: bool = False
    run_max: dict = field(default_factory=dict)    # run id -> max count seen
    runs: set = field(default_factory=set)
    overlays: set = field(default_factory=set)     # resident csums at hit time
    vmops: set = field(default_factory=set)        # script op "0xVA(+0xOFF)"


def parse_rows(lines, run: str = "") -> list[Row]:
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
                count=int(parts[7]), note=note, run=run))
        except ValueError:
            continue
    return rows


def load_inputs(paths: list[str]) -> list[Row]:
    """Load one or more csvs / run directories into a single row stream.

    Directories are scanned recursively for flag_reader_watch.csv; each csv's
    rows are tagged with its run id (the parent directory name, i.e. the
    run timestamp) so merged sites keep per-run provenance.
    """
    csvs: list[Path] = []
    for p in paths:
        path = Path(p)
        if path.is_dir():
            csvs.extend(sorted(path.rglob("flag_reader_watch.csv")))
        else:
            csvs.append(path)
    rows: list[Row] = []
    for c in csvs:
        rows.extend(parse_rows(c.read_text().splitlines(), run=c.parent.name))
    return rows


def overlay_base_for(addr: int, bases: list[int]) -> int | None:
    """The overlay slot window containing addr, or None. `bases` sorted."""
    prev = None
    for b in bases:
        if addr < b:
            break
        prev = b
    if prev is None:
        return None
    idx = bases.index(prev)
    limit = bases[idx + 1] if idx + 1 < len(bases) else prev + OVERLAY_WINDOW_MAX
    return prev if addr < min(limit, prev + OVERLAY_WINDOW_MAX) else None


def collect_sites(rows: list[Row]) -> dict[tuple, Site]:
    """Aggregate hit rows into per-(kind,flag,pc,ra) sites.

    Run-aware: counts are per-run running maxima summed at the end (the
    count column resets per run), and each site records which runs saw it.
    Overlay-aware: `overlay` context rows update the per-run resident-slot
    checksums as the stream plays; a hit whose interesting address falls in
    a slot window is stamped with the checksum resident at that moment.
    """
    sites: dict[tuple, Site] = {}
    resident: dict[tuple, str] = {}  # (run, base) -> current csum
    bases: list[int] = sorted({r.pc for r in rows if r.kind == "overlay"})
    for r in rows:
        if r.kind == "overlay":
            for tok in r.note.split():
                if tok.startswith("csum="):
                    resident[(r.run, r.pc)] = tok[5:]
            continue
        if r.kind in CONTEXT_KINDS:
            continue
        key = (r.kind, r.flag, r.pc, r.ra)
        s = sites.get(key)
        if s is None:
            s = Site(kind=r.kind, flag=r.flag, pc=r.pc, ra=r.ra,
                     first_tick=r.tick, first_scene=r.scene)
            sites[key] = s
        s.run_max[r.run] = max(s.run_max.get(r.run, 0), r.count)
        s.runs.add(r.run)
        s.scenes.add(r.scene)
        if bases:
            who = r.ra if r.pc in HELPER_PCS else r.pc
            b = overlay_base_for(who, bases)
            if b is not None:
                csum = resident.get((r.run, b))
                if csum is not None:
                    s.overlays.add(csum)
        vm_va, vm_off = None, None
        for tok in r.note.split():
            if tok == "tgt":
                s.target = True
            elif tok.startswith("vm="):
                vm_va = tok[3:]
            elif tok.startswith("vmo="):
                vm_off = tok[4:]
            elif tok.startswith(("pre=", "now=")):
                s.values.add(tok)
            elif ";" in tok:
                if tok.startswith("t"):
                    s.tiles.add(tok)
                elif tok.startswith(("r", "d")):
                    s.rects.add(tok)
            elif r.kind == "write" and not s.name:
                s.name = tok
        if vm_va:
            s.vmops.add(vm_va + (f"(+{vm_off})" if vm_off else ""))
    # The probe logs every hit up to a per-class prefix (targets 8,
    # background 4) and then only every Nth - so a max count inside the
    # prefix is exact, anything past it is a lower bound. Totals sum the
    # per-run maxima (each run's count column starts fresh).
    for s in sites.values():
        s.total = sum(s.run_max.values())
        prefix = 8 if s.target else 4
        s.exact = all(v <= prefix for v in s.run_max.values())
    return sites


def label_for(addr: int, labels: dict[int, str]) -> str:
    if addr in labels:
        return labels[addr]
    return f"[NEW] uncataloged, {classify_region(addr)}"


def resident_note(s: Site, omap: dict[str, str]) -> str:
    if not s.overlays:
        return ""
    names = sorted(omap.get(c, f"csum:{c}?") for c in s.overlays)
    return " resident=[" + ", ".join(names) + "]"


def site_line(s: Site, labels: dict[int, str],
              omap: dict[str, str] | None = None, multi: bool = False) -> str:
    # For helper hits the pc IS the helper; the caller ra is the news.
    who = s.ra if s.pc in HELPER_PCS else s.pc
    cnt = f"{s.total}" if s.exact else f">={s.total}"
    tiles = f" tiles={','.join(sorted(s.tiles))}" if s.tiles else ""
    runs = f" runs={len(s.runs)}" if multi else ""
    res = resident_note(s, omap or {})
    line = (f"    {s.kind:<9} pc=0x{s.pc:08X} ra=0x{s.ra:08X} n={cnt:<7} "
            f"first@{s.first_tick}/{s.first_scene}{tiles}{runs}\n"
            f"              -> {label_for(who, labels)}{res}")
    if s.vmops:
        shown = sorted(s.vmops)
        extra = f" +{len(shown) - 6} more" if len(shown) > 6 else ""
        line += ("\n              script-ops: "
                 + " ".join(shown[:6]) + extra
                 + "  (buffer VA(+offset) of the bytecode op)")
    return line


def annotate_rect(tok: str) -> str:
    """Tag a vram rect token: single-row narrow uploads are CLUT-shaped."""
    if not tok.startswith("r"):
        return tok
    try:
        _x, y, w, h = (int(v) for v in tok[1:].split(";"))
    except ValueError:
        return tok
    if h == 1 and w <= 256:
        return tok + "[CLUT?]"
    if y >= 448:
        return tok + "[CLUT-region]"
    return tok


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


def fnv1a32(data: bytes) -> int:
    """FNV-1a 32-bit - must stay bit-identical to the probe's Lua copy."""
    h = 0x811C9DC5
    for b in data:
        h ^= b
        h = (h * 0x01000193) & 0xFFFFFFFF
    return h


OVERLAY_CSUM_BYTES = 512  # first 512 as-loaded bytes; within every clean prefix


def load_overlay_map(path: str | None) -> dict[str, str]:
    """csum(8-hex) -> overlay label. Defaults to the committed overlay-map.txt
    next to this script (regenerate with --gen-overlay-map after a TOML
    change); silently empty when absent so plain runs still work."""
    p = Path(path) if path else Path(__file__).with_name("overlay-map.txt")
    omap: dict[str, str] = {}
    if not p.exists():
        return omap
    for line in p.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        csum, _, label = line.partition(" ")
        if len(csum) == 8:
            omap[csum.lower()] = label.strip()
    return omap


def gen_overlay_map(extracted_dir: str, toml_path: str | None) -> str:
    """Regenerate overlay-map.txt content from a disc extraction: the FNV-1a
    checksum of each raw overlay's first OVERLAY_CSUM_BYTES as-loaded bytes
    (file offset 0 lands at base_va), labeled from static-overlays.toml.
    Checksums only - no Sony bytes (same policy class as the committed
    sha256 fingerprints in the TOML)."""
    import tomllib

    tp = Path(toml_path) if toml_path else (
        Path(__file__).resolve().parents[2]
        / "crates" / "asset" / "data" / "static-overlays.toml")
    overlays = tomllib.loads(tp.read_text())["overlays"]
    by_csum: dict[str, list[str]] = {}
    misses = []
    for o in overlays:
        if o.get("form") != "raw":
            continue
        hits = sorted(Path(extracted_dir, "PROT").glob(f"{o['prot_index']:04d}_*"))
        if not hits:
            misses.append(o["label"])
            continue
        data = hits[0].read_bytes()[:OVERLAY_CSUM_BYTES]
        csum = f"{fnv1a32(data):08x}"
        by_csum.setdefault(csum, []).append(
            f"{o['label']} (PROT {o['prot_index']} @0x{o['base_va']:08X})")
    lines = ["# overlay residency map: FNV-1a32 of each overlay's first "
             f"{OVERLAY_CSUM_BYTES} as-loaded bytes.",
             "# Regenerate: analyze_reader_watch.py --gen-overlay-map <extracted-dir>",
             "# Derived checksums only - no Sony bytes."]
    for csum in sorted(by_csum):
        lines.append(f"{csum} {' | '.join(by_csum[csum])}")
    for m in misses:
        lines.append(f"# missing extraction for: {m}")
    return "\n".join(lines) + "\n"


def render(rows: list[Row], labels: dict[int, str], only: str | None,
           omap: dict[str, str] | None = None) -> str:
    sites = collect_sites(rows)
    omap = omap or {}
    runs = sorted({r.run for r in rows})
    multi = len(runs) > 1
    out: list[str] = []
    scenes = []
    for r in rows:
        if r.kind == "scene" and (not scenes or scenes[-1] != r.scene):
            scenes.append(r.scene)
    span = f"{rows[0].tick}..{rows[-1].tick}" if rows else "-"
    out.append(f"ticks {span}; scenes: {' > '.join(scenes) or '-'}")
    if multi:
        out.append(f"runs merged: {len(runs)} - " + ", ".join(runs))
    totals: dict[str, int] = {}
    for s in sites.values():
        totals[s.kind] = totals.get(s.kind, 0) + s.total
    out.append("totals (lower bounds): " + " ".join(
        f"{k}={v}" for k, v in sorted(totals.items())) or "-")

    flag_sites = [s for s in sites.values() if s.kind in FLAG_KINDS]
    target_flags = sorted({s.flag for s in flag_sites
                           if s.target or s.kind == "byteread"})

    if only in (None, "targets"):
        out.append("\n== TARGET FLAGS ==")
        if not target_flags:
            out.append("  (no target-flag hits)")
        for f in target_flags:
            out.append(f"  flag 0x{f:X} ({f}):")
            fs = sorted((s for s in flag_sites if s.flag == f),
                        key=lambda s: (s.kind, s.first_tick))
            for s in fs:
                out.append(site_line(s, labels, omap, multi))
                if s.kind == "byteread":
                    out.append("              byteread covers 8 flags - verify the code at pc masks this bit")

    if only in (None, "background"):
        out.append("\n== ALL-FLAG PROVENANCE (background, deduped) ==")
        bg_flags = sorted({s.flag for s in flag_sites} - set(target_flags))
        if not bg_flags:
            out.append("  (none)")
        for f in bg_flags:
            fs = sorted((s for s in flag_sites if s.flag == f),
                        key=lambda s: (s.kind, s.first_tick))
            kinds = {}
            news = []
            resident: set[str] = set()
            vmops: set[str] = set()
            for s in fs:
                who = s.ra if s.pc in HELPER_PCS else s.pc
                kinds.setdefault(s.kind, set()).add(who)
                if who not in labels:
                    news.append(who)
                resident |= s.overlays
                vmops |= s.vmops
            desc = " ".join(
                f"{k}[{','.join(f'0x{a:08X}' for a in sorted(v))}]"
                for k, v in sorted(kinds.items()))
            mark = "  [NEW ra]" if news else ""
            res = ""
            if resident:
                names = sorted(omap.get(c, f"csum:{c}?") for c in resident)
                res = " resident=[" + ", ".join(names) + "]"
            ops = ""
            if vmops:
                shown = sorted(vmops)
                more = f" +{len(shown) - 4}" if len(shown) > 4 else ""
                ops = " script-ops: " + " ".join(shown[:4]) + more
            out.append(f"  0x{f:<5X} {desc}{res}{ops}{mark}")

    if only in (None, "writes"):
        ws = sorted((s for s in sites.values() if s.kind == "write"),
                    key=lambda s: (s.flag, s.first_tick))
        if ws:
            out.append("\n== WATCHED WRITES (P7 allowlist) ==")
            for s in ws:
                out.append(f"  {s.name or f'slot{s.flag}'}:")
                out.append(site_line(s, labels, omap, multi))
                if s.values:
                    out.append("              values: "
                               + " ".join(sorted(s.values)))

    if only in (None, "vram"):
        vs = sorted((s for s in sites.values()
                     if s.kind in ("vram", "vrammove")),
                    key=lambda s: (s.kind, s.first_tick))
        if vs:
            out.append("\n== VRAM UPLOADS (P8) ==")
            for s in vs:
                out.append(site_line(s, labels, omap, multi))
                if s.rects:
                    out.append("              rects: " + " ".join(
                        annotate_rect(t) for t in sorted(s.rects)))

    if only in (None, "battles"):
        battles = [r for r in rows if r.kind == "battle"]
        if battles:
            out.append("\n== BATTLES (P9; formation writer ra = the `form` watched write) ==")
            for r in battles:
                lone = " *boss-shaped*" if r.note.startswith(
                    f"form={r.flag:02X}000000") else ""
                out.append(f"  tick {r.tick} scene {r.scene}: {r.note}{lone}")

    if only in (None, "snaps"):
        snaps = [r for r in rows if r.kind == "snap"]
        if snaps:
            out.append("\n== SNAPSHOTS ==")
            for r in snaps:
                out.append(f"  tick {r.tick} scene {r.scene}: {r.note}")
    return "\n".join(out)


def to_json(rows: list[Row], labels: dict[int, str],
            omap: dict[str, str] | None = None) -> str:
    sites = collect_sites(rows)
    omap = omap or {}
    payload = []
    for s in sites.values():
        who = s.ra if s.pc in HELPER_PCS else s.pc
        payload.append({
            "kind": s.kind, "flag": s.flag, "pc": f"0x{s.pc:08X}",
            "ra": f"0x{s.ra:08X}", "total_min": s.total, "exact": s.exact,
            "first_tick": s.first_tick, "first_scene": s.first_scene,
            "scenes": sorted(s.scenes), "tiles": sorted(s.tiles),
            "rects": sorted(s.rects), "values": sorted(s.values),
            "name": s.name,
            "runs": sorted(s.runs),
            "resident": sorted(omap.get(c, f"csum:{c}?") for c in s.overlays),
            "script_ops": sorted(s.vmops),
            "target": s.target, "label": label_for(who, labels),
            "new": who not in labels,
        })
    payload.sort(key=lambda d: (d["flag"], d["kind"], d["first_tick"]))
    return json.dumps(payload, indent=2)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("csv", nargs="*",
                    help="flag_reader_watch.csv path(s) and/or run "
                         "directories (scanned recursively); several inputs "
                         "merge into one cumulative provenance map")
    ap.add_argument("--only",
                    choices=["targets", "background", "writes", "vram",
                             "battles", "snaps"])
    ap.add_argument("--labels", help="extra site labels: '0xADDR text' lines")
    ap.add_argument("--overlay-map",
                    help="csum->overlay label map (default: overlay-map.txt "
                         "next to this script)")
    ap.add_argument("--merged-out",
                    help="also write the merged site map as JSON to FILE")
    ap.add_argument("--gen-overlay-map", metavar="EXTRACTED_DIR",
                    help="print a fresh overlay-map.txt from a disc "
                         "extraction and exit")
    ap.add_argument("--toml", help="static-overlays.toml override "
                                   "(with --gen-overlay-map)")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args(argv)
    if args.gen_overlay_map:
        print(gen_overlay_map(args.gen_overlay_map, args.toml), end="")
        return 0
    if not args.csv:
        ap.error("at least one csv/run-directory is required")
    rows = load_inputs(args.csv)
    labels = load_labels(args.labels)
    omap = load_overlay_map(args.overlay_map)
    if args.merged_out:
        Path(args.merged_out).write_text(to_json(rows, labels, omap))
    if args.json:
        print(to_json(rows, labels, omap))
    else:
        print(render(rows, labels, args.only, omap))
    return 0


if __name__ == "__main__":
    sys.exit(main())
