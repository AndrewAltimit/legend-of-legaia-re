#!/usr/bin/env python3
"""Synthetic-row tests for check_state_poll_selftest.py (no capture needed).

Run: python3 scripts/pcsx-redux/test_check_state_poll_selftest.py
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from check_state_poll_selftest import (  # noqa: E402
    EXPECTED_SNAPS, PHASE_A_KINDS, PHASE_B_KINDS, Row, evaluate, render,
)


def _full_pass_rows() -> list[Row]:
    rows = []
    t = 100
    for k in PHASE_A_KINDS + PHASE_B_KINDS:
        if k == "snap":
            continue
        scene = "zztest" if k == "scene" else "town01"
        rows.append(Row(t, k, 0, 1, 1, "0x03", scene, ""))
        t += 10
    # one clean + one bulk flag row (the plain flagset above is the clean one)
    rows.append(Row(t, "flagset", 3840, 1, 1, "0x03", "town01", "bulkload"))
    # every expected snap reason
    for reason in EXPECTED_SNAPS:
        rows.append(Row(t, "snap", 1, 0, 0, "0x03", "town01",
                        f"{reason} -> snap_0000001_{reason}_town01.sstate"))
        t += 10
    return rows


def test_full_pass():
    findings = evaluate(_full_pass_rows(), run_dir=None)
    bad = [f for f in findings if not f.ok]
    assert not bad, f"expected full pass, got failures: {[f.name for f in bad]}"
    report, failed = render(findings, phase_b_skipped=False)
    assert not failed and "VERDICT: PASS" in report


def test_missing_stream_fails():
    rows = [r for r in _full_pass_rows() if r.kind != "aq"]
    findings = evaluate(rows, run_dir=None)
    names = {f.name: f.ok for f in findings}
    assert names["kind:aq"] is False
    _, failed = render(findings, phase_b_skipped=False)
    assert failed


def test_missing_snap_fails():
    rows = [r for r in _full_pass_rows()
            if not (r.kind == "snap" and r.note.startswith("status400"))]
    findings = evaluate(rows, run_dir=None)
    names = {f.name: f.ok for f in findings}
    assert names["snap:status400"] is False


def test_phase_b_skip_masks_battle_streams():
    rows = [r for r in _full_pass_rows()
            if r.kind not in PHASE_B_KINDS
            and not (r.kind == "snap"
                     and (r.note.startswith("status400") or r.note.startswith("artsin0")))]
    findings = evaluate(rows, run_dir=None)
    report, failed = render(findings, phase_b_skipped=True)
    assert not failed, report
    assert "PASS (phase B skipped)" in report
    # but WITHOUT the skip downgrade the same rows must fail
    _, failed_strict = render(findings, phase_b_skipped=False)
    assert failed_strict


def test_bulk_only_flags_fail_clean_check():
    rows = _full_pass_rows()
    rows = [r if r.kind not in ("flagset", "flagclr") or r.note != ""
            else Row(r.tick, r.kind, r.idx, r.value, r.delta, r.mode, r.scene, "bulkload")
            for r in rows]
    findings = evaluate(rows, run_dir=None)
    names = {f.name: f.ok for f in findings}
    assert names["flag:clean-beat"] is False


def main() -> int:
    fails = 0
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            try:
                fn()
                print(f"ok   {name}")
            except AssertionError as e:
                print(f"FAIL {name}: {e}")
                fails += 1
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
