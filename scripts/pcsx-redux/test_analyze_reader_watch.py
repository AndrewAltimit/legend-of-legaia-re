#!/usr/bin/env python3
"""Tests for analyze_reader_watch.py.

Dependency-free: importable by pytest (`test_*` functions) and runnable
standalone (`python3 test_analyze_reader_watch.py`) which executes every test
and exits non-zero on the first failure. Uses only synthetic in-memory CSV
rows, so it needs no capture on disk and no Sony bytes.
"""
from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    "analyze_reader_watch", Path(__file__).with_name("analyze_reader_watch.py")
)
arw = importlib.util.module_from_spec(_spec)
sys.modules["analyze_reader_watch"] = arw
_spec.loader.exec_module(arw)


HEADER = "tick,kind,flag,pc,ra,mode,scene,count,note"


def _rows(*csv_lines):
    return arw.parse_rows([HEADER, *csv_lines])


def test_parse_skips_header_junk_and_old_schema():
    rows = arw.parse_rows([
        HEADER,
        "",
        "junk",
        "10,test,488,0x8003CE64,0x801E35E8,0x03,map02,1,tgt t3;-2",
        # old 8-column schema (no note) still parses
        "20,test,488,0x8003CE64,0x801E35E8,0x03,map02,2",
    ])
    assert len(rows) == 2
    assert rows[0].note == "tgt t3;-2"
    assert rows[1].note == ""


def test_sites_dedup_and_lower_bound_counts():
    rows = _rows(
        "10,test,488,0x8003CE64,0x801E35E8,0x03,map02,1,tgt",
        "11,test,488,0x8003CE64,0x801E35E8,0x03,map02,2,tgt",
        "500,test,488,0x8003CE64,0x801E35E8,0x03,map02,64,tgt",
        "12,set,549,0x8003CE08,0x801D9000,0x03,town01,1,",
    )
    sites = arw.collect_sites(rows)
    assert len(sites) == 2
    s = sites[("test", 488, 0x8003CE64, 0x801E35E8)]
    assert s.total == 64 and not s.exact and s.target
    w = sites[("set", 549, 0x8003CE08, 0x801D9000)]
    assert w.total == 1 and w.exact and not w.target


def test_target_vs_background_split_and_new_label():
    rows = _rows(
        # target: known field-VM TEST handler ra -> labeled
        "10,test,488,0x8003CE64,0x801E35E8,0x03,map02,1,tgt",
        # background: uncataloged overlay writer -> [NEW ra]
        "20,set,1234,0x8003CE08,0x801DFFFF,0x03,stone,1,",
        "30,scene,0,0x0,0x0,0x03,stone,1,",
    )
    text = arw.render(rows, arw.load_labels(None), None)
    assert "flag 0x1E8" in text
    assert "field-VM op-0x70 TEST handler" in text
    assert "[NEW ra]" in text
    # background section keys by flag hex
    assert "0x4D2" in text


def test_byteread_is_target_class_and_flagged_for_mask_check():
    rows = _rows(
        "10,byteread,488,0x8003BF78,0x801D218C,0x03,rayman,1,tgt",
    )
    text = arw.render(rows, arw.load_labels(None), "targets")
    assert "walk-on tile-trigger dispatch" not in text  # pc labels, not ra, for non-helper hits
    assert "FUN_8003BDE0 internal gate-bit read" in text
    assert "verify the code at pc masks this bit" in text


def test_tiles_and_snap_section():
    rows = _rows(
        "10,test,488,0x8003CE64,0x801E35E8,0x03,map02,1,tgt t3;-2",
        "50,snap,1,0x0,0x0,0x03,map02,1,hit_f1E8 -> snap_0000050_hit_f1E8_map02.sstate",
    )
    sites = arw.collect_sites(rows)
    s = sites[("test", 488, 0x8003CE64, 0x801E35E8)]
    assert "t3;-2" in s.tiles
    text = arw.render(rows, arw.load_labels(None), None)
    assert "SNAPSHOTS" in text and "hit_f1E8" in text


def test_extra_labels_file(tmp_path=None):
    import tempfile

    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False) as fh:
        fh.write("# comment\n0x801DFFFF my custom writer\n")
        path = fh.name
    labels = arw.load_labels(path)
    assert labels[0x801DFFFF] == "my custom writer"
    # built-ins still present
    assert 0x8003CE64 in labels


def test_json_output_shape():
    rows = _rows(
        "10,test,488,0x8003CE64,0x801E35E8,0x03,map02,1,tgt",
    )
    import json

    payload = json.loads(arw.to_json(rows, arw.load_labels(None)))
    assert payload[0]["flag"] == 488
    assert payload[0]["target"] is True
    assert payload[0]["new"] is False


def test_watched_write_section_with_values_and_name():
    rows = _rows(
        # P7: formation-table write from the encounter launcher, committed
        # value appended at the drain
        "100,write,1,0x801DA5F8,0x801DA51C,0x03,rayman,1,tgt form pre=0x0 t3;-2 now=0x4F",
    )
    sites = arw.collect_sites(rows)
    s = sites[("write", 1, 0x801DA5F8, 0x801DA51C)]
    assert s.name == "form"
    assert {"pre=0x0", "now=0x4F"} <= s.values
    assert s.target  # tgt-class suppression tier
    text = arw.render(rows, arw.load_labels(None), "writes")
    assert "WATCHED WRITES" in text and "form" in text and "now=0x4F" in text
    # write slots must NOT leak into the flag sections
    full = arw.render(rows, arw.load_labels(None), None)
    assert "flag 0x1 (1)" not in full


def test_vram_section_clut_classification():
    rows = _rows(
        # CLUT-shaped row-479 upload + a texture page + a move
        "10,vram,0,0x800583C8,0x801E4C58,0x03,map01,1,r64;479;16;1",
        "11,vram,0,0x800583C8,0x801E4C58,0x03,map01,2,r960;256;64;64",
        "12,vrammove,0,0x80058490,0x801E4794,0x03,map01,1,r0;0;16;1 d64;479",
    )
    text = arw.render(rows, arw.load_labels(None), "vram")
    assert "VRAM UPLOADS" in text
    assert "r64;479;16;1[CLUT?]" in text
    assert "r960;256;64;64" in text and "r960;256;64;64[" not in text
    assert "FUN_800583C8 LoadImage" not in text  # helper pc labels the CALLER ra
    assert "[NEW] uncataloged, overlay" in text
    # vram rows never enter the flag sections
    assert "flag 0x0" not in arw.render(rows, arw.load_labels(None), "targets")


def test_battle_rows_render_with_boss_marker():
    rows = _rows(
        # boss-shaped (lone formation slot) + a normal 2-monster fight
        "100,battle,79,0x0,0x0,0x14,rimelm,1,form=4F000000 enter=0x08 t3;-2",
        "200,battle,18,0x0,0x0,0x14,map02,2,form=12120000 enter=0x08 batid=0x05",
    )
    text = arw.render(rows, arw.load_labels(None), "battles")
    assert "BATTLES" in text
    assert "form=4F000000" in text and "*boss-shaped*" in text
    assert "form=12120000" in text
    assert text.count("*boss-shaped*") == 1
    # battle rows are context, not flag sites
    assert "flag 0x4F" not in arw.render(rows, arw.load_labels(None), "targets")


def test_fnv1a32_reference_vectors():
    assert arw.fnv1a32(b"") == 0x811C9DC5
    assert arw.fnv1a32(b"a") == 0xE40C292C
    # cross-checked bit-identical against the probe's LuaJIT copy
    t, x = [], 12345
    for _ in range(512):
        x = (x * 75 + 74) % 65537
        t.append(x % 256)
    assert arw.fnv1a32(bytes(t)) == 0x9D7577E8


def test_merge_sums_per_run_maxima_and_tracks_runs():
    r1 = arw.parse_rows([HEADER,
        "10,test,488,0x8003CE64,0x801E35E8,0x03,map02,1,tgt",
        "90,test,488,0x8003CE64,0x801E35E8,0x03,map02,64,tgt",
    ], run="runA")
    r2 = arw.parse_rows([HEADER,
        "10,test,488,0x8003CE64,0x801E35E8,0x03,map02,7,tgt",
    ], run="runB")
    sites = arw.collect_sites(r1 + r2)
    s = sites[("test", 488, 0x8003CE64, 0x801E35E8)]
    assert s.total == 64 + 7          # per-run maxima summed
    assert not s.exact                # runA went past the logged prefix
    assert s.runs == {"runA", "runB"}
    text = arw.render(r1 + r2, arw.load_labels(None), None)
    assert "runs merged: 2" in text and "runs=2" in text


def test_overlay_residency_join_and_label():
    rows = _rows(
        # slot A resident = csum aabbccdd, then a menu swap to 11223344
        "5,overlay,0,0x801CE818,0x0,0x03,map01,1,csum=aabbccdd",
        "10,set,549,0x8003CE08,0x801D9000,0x03,map01,1,",
        "20,overlay,0,0x801CE818,0x0,0x04,map01,2,csum=11223344",
        "30,set,549,0x8003CE08,0x801D9000,0x04,map01,2,",
        # slot B window hit picks slot B residency, not slot A's
        "40,overlay,0,0x801F69D8,0x0,0x14,map01,1,csum=55667788",
        "50,test,300,0x8003CE64,0x801F7000,0x14,map01,1,",
    )
    sites = arw.collect_sites(rows)
    w = sites[("set", 549, 0x8003CE08, 0x801D9000)]
    assert w.overlays == {"aabbccdd", "11223344"}
    b = sites[("test", 300, 0x8003CE64, 0x801F7000)]
    assert b.overlays == {"55667788"}
    omap = {"aabbccdd": "field (PROT 897)", "11223344": "menu (PROT 899)"}
    text = arw.render(rows, arw.load_labels(None), None, omap)
    assert "resident=[field (PROT 897), menu (PROT 899)]" in text
    assert "csum:55667788?" in text  # unmapped csum degrades visibly


def test_overlay_base_windowing():
    bases = [0x801CE818, 0x801F69D8]
    assert arw.overlay_base_for(0x801DE840, bases) == 0x801CE818
    assert arw.overlay_base_for(0x801F7000, bases) == 0x801F69D8
    assert arw.overlay_base_for(0x8003CE64, bases) is None
    # slot A window ends at slot B's base, not at +MAX
    assert arw.overlay_base_for(0x801F69D8 - 1, bases) == 0x801CE818


def _run_all():
    mod = sys.modules[__name__]
    names = [n for n in dir(mod) if n.startswith("test_")]
    for n in names:
        getattr(mod, n)()
        print(f"ok {n}")
    print(f"{len(names)} tests passed")


if __name__ == "__main__":
    _run_all()
