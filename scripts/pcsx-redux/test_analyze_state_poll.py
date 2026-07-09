#!/usr/bin/env python3
"""Tests for analyze_state_poll.py.

Dependency-free: importable by pytest (`test_*` functions) and runnable
standalone (`python3 test_analyze_state_poll.py`) which executes every test and
exits non-zero on the first failure. Uses only synthetic in-memory CSV rows, so
it needs no capture on disk and no Sony bytes.
"""
from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    "analyze_state_poll", Path(__file__).with_name("analyze_state_poll.py")
)
asp = importlib.util.module_from_spec(_spec)
sys.modules["analyze_state_poll"] = asp  # dataclass forward-ref resolution needs this
_spec.loader.exec_module(asp)


HEADER = "tick,kind,idx,value,delta,mode,scene,note"


def _rows(*csv_lines):
    return asp.parse_rows([HEADER, *csv_lines])


def test_parse_skips_header_and_short_lines():
    rows = asp.parse_rows([HEADER, "10,mode,3,3,0,0x03,town01,", "", "junk", "20,gold,0,500,500,0x03,town01,"])
    assert len(rows) == 2
    assert rows[0].kind == "mode" and rows[0].mode == 0x03
    assert rows[1].kind == "gold" and rows[1].value == 500


def test_scene_timeline_collapses_runs():
    rows = _rows(
        "10,mode,3,3,0,0x03,town01,",
        "20,flagset,5,1,1,0x03,town01,",
        "30,scene,0,0,0,0x03,vozz,",
        "40,flagset,6,1,1,0x03,vozz,",
        "50,scene,0,0,0,0x03,town01,",
    )
    tl = asp.scene_timeline(rows)
    scenes = [(w.scene, w.enter_tick, w.exit_tick) for w in tl]
    assert scenes == [("town01", 10, 30), ("vozz", 30, 50), ("town01", 50, 50)]
    assert tl[0].duration == 20


def test_battle_windows_bracket_by_mode_column():
    rows = _rows(
        "10,mode,3,3,0,0x03,vozz,",
        "20,mode,20,20,0,0x14,vozz,",       # enter battle orbit
        "25,flagclr,9,0,-1,0x15,vozz,",     # still in battle (mode col 0x15)
        "30,mode,2,2,0,0x02,vozz,",         # leave
        "40,mode,20,20,0,0x14,vozz,",       # second battle
        "50,mode,3,3,0,0x03,vozz,",
    )
    bw = asp.battle_windows(rows)
    assert len(bw) == 2
    assert (bw[0].enter_tick, bw[0].exit_tick) == (20, 30)
    assert (bw[1].enter_tick, bw[1].exit_tick) == (40, 50)


def test_battle_starts_parse_formation_and_lone_flag():
    rows = _rows(
        # a lone boss: formation [4B,00,00,00], staging id captured (0x4B)
        "82963,battle,75,75,0,0x16,garmel,form=4B000000 enter=0x16",
        # a multi-enemy random: formation [1D,31,00,00], staging consumed (0)
        "75161,battle,0,29,0,0x14,garmel,form=1D310000 enter=0x08",
    )
    bs = asp.battle_starts(rows)
    assert len(bs) == 2
    boss = bs[0]
    assert boss.tick == 82963 and boss.scene == "garmel"
    assert boss.formation == [0x4B, 0, 0, 0]
    assert boss.staging_id == 75 and boss.enter_mode == 0x16
    assert boss.is_lone is True
    rnd = bs[1]
    assert rnd.formation == [0x1D, 0x31, 0, 0]
    assert rnd.staging_id == 0 and rnd.enter_mode == 0x08
    assert rnd.is_lone is False


def test_battle_starts_fallback_to_value_when_note_malformed():
    # note missing form= -> formation[0] falls back to the value column (44).
    rows = _rows("100,battle,0,44,0,0x14,vozz,enter=0x08")
    bs = asp.battle_starts(rows)
    assert len(bs) == 1
    assert bs[0].formation == [44, 0, 0, 0]
    assert bs[0].enter_mode == 0x08


def test_battle_starts_in_json_and_report():
    rows = _rows("82963,battle,75,75,0,0x16,garmel,form=4B000000 enter=0x16")
    j = asp.build_json(rows, bulk_threshold=100)
    assert j["battle_starts"][0]["formation"] == [0x4B, 0, 0, 0]
    assert j["battle_starts"][0]["is_lone"] is True
    txt = asp.render_report(rows, bulk_threshold=100, want={"battles"})
    assert "form=4B000000" in txt
    assert "*" in txt  # lone-enemy marker


def test_flag_census_excludes_bulk_and_keeps_sticky():
    # tick 100: a 3-flag bulk (threshold 3) => excluded, reported as bulk.
    # tick 200: a lone story beat (sticky). tick 300: a flag set then cleared
    # at 310 => not sticky, dropped.
    lines = [
        "100,flagset,1,1,1,0x03,opdeene,",
        "100,flagset,2,1,1,0x03,opdeene,",
        "100,flagset,3,1,1,0x03,opdeene,",
        "200,flagset,50,1,1,0x03,garmel,",
        "300,flagset,60,1,1,0x03,garmel,",
        "310,flagclr,60,0,-1,0x03,garmel,",
    ]
    cen = asp.flag_census(_rows(*lines), bulk_threshold=3)
    assert cen.bulk_ticks == [(100, "opdeene", 3)]
    beat_idxs = [b.idx for b in cen.beats]
    assert beat_idxs == [50]  # 60 dropped (ends cleared), bulk 1/2/3 excluded
    assert cen.beats[0].scene == "garmel" and cen.beats[0].set_tick == 200


def test_flag_census_churn_counted():
    lines = [
        "200,flagset,50,1,1,0x03,garmel,",
        "210,flagclr,50,0,-1,0x03,garmel,",
        "220,flagset,50,1,1,0x03,garmel,",  # ends set => sticky, churn 3
    ]
    cen = asp.flag_census(_rows(*lines), bulk_threshold=100)
    assert len(cen.beats) == 1
    assert cen.beats[0].churn == 3 and cen.beats[0].set_tick == 220


def test_item_and_gold_and_party_changes():
    rows = _rows(
        "10,item,127,3,3,0x03,town01,slot0 id00->7F",
        "20,item,127,3,0,0x03,town01,slot0",           # delta 0 => ignored
        "30,gold,0,500,500,0x03,town01,",
        "40,gold,0,500,0,0x03,town01,",                # delta 0 => ignored
        "50,party,3,3,1,0x03,town01,ids=00010200",
    )
    items = asp.item_changes(rows)
    assert len(items) == 1 and items[0].idx == 127 and items[0].delta == 3
    golds = asp.gold_changes(rows)
    assert len(golds) == 1 and golds[0].value == 500
    parties = asp.party_changes(rows)
    assert len(parties) == 1 and parties[0].note == "ids=00010200"


def test_level_and_spell_changes():
    rows = _rows(
        "10,level,0,4,1,0x15,vozz,",                    # Vahn level-up 3->4
        "20,spell,1,1,1,0x15,garmel,ids=81 lv=01",      # Noa Seru grant (Gimard)
        "30,spell,1,1,0,0x03,garmel,ids=81 lv=02",      # spell level-up (count fixed)
    )
    lvls = asp.level_changes(rows)
    assert len(lvls) == 1 and lvls[0].idx == 0 and lvls[0].value == 4
    spells = asp.spell_changes(rows)
    assert len(spells) == 2
    assert spells[0].delta == 1 and spells[0].note == "ids=81 lv=01"  # grant
    assert spells[1].delta == 0                                       # level-up
    j = asp.build_json(rows, bulk_threshold=100)
    assert j["levels"][0] == {"tick": 10, "scene": "vozz", "slot": 0, "level": 4, "delta": 1}
    assert j["spells"][0]["note"] == "ids=81 lv=01"
    txt = asp.render_report(rows, bulk_threshold=100, want={"progress"})
    assert "level-ups" in txt and "slot 0  level=4 (+1)" in txt
    assert "Seru grants" in txt and "ids=81 lv=01" in txt


def test_json_shape():
    rows = _rows(
        "10,mode,3,3,0,0x03,town01,",
        "20,mode,20,20,0,0x14,town01,",
        "30,mode,3,3,0,0x03,town01,",
        "40,flagset,50,1,1,0x03,town01,",
    )
    j = asp.build_json(rows, bulk_threshold=100)
    assert j["rows"] == 4
    assert j["tick_span"] == [10, 40]
    assert any(s["scene"] == "town01" for s in j["scenes"])
    assert len(j["battles"]) == 1
    assert j["flag_beats"][0]["flag_hex"] == "0x32"


def test_report_omits_boot_scene_and_respects_only():
    rows = _rows(
        "5,mode,0,0,0,0x00,?,",
        "10,mode,3,3,0,0x03,town01,",
        "20,flagset,50,1,1,0x03,town01,",
    )
    txt = asp.render_report(rows, bulk_threshold=100, want={"scenes"})
    assert "town01" in txt
    assert "\n  " in txt  # has a scene row
    assert "flag 0x32" not in txt  # flags section not requested
    # boot scene '?' suppressed from the scene timeline
    for line in txt.splitlines():
        assert not line.strip().endswith(" ?")


def _run_all():
    tests = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    for t in tests:
        t()
        print(f"ok  {t.__name__}")
    print(f"\n{len(tests)} passed")


if __name__ == "__main__":
    _run_all()
