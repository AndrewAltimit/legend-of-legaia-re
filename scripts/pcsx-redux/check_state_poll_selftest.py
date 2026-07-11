#!/usr/bin/env python3
"""Verify a state-poll SELF-TEST run: every stream fired, every autosnap fired.

Companion to `autorun_state_poll_selftest.lua` (which pokes every RAM cell the
community poll probe watches). This checker reads the run directory's
`state_poll.csv` and asserts:

  * every CSV `kind` the probe advertises appears at least once
    (flagset/flagclr incl. one clean AND one `bulkload`-tagged frame),
  * the scene poke produced the never-seen `zztest` scene row,
  * every expected autosnap reason fired (`flag4001`, `scene_zztest`,
    `batid`, `status400`, `artsin0`) and its `.sstate` file exists in the
    run dir,
  * phase B (battle streams: battle/status/hp/aq) ran - unless the run
    manifest shows no battle state was provided, in which case those are
    reported SKIPPED but the check still fails (a self-test without phase B
    is not a full pass; pass --allow-no-battle to downgrade to a warning).

Exit code 0 = full pass, 1 = any required stream/snap missing, 2 = usage.

Pure logic lives in importable functions (`load_rows`, `evaluate`) so
`test_check_state_poll_selftest.py` can drive them on synthetic rows with no
capture on disk. No Sony bytes involved: reads only the derived CSV.
"""
from __future__ import annotations

import argparse
import csv
import sys
from dataclasses import dataclass
from pathlib import Path

# Streams exercised by phase A (field state). `flag_clean` / `flag_bulk` are
# derived checks over flagset/flagclr notes rather than raw kinds.
PHASE_A_KINDS = [
    "flagset", "flagclr", "gold", "item", "party", "level", "spell", "xp",
    "equip", "counter", "scene", "mode", "pos", "bgm", "fmv", "wmcam", "dt",
    "input", "pick", "battleid", "snap",
]
# Streams exercised by phase B (battle state loaded mid-run).
PHASE_B_KINDS = ["battle", "status", "hp", "aq"]

# Autosnap reasons the self-test must trigger (matched as prefixes of the
# snap row's note, which is "<reason> -> <filename>").
EXPECTED_SNAPS = {
    "flag4001": "target-flag first-set autosnap (LEGAIA_SNAP_FLAGS=4001)",
    "scene_zztest": "never-seen-scene autosnap",
    "batid": "first nonzero battle-id staging autosnap",
    "status400": "first 0x400 status autosnap (phase B)",
    "artsin0": "first arts-input autosnap (phase B)",
}
PHASE_B_SNAPS = {"status400", "artsin0"}


@dataclass
class Row:
    tick: int
    kind: str
    idx: int
    value: int
    delta: int
    mode: str
    scene: str
    note: str


@dataclass
class Finding:
    name: str
    ok: bool
    detail: str
    phase_b: bool = False


def load_rows(csv_path: Path) -> list[Row]:
    rows: list[Row] = []
    with csv_path.open(newline="") as fh:
        for parts in csv.reader(fh):
            if len(parts) < 8 or parts[0] == "tick":
                continue
            try:
                rows.append(Row(int(parts[0]), parts[1], int(parts[2]),
                                int(parts[3]), int(parts[4]), parts[5],
                                parts[6], ",".join(parts[7:])))
            except ValueError:
                continue
    return rows


def evaluate(rows: list[Row], run_dir: Path | None) -> list[Finding]:
    """Assess a self-test capture. `run_dir=None` skips snapshot-file checks
    (the synthetic-row unit-test path)."""
    findings: list[Finding] = []
    kinds: dict[str, int] = {}
    for r in rows:
        kinds[r.kind] = kinds.get(r.kind, 0) + 1

    for k in PHASE_A_KINDS + PHASE_B_KINDS:
        n = kinds.get(k, 0)
        findings.append(Finding(
            f"kind:{k}", n > 0,
            f"{n} rows" if n else "NO rows",
            phase_b=k in PHASE_B_KINDS,
        ))

    flag_rows = [r for r in rows if r.kind in ("flagset", "flagclr")]
    clean = [r for r in flag_rows if "bulkload" not in r.note]
    bulk = [r for r in flag_rows if "bulkload" in r.note]
    findings.append(Finding("flag:clean-beat", len(clean) > 0,
                            f"{len(clean)} clean flag rows"))
    findings.append(Finding("flag:bulk-tagged", len(bulk) > 0,
                            f"{len(bulk)} bulkload-tagged flag rows"))

    zz = [r for r in rows if r.kind == "scene" and r.scene == "zztest"]
    findings.append(Finding("scene:zztest", len(zz) > 0,
                            "poked scene name emitted" if zz else "no zztest scene row"))

    snap_rows = [r for r in rows if r.kind == "snap"]
    for reason, why in EXPECTED_SNAPS.items():
        hits = [r for r in snap_rows if r.note.startswith(reason)]
        ok = len(hits) > 0
        detail = why if ok else f"MISSING - {why}"
        if ok and run_dir is not None:
            fname = hits[0].note.split(" -> ")[-1].strip()
            if not (run_dir / fname).exists():
                ok = False
                detail = f"snap row present but state file missing: {fname}"
        findings.append(Finding(f"snap:{reason}", ok, detail,
                                phase_b=reason in PHASE_B_SNAPS))
    return findings


def render(findings: list[Finding], phase_b_skipped: bool) -> tuple[str, bool]:
    lines = ["# state_poll self-test verdict"]
    failed = False
    for f in findings:
        if phase_b_skipped and f.phase_b:
            lines.append(f"  SKIP  {f.name:<22} (no battle state provided)")
            continue
        mark = "ok" if f.ok else "FAIL"
        if not f.ok:
            failed = True
        lines.append(f"  {mark:<4}  {f.name:<22} {f.detail}")
    lines.append("")
    lines.append("VERDICT: " + ("FAIL" if failed else
                                ("PASS (phase B skipped)" if phase_b_skipped else "PASS")))
    return "\n".join(lines), failed


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("run_dir", type=Path,
                    help="self-test run dir (contains state_poll.csv)")
    ap.add_argument("--allow-no-battle", action="store_true",
                    help="treat a missing phase B (no battle sstate) as SKIP, not FAIL")
    args = ap.parse_args()

    csv_path = args.run_dir / "state_poll.csv"
    if args.run_dir.is_file():
        csv_path, args.run_dir = args.run_dir, args.run_dir.parent
    if not csv_path.exists():
        print(f"ERROR: {csv_path} not found", file=sys.stderr)
        return 2
    rows = load_rows(csv_path)
    if not rows:
        print(f"ERROR: {csv_path} has no data rows", file=sys.stderr)
        return 1

    # Phase B counts as deliberately skipped only when the wrapper was
    # launched without a battle state AND the caller allows it.
    battle_present = any(r.kind in PHASE_B_KINDS for r in rows)
    phase_b_skipped = args.allow_no_battle and not battle_present

    findings = evaluate(rows, args.run_dir)
    report, failed = render(findings, phase_b_skipped)
    print(report)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
