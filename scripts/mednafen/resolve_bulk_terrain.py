#!/usr/bin/env python3
"""
resolve_bulk_terrain.py

Mednafen-state companion to ``scripts/pcsx-redux/resolve_actor_tmds.py``.

The world-map landmark pack contains 40-56 TMDs per kingdom; the
MAN-records table only nails down a handful per kingdom (~5-17) with
literal world coordinates. The rest are positioned by runtime code -
either MAN's FieldVM prescripts (``FUN_801DE840`` invoked from
``FUN_8003A1E4``) or other actor-spawn paths in the world-map overlay.
The static MAN walker can't tell us where those landed without porting
the whole FieldVM driver; the practical alternative is to snapshot
live actor state out of a save state and reverse-map each actor's
mesh chain (``actor[+0x44]``) back to its source slot in the kingdom
TMD pack.

This script is the mednafen-save-state version of that resolver. It
emits ``site/world-overview-live.json`` (the file
``extract-world-placements.py`` already merges per-kingdom into
``site/world-overview.json``) augmented with three additions vs the
PCSX-Redux version:

  1. Each placement gets a ``kind`` tag (``bulk_terrain`` vs
     ``man_actor``) based on whether the actor's ``actor[+0x90]``
     points into the MAN buffer at ``_DAT_8007B898``.
  2. Atmospheric actors (``tick == FUN_801E3E00`` at ``0x801E3E00``)
     surface their live ``actor[+0x74]`` u24 RGB as the kingdom's
     ``fog_color`` (per-kingdom haze, set by the world-map overlay's
     atmospheric script). When no atmospheric tick is captured, the
     viewer falls back to its hardcoded ``KINGDOM_FOG_TINT``.
  3. The full MAN buffer pointer ``_DAT_8007B898`` and disc-side
     record count are surfaced so the viewer can cross-reference
     actors to their MAN-record names without re-doing the parse.

USAGE
    scripts/mednafen/resolve_bulk_terrain.py \\
        --bundles map01,map02,map03 \\
        --json site/world-overview-live.json \\
        path/to/Legend\\ of\\ Legaia\\ \\(USA\\).{hash}.mc1 \\
        path/to/Legend\\ of\\ Legaia\\ \\(USA\\).{hash}.mc2 \\
        path/to/Legend\\ of\\ Legaia\\ \\(USA\\).{hash}.mc3

Requires:
  * ``target/release/mednafen-state`` built
    (``cargo build --release -p legaia-mednafen``)
  * ``target/release/lzs-decode`` built
  * ``extracted/`` populated by ``legaia-extract``
"""
from __future__ import annotations

import argparse
import json
import re
import struct
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(SCRIPTS_DIR / "pcsx-redux"))
from match_prim_groups_to_disc import (  # type: ignore  # noqa: E402
    find_asset_table,
    lzs_decompress,
)
from resolve_actor_tmds import (  # type: ignore  # noqa: E402
    KINGDOM_BASE,
    LIST_HEAD_ADDRS,
    PSX_BASE,
    find_containing_tmd,
    find_landmark_load_base,
    load_kingdom_pack,
    mesh_chain_ptrs,
    read_i16,
    read_u32,
    tmd_addr_to_slot,
)

# Mednafen extracts only the 2 MiB of physical PSX main RAM; the imported
# helpers default to the PCSX-Redux 8 MiB dump shape. Override the module
# global so range checks behave on a 2 MiB blob.
PSX_PHYS_RAM_SIZE = 0x0020_0000
import match_prim_groups_to_disc  # type: ignore  # noqa: E402
import resolve_actor_tmds  # type: ignore  # noqa: E402
match_prim_groups_to_disc.RAM_SIZE = PSX_PHYS_RAM_SIZE
resolve_actor_tmds.RAM_SIZE = PSX_PHYS_RAM_SIZE

# Tick function pointer for the world-map atmosphere actor
# (FUN_801E3E00 in overlay_world_map_801e3e00.txt). The atmospheric
# script interpolates fog RGB into actor[+0x74] per frame.
ATMOSPHERIC_TICK = 0x801E3E00

# Address of the SCUS global pointer holding the decompressed MAN buffer.
# Written by FUN_8001F05C case 3 (MAN asset loader).
MAN_BUFFER_PTR_ADDR = 0x8007B898

# `mednafen-state prim-trace` stdout line we mine for tile clusters.
_CLUSTER_LINE_RE = re.compile(
    r"#(\d+)\s+clut=0x([0-9A-Fa-f]+)\s+tpage=0x([0-9A-Fa-f]+)\s+uvs=\[\(([0-9, ]+)\)"
    r".*?hits=(\d+)"
)

# `mednafen-state world-map-camera` text-form fields we mine for the
# top-view camera anchor and zoom.
_CAM_FIELD_RE = {
    "raw_x":   re.compile(r"_DAT_80089120\s+raw i32\s+=\s+(-?\d+)"),
    "raw_z":   re.compile(r"_DAT_80089118\s+raw i32\s+=\s+(-?\d+)"),
    "azimuth": re.compile(r"_DAT_8007B794\s+azimuth\s+\(low u16\)\s+=\s+0x([0-9A-Fa-f]+)"),
    "zoom":    re.compile(r"_DAT_8007B6F4\s+zoom/mode\s+\(low u16\)\s+=\s+0x([0-9A-Fa-f]+)"),
}


# ----- VRAM CLUT sampling for ocean colour ------------------------------

def _decode_tpage(tpage: int) -> tuple[int, int, int]:
    """Returns (page_x, page_y, bpp) for a PSX tpage word."""
    page_x = (tpage & 0xF) * 64
    page_y = ((tpage >> 4) & 1) * 256
    bpp = {0: 4, 1: 8, 2: 16}.get((tpage >> 5) & 3, 4)
    return page_x, page_y, bpp


def _clut_xy(clut: int) -> tuple[int, int]:
    return (clut & 0x3F) * 16, (clut >> 6) & 0x1FF


def _bgr555_to_rgb(p: int) -> tuple[int, int, int] | None:
    if p == 0:
        return None
    return ((p & 0x1F) << 3, ((p >> 5) & 0x1F) << 3, ((p >> 10) & 0x1F) << 3)


def _sample_tile(vram: bytes, tpage: int, clut: int, ux: int, uy: int,
                 w: int = 32, h: int = 32) -> Counter:
    """Sample a 4bpp tile from VRAM via its CLUT. Returns a colour histogram."""
    page_x, page_y, bpp = _decode_tpage(tpage)
    if bpp != 4:
        return Counter()
    cx, cy = _clut_xy(clut)
    palette: list[tuple[int, int, int] | None] = []
    base = (cy * 1024 + cx) * 2
    for i in range(16):
        off = base + i * 2
        if off + 2 > len(vram):
            palette.append(None)
            continue
        bgr = vram[off] | (vram[off + 1] << 8)
        palette.append(_bgr555_to_rgb(bgr))
    out: Counter = Counter()
    for v in range(uy, uy + h):
        ry = page_y + v
        if ry < 0 or ry >= 512:
            continue
        row = ry * 1024 * 2
        for u in range(ux, ux + w):
            cx2 = page_x + u // 4
            if cx2 < 0 or cx2 >= 1024:
                continue
            off = row + cx2 * 2
            if off + 2 > len(vram):
                continue
            hw = vram[off] | (vram[off + 1] << 8)
            nibble = (hw >> ((u & 3) * 4)) & 0xF
            rgb = palette[nibble]
            if rgb is not None:
                out[rgb] += 1
    return out


def _avg_top(counter: Counter, top: int = 5) -> tuple[int, int, int] | None:
    rs = gs = bs = w = 0
    for (r, g, b), n in counter.most_common(top):
        rs += r * n
        gs += g * n
        bs += b * n
        w += n
    return None if w == 0 else (rs // w, gs // w, bs // w)


def _blue_score(rgb: tuple[int, int, int]) -> int:
    """Positive for blue-dominant tiles; magnitude = saturation toward blue.

    Ocean tiles read as ``(r,g,b)`` where ``b`` clearly dominates ``r``;
    rejecting pure black (palette index 0 leaking through ``_sample_tile``)
    keeps stripe-of-transparent CLUTs from winning the rank."""
    r, g, b = rgb
    if b < 24:
        return 0
    return max(0, b - max(r, g - 8))


def pick_ocean_color(save: Path, mednafen_state_bin: Path,
                     verbose: bool = True) -> dict | None:
    """Run ``mednafen-state prim-trace`` + ``vram-dump`` and choose the
    POLY_FT4 cluster whose dominant texel colour is most strongly blue,
    weighted by hit count. Returns ``{ocean_clut, ocean_tpage, ocean_uv,
    ocean_hits, ocean_color_rgb, ocean_color_normalized}`` or ``None`` if
    no candidate qualifies.

    The walk-view prim pool already includes the kingdom's ocean tiles
    (the user's save states are walk-view with ocean visible on screen),
    so this works without dev-menu top-view captures."""
    # 1. Cluster list (stdout text — mednafen-state JSON omits non-matching
    # clusters, which we still want here).
    proc = subprocess.run([
        str(mednafen_state_bin), "prim-trace",
        str(save), "--top", "300",
    ], capture_output=True, text=True)
    if proc.returncode != 0:
        if verbose:
            print(f"  ocean: prim-trace failed: {proc.stderr.strip()}",
                  flush=True)
        return None
    clusters: list[tuple[int, int, int, int, int, int]] = []
    for line in proc.stdout.splitlines():
        m = _CLUSTER_LINE_RE.search(line)
        if not m:
            continue
        idx = int(m.group(1))
        clut = int(m.group(2), 16)
        tpage = int(m.group(3), 16)
        uvs = [int(x) for x in m.group(4).replace(" ", "").split(",")]
        hits = int(m.group(5))
        clusters.append((idx, clut, tpage, uvs[0], uvs[1], hits))
    if not clusters:
        return None

    # 2. VRAM blob via vram-dump --out-bin (PNG is a side effect).
    with tempfile.NamedTemporaryFile(suffix=".png", delete=False) as png_t:
        png_out = Path(png_t.name)
    with tempfile.NamedTemporaryFile(suffix=".bin", delete=False) as bin_t:
        bin_out = Path(bin_t.name)
    try:
        proc = subprocess.run([
            str(mednafen_state_bin), "vram-dump",
            str(save), "--out", str(png_out), "--out-bin", str(bin_out),
        ], capture_output=True, text=True)
        if proc.returncode != 0:
            return None
        vram = bin_out.read_bytes()
    finally:
        png_out.unlink(missing_ok=True)
        bin_out.unlink(missing_ok=True)

    # 3. Sample each cluster + score.
    scored: list[tuple[int, int, int, int, int, int, tuple[int, int, int], int, int]] = []
    for idx, clut, tpage, ux, uy, hits in clusters:
        counter = _sample_tile(vram, tpage, clut, ux, uy)
        avg = _avg_top(counter, 5)
        if avg is None:
            continue
        bsc = _blue_score(avg)
        if bsc == 0:
            continue
        scored.append((idx, clut, tpage, ux, uy, hits, avg, bsc, hits * bsc))
    if not scored:
        return None
    scored.sort(key=lambda c: -c[8])
    idx, clut, tpage, ux, uy, hits, avg, _bsc, score = scored[0]
    r, g, b = avg
    return {
        "ocean_clut": clut,
        "ocean_tpage": tpage,
        "ocean_uv": [ux, uy],
        "ocean_hits": hits,
        "ocean_color_rgb": [r, g, b],
        "ocean_color_hex": f"#{r:02X}{g:02X}{b:02X}",
        "ocean_color_normalized": [round(r / 255.0, 4),
                                    round(g / 255.0, 4),
                                    round(b / 255.0, 4)],
        "ocean_cluster_score": score,
        "source": "blue-weighted top POLY_FT4 cluster, CLUT-sampled from save VRAM",
    }


# ----- Top-view camera capture -----------------------------------------

def capture_topview_cam(save: Path, mednafen_state_bin: Path,
                        verbose: bool = True) -> dict | None:
    """Capture the per-kingdom camera-state globals out of the save state.

    These are useful as top-view-camera defaults regardless of whether the
    dev-menu top-view flag (``DAT_801F2B94``) is set: ``_DAT_80089118/20``
    hold the negated map-origin coords (camera target = ``-_DAT``) and
    ``_DAT_8007B794/6F4`` carry azimuth + zoom.

    Returns ``{cam_x, cam_z, azimuth, zoom, source}`` or ``None``."""
    proc = subprocess.run([
        str(mednafen_state_bin), "world-map-camera", str(save),
    ], capture_output=True, text=True)
    if proc.returncode != 0:
        if verbose:
            print(f"  topview cam: capture failed: {proc.stderr.strip()}",
                  flush=True)
        return None
    fields: dict[str, int] = {}
    for key, regex in _CAM_FIELD_RE.items():
        m = regex.search(proc.stdout)
        if m:
            fields[key] = (int(m.group(1)) if key.startswith("raw")
                           else int(m.group(1), 16))
    if "raw_x" not in fields or "raw_z" not in fields:
        return None
    return {
        "cam_x": -fields["raw_x"],
        "cam_z": -fields["raw_z"],
        "azimuth": fields.get("azimuth", 0),
        "zoom": fields.get("zoom", 0x170),
        "source": "mednafen-state world-map-camera (per-kingdom save)",
    }


def extract_mednafen_ram(save: Path, mednafen_state_bin: Path) -> bytes:
    """Use ``mednafen-state extract`` to spill the save's 2 MiB main RAM
    out as a flat blob (the same format ``resolve_actor_tmds`` expects)."""
    with tempfile.NamedTemporaryFile(suffix=".bin", delete=False) as t:
        out_path = Path(t.name)
    try:
        cmd = [
            str(mednafen_state_bin),
            "extract",
            "--start", "0x80000000",
            "--end", "0x80200000",
            "--out", str(out_path),
            str(save),
        ]
        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode != 0:
            raise RuntimeError(
                f"mednafen-state extract failed for {save}:\n"
                f"  stdout: {r.stdout}\n  stderr: {r.stderr}"
            )
        blob = out_path.read_bytes()
        if len(blob) != PSX_PHYS_RAM_SIZE:
            raise RuntimeError(
                f"unexpected RAM size {len(blob):#x} from {save}"
            )
        return blob
    finally:
        out_path.unlink(missing_ok=True)


def parse_man_records(man: bytes) -> tuple[int, int, int, list[dict]]:
    """Walk the MAN buffer and return ``(a, b, c, records)``. Records are
    the placement-walker subset (``s4 in [1, total - a)`` -> ``a3 in
    [a+1, total)``), matching ``extract-world-placements.py``'s
    `parse_placements`."""
    hdr = 0x22
    a, b, c = struct.unpack_from("<3h", man, hdr)
    total = a + b + c
    off_tbl = hdr + 9
    offsets = [
        man[off_tbl + i * 3]
        | (man[off_tbl + i * 3 + 1] << 8)
        | (man[off_tbl + i * 3 + 2] << 16)
        for i in range(total)
    ]
    data_area = off_tbl + total * 3
    records: list[dict] = []
    for s4 in range(1, total - a):
        a3 = a + s4
        if a3 >= total:
            break
        rec_off = data_area + offsets[a3]
        next_off = (
            data_area + offsets[a + s4 + 1]
            if a + s4 + 1 < total
            else len(man)
        )
        n_chars = man[rec_off]
        name_end = rec_off + 1 + 2 * n_chars
        s1 = name_end
        if s1 + 4 > len(man):
            break
        try:
            name = man[rec_off + 1:name_end].decode("shift_jis", errors="replace")
        except Exception:
            name = repr(man[rec_off + 1:name_end])
        tmd, flag, x_enc, z_enc = man[s1], man[s1 + 1], man[s1 + 2], man[s1 + 3]
        script_positioned = (x_enc == 0x7F and z_enc == 0x7F)
        records.append({
            "id": s4,
            "name": name,
            "rec_off": rec_off,
            "rec_end": next_off,
            "tmd_slot": tmd,
            "flag": flag,
            "x_enc": x_enc,
            "z_enc": z_enc,
            "script_positioned": script_positioned,
        })
    return a, b, c, records


def load_disc_man(prot_dir: Path, bundle: str, lzs_bin: Path) -> bytes | None:
    """LZS-decompress the kingdom's MAN slot (slot 2, type 0x03) off disc.

    We keep this disc-sourced so the records we cross-reference are the
    canonical (untouched) ones - the RAM copy may have been mutated by
    the FieldVM after init."""
    prot_base = KINGDOM_BASE[bundle]
    matches = sorted(prot_dir.glob(f"{prot_base:04d}_*.BIN"))
    if not matches:
        return None
    raw = matches[0].read_bytes()
    table_off = find_asset_table(raw)
    if table_off is None:
        return None
    table = raw[table_off:]
    ts = struct.unpack_from("<I", table, 8 + 2 * 8)[0]
    do = struct.unpack_from("<I", table, 8 + 2 * 8 + 4)[0]
    slot_size = ts & 0xFFFFFF
    return lzs_decompress(table[do:], slot_size)


def collect_actors(ram: bytes) -> list[dict]:
    """Walk every actor list head; return one dict per actor with the
    fields we need for bulk-terrain resolution."""
    out = []
    seen = set()
    for head_addr in LIST_HEAD_ADDRS:
        node = read_u32(ram, head_addr)
        while node and node != 0xFFFFFFFF and node not in seen:
            seen.add(node)
            nxt = read_u32(ram, node + 0x00)
            if nxt is None:
                break
            out.append(dict(
                node=node,
                list_head=head_addr,
                tick=read_u32(ram, node + 0x0C) or 0,
                flags=read_u32(ram, node + 0x10) or 0,
                mesh_head=read_u32(ram, node + 0x44) or 0,
                script_ptr=read_u32(ram, node + 0x90) or 0,
                x=read_i16(ram, node + 0x14) or 0,
                y=read_i16(ram, node + 0x16) or 0,
                z=read_i16(ram, node + 0x18) or 0,
                c74=read_u32(ram, node + 0x74) or 0,
                render_mode=read_u32(ram, node + 0x56) or 0,
            ))
            node = nxt
    return out


def find_record_for_actor(actor_script_ptr: int, man_buffer_ram_base: int,
                          man_buffer_len: int, records: list[dict]) -> dict | None:
    """Match actor[+0x90] against the MAN buffer's record ranges."""
    if actor_script_ptr < man_buffer_ram_base:
        return None
    rel = actor_script_ptr - man_buffer_ram_base
    if rel >= man_buffer_len:
        return None
    for r in records:
        if r["rec_off"] == rel:
            return r
    for r in records:
        if r["rec_off"] <= rel < r["rec_end"]:
            return r
    return None


def run_one_kingdom(save: Path, bundle: str, extracted_root: Path,
                    mednafen_state_bin: Path, lzs_bin: Path,
                    prot_dir: Path, verbose: bool = True) -> dict | None:
    if verbose:
        print(f"\n=== {save.name} -> {bundle} ===", flush=True)
    ram = extract_mednafen_ram(save, mednafen_state_bin)

    pack, byte_offsets, fname = load_kingdom_pack(extracted_root, bundle)
    if pack is None or byte_offsets is None:
        print(f"  !! could not load kingdom pack for {bundle}", flush=True)
        return None

    load_base = find_landmark_load_base(ram, pack, byte_offsets)
    if load_base is None:
        # Fallback: try multiple sample positions and pick whichever
        # the live RAM still matches. Body bytes near vert/norm pointer-
        # fixup regions get rewritten at load, so the 80% sample can
        # miss when the pack has many TMDs - Karisto's 56 vs Drake's 40
        # exercises this. Walk samples across the first few TMD bodies
        # until one resolves.
        for tmd_i in range(min(8, len(byte_offsets))):
            start = byte_offsets[tmd_i]
            end = (byte_offsets[tmd_i + 1] if tmd_i + 1 < len(byte_offsets)
                   else len(pack))
            body = pack[start:end]
            if len(body) < 200:
                continue
            for frac in (40, 50, 60, 70, 75, 80, 85, 90):
                so = (len(body) * frac) // 100
                if so + 64 >= len(body):
                    continue
                sample = bytes(body[so:so + 64])
                pos = ram.find(sample)
                if pos >= 0:
                    load_base = (PSX_BASE + pos) - start - so
                    break
            if load_base is not None:
                break
        if load_base is None:
            print(f"  !! could not locate landmark pack in RAM", flush=True)
            return None
    if verbose:
        print(
            f"  landmark pack: {fname}  {len(byte_offsets)} TMDs, "
            f"loaded at RAM {load_base:#010x}",
            flush=True,
        )

    man_disc = load_disc_man(prot_dir, bundle, lzs_bin)
    if man_disc is None:
        print(f"  !! could not load MAN slot for {bundle}", flush=True)
        return None
    a, b, c, records = parse_man_records(man_disc)
    sentinel_count = sum(1 for r in records if r["script_positioned"])

    man_base = read_u32(ram, MAN_BUFFER_PTR_ADDR) or 0
    if man_base < PSX_BASE:
        man_base = 0
    if verbose:
        print(
            f"  MAN: disc={len(man_disc)} bytes, RAM @ {man_base:#010x}, "
            f"{len(records)} records ({sentinel_count} 0x7F-sentinel)",
            flush=True,
        )

    actors = collect_actors(ram)
    placements: list[dict] = []
    slot_use: Counter[int] = Counter()
    unresolved_total = 0
    total_ptrs = 0
    bulk_terrain_count = 0
    man_actor_count = 0
    fog_color_pick: int | None = None
    fog_actor_count = 0

    for actor in actors:
        ptrs = mesh_chain_ptrs(ram, actor["mesh_head"])
        slots: list[int] = []
        unresolved = 0
        for p in ptrs:
            total_ptrs += 1
            tmd_addr = find_containing_tmd(ram, p)
            slot = tmd_addr_to_slot(tmd_addr, load_base, byte_offsets)
            if slot >= 0:
                slots.append(slot)
                slot_use[slot] += 1
            else:
                unresolved += 1
                unresolved_total += 1
        unique_slots = sorted(set(slots))

        rec = None
        if man_base and actor["script_ptr"]:
            rec = find_record_for_actor(actor["script_ptr"], man_base,
                                         len(man_disc), records)
        if unique_slots:
            kind = "man_actor" if rec is not None else "bulk_terrain"
            if kind == "bulk_terrain":
                bulk_terrain_count += 1
            else:
                man_actor_count += 1
            entry = {
                "node": f"{actor['node']:#010x}",
                "kind": kind,
                "pos": [actor["x"], actor["y"], actor["z"]],
                "slots": unique_slots,
                "tick": f"{actor['tick']:#010x}",
                "list_head": f"{actor['list_head']:#010x}",
                "flags": actor["flags"],
                "render_mode": actor["render_mode"],
                "chain_size": len(ptrs),
                "unresolved": unresolved,
            }
            if rec is not None:
                entry["man_record_index"] = rec["id"]
                entry["man_record_name"] = rec["name"]
                entry["script_positioned"] = rec["script_positioned"]
            placements.append(entry)

        # Atmospheric actor capture (FUN_801E3E00 tick): live actor[+0x74]
        # holds the kingdom's interpolated fog colour as a packed
        # 0x00BBGGRR u32. We surface the first non-zero sample.
        if actor["tick"] == ATMOSPHERIC_TICK and (actor["c74"] & 0xFFFFFF) != 0:
            fog_actor_count += 1
            if fog_color_pick is None:
                fog_color_pick = actor["c74"] & 0xFFFFFF

    if verbose:
        resolved = total_ptrs - unresolved_total
        print(
            f"  {len(placements)} placements ({bulk_terrain_count} "
            f"bulk_terrain, {man_actor_count} man_actor), "
            f"{resolved}/{total_ptrs} ptrs resolved",
            flush=True,
        )
        if fog_actor_count:
            print(f"  fog colour from {fog_actor_count} atmospheric tick "
                  f"actor(s): #{fog_color_pick:06x}", flush=True)
        else:
            print(f"  no atmospheric tick actor (FUN_801E3E00) found in "
                  f"this save", flush=True)

    out = {
        "bundle": bundle,
        "save": save.name,
        "man_buffer_ram_base": f"{man_base:#010x}",
        "kingdom_pack_load_base": f"{load_base:#010x}",
        "slots_in_pack": len(byte_offsets),
        "man_record_count": len(records),
        "man_sentinel_count": sentinel_count,
        "actors": placements,
        "slot_usage": {str(k): v for k, v in sorted(slot_use.items())},
    }
    if fog_color_pick is not None:
        r_byte = fog_color_pick & 0xFF
        g_byte = (fog_color_pick >> 8) & 0xFF
        b_byte = (fog_color_pick >> 16) & 0xFF
        out["fog_color"] = {
            "r": r_byte,
            "g": g_byte,
            "b": b_byte,
            "u24": fog_color_pick,
            "source": "actor[+0x74] of FUN_801E3E00 tick actor",
        }

    # Ocean colour: blue-weighted top POLY_FT4 cluster, CLUT-sampled from
    # the save's VRAM. The walk-view prim pool already contains the
    # kingdom's ocean tiles (visible on screen in the user's saves), so
    # this works without a dev-menu top-view capture.
    ocean = pick_ocean_color(save, mednafen_state_bin, verbose=verbose)
    if ocean is not None:
        if verbose:
            r, g, b = ocean["ocean_color_rgb"]
            print(f"  ocean colour: #{r:02X}{g:02X}{b:02X} "
                  f"(clut=0x{ocean['ocean_clut']:04X} "
                  f"tpage=0x{ocean['ocean_tpage']:04X} "
                  f"hits={ocean['ocean_hits']})", flush=True)
        out["ocean_color"] = ocean
    elif verbose:
        print("  ocean colour: no blue-dominant cluster found", flush=True)

    # Top-view camera anchor: per-kingdom (cam_x, cam_z, azimuth, zoom)
    # captured from the same save state. Useful as a "lock to top-view"
    # default regardless of whether the dev-menu flag is set, since the
    # raw map-origin coords vary per kingdom.
    topview = capture_topview_cam(save, mednafen_state_bin, verbose=verbose)
    if topview is not None:
        if verbose:
            print(f"  topview cam: ({topview['cam_x']}, {topview['cam_z']}) "
                  f"az=0x{topview['azimuth']:04X} "
                  f"zoom=0x{topview['zoom']:04X}", flush=True)
        out["topview_cam"] = topview
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "save",
        nargs="+",
        type=Path,
        help="One or more mednafen save state paths (.mc0..9).",
    )
    ap.add_argument(
        "--bundles",
        help="Comma-separated bundle list aligned with the positional save "
             "args. Defaults to one --bundle for every save.",
    )
    ap.add_argument(
        "--bundle",
        default="map01",
        choices=sorted(KINGDOM_BASE.keys()),
        help="Kingdom bundle for all states when --bundles isn't given.",
    )
    ap.add_argument(
        "--extracted",
        default="extracted",
        type=Path,
        help="Extracted disc root (default: extracted/).",
    )
    ap.add_argument(
        "--prot-dir",
        type=Path,
        help="Override PROT subdirectory (default: <extracted>/PROT).",
    )
    ap.add_argument(
        "--mednafen-state-bin",
        default="target/release/mednafen-state",
        type=Path,
        help="Path to the mednafen-state CLI binary.",
    )
    ap.add_argument(
        "--lzs-bin",
        default="target/release/lzs-decode",
        type=Path,
        help="Path to the lzs-decode CLI binary.",
    )
    ap.add_argument(
        "--json",
        type=Path,
        help="Output JSON path. Multi-kingdom runs write a list of bundle "
             "dicts; single-kingdom runs write a single dict.",
    )
    args = ap.parse_args()

    prot_dir = args.prot_dir or (args.extracted / "PROT")
    if not args.mednafen_state_bin.exists():
        sys.exit(
            f"mednafen-state CLI not found at {args.mednafen_state_bin}.\n"
            f"Build it first: cargo build --release -p legaia-mednafen"
        )
    if not args.lzs_bin.exists():
        sys.exit(
            f"lzs-decode CLI not found at {args.lzs_bin}.\n"
            f"Build it first: cargo build --release -p legaia-lzs"
        )

    if args.bundles:
        bundles = [b.strip() for b in args.bundles.split(",")]
        if len(bundles) != len(args.save):
            sys.exit(
                f"--bundles has {len(bundles)} entries but {len(args.save)} "
                f"save paths were given."
            )
    else:
        bundles = [args.bundle] * len(args.save)

    results = []
    for save, bundle in zip(args.save, bundles):
        r = run_one_kingdom(
            save, bundle, args.extracted, args.mednafen_state_bin,
            args.lzs_bin, prot_dir,
        )
        if r is not None:
            results.append(r)

    if not results:
        print("\nNo kingdoms resolved.", flush=True)
        return 1

    if args.json:
        if len(results) == 1:
            args.json.write_text(json.dumps(results[0], indent=2,
                                            ensure_ascii=False))
        else:
            args.json.write_text(json.dumps(results, indent=2,
                                            ensure_ascii=False))
        print(f"\nwrote {args.json} ({args.json.stat().st_size:,} bytes)",
              flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main() or 0)
