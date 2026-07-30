#!/usr/bin/env python3
"""
check-0968-residency.py - test a save state for PROT 0968 (Cort battle stage
overlay) residency in the slot-B overlay buffer.

The PROT 0968 identity thread (docs/reference/re-settled-threads.md
section "PROT 0968 - the Cort battle stage overlay") is pinned by
disassembly; the residency leg needs a capture showing:

  - the loader-B current-id tracker u32 at 0x8007BC4C reading 0x49, and
  - entry 968's bytes resident at the slot-B buffer base 0x801F69D8
    over the module's own 0xA28-byte extent.

This script reads both, plus the formation-head byte *(u8*)0x8007BD0C
(0xB4 = Cort first form, archive id 180; 0xB5 = evolved Cort, id 181 -
the stage-overlay selector in FUN_80055B6C tests 0xB5) and the
battle-stage id byte _DAT_8007B64A. To name whatever IS resident it
byte-scores the slot-B window against every extracted PROT entry in the
loader-B band 0900..0969.

Default input is the six catalogued cort_*_mid_cast scenarios (resolved
via scripts/scenarios.toml -> saves/library/mednafen/<sha>.mcr, each
file verified against its manifest fingerprint before use). All six are
mid-cast states, so all six show a cast stager 100% resident and CANNOT
show 0968 - the negative half of the measurement. Point it at a fresh
state taken at the evolved-Cort battle entry (before any special-attack
or summon cast) to close the thread. Both emulators' states work: a
`.sstate` file is read through `pcsxr-state extract` (PCSX-Redux),
anything else through `mednafen-state extract`:

    scripts/mednafen/check-0968-residency.py /path/to/new-state.mc7
    scripts/mednafen/check-0968-residency.py /path/to/cort-entry.sstate

Exit status: 0 if every examined state was read successfully (whether or
not 0968 was resident); the verdict is in the output.
"""

import hashlib
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
CLI = REPO_ROOT / "target" / "release" / "mednafen-state"
PCSXR_CLI = REPO_ROOT / "target" / "release" / "pcsxr-state"
MANIFEST = REPO_ROOT / "scripts" / "scenarios.toml"
LIBRARY = REPO_ROOT / "saves" / "library" / "mednafen"
PROT_DIR = REPO_ROOT / "extracted" / "PROT"

SLOT_B_BASE = 0x801F69D8
SLOT_B_LEN = 0x1000        # one loader-B page (2 sectors)
OWN_EXTENT_0968 = 0xA28    # 0968's own content; the tail is 0967's stale bytes
TRACKER_VA = 0x8007BC4C    # loader-B current-id (gp+0x934)
FORMATION_VA = 0x8007BD0C  # first formation monster id byte
STAGE_VA = 0x8007B64A      # battle-stage id byte read by loader sub-states 0x0E/0x10
CORT_LABELS = [
    "cort_mystic_shield_mid_cast",
    "cort_guilty_cross_mid_cast",
    "cort_mystic_circle_mid_cast",
    "cort_evolved_ultra_charge_mid_cast",
    "cort_evolved_final_crisis_mid_cast",
    "cort_evil_seru_magic_mid_cast",
]


def extract(state: Path, start: int, end: int) -> bytes:
    cli = PCSXR_CLI if state.suffix == ".sstate" else CLI
    with tempfile.NamedTemporaryFile(suffix=".bin") as tmp:
        subprocess.run(
            [str(cli), "extract", str(state),
             "--start", hex(start), "--end", hex(end), "--out", tmp.name],
            check=True, capture_output=True)
        return Path(tmp.name).read_bytes()


def match_pct(a: bytes, b: bytes, n: int) -> float:
    n = min(n, len(a), len(b))
    same = sum(1 for i in range(n) if a[i] == b[i])
    return 100.0 * same / n if n else 0.0


def band_entries():
    """Extracted PROT entries in the loader-B page band 0900..0969."""
    out = []
    for p in sorted(PROT_DIR.glob("09*_*.BIN")):
        idx = int(p.name[:4])
        if 900 <= idx <= 969:
            out.append((idx, p, p.read_bytes()))
    return out


def resolve_scenarios(labels):
    manifest = tomllib.loads(MANIFEST.read_text())
    by_label = {s["label"]: s for s in manifest["scenarios"]}
    resolved = []
    for label in labels:
        scn = by_label.get(label)
        if scn is None:
            sys.exit(f"scenario not in manifest: {label}")
        fp = scn["backup_fingerprint"]
        path = LIBRARY / f"{fp}.mcr"
        if not path.exists():
            sys.exit(f"library state missing: {path}")
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != fp:
            sys.exit(f"fingerprint mismatch for {label}: {actual} != {fp}")
        resolved.append((label, path))
    return resolved


def main() -> int:
    if not CLI.exists():
        sys.exit("build first: cargo build --release -p legaia-mednafen")
    if any(Path(a).suffix == ".sstate" for a in sys.argv[1:]) \
            and not PCSXR_CLI.exists():
        sys.exit("build first: cargo build --release -p legaia-pcsxr")
    e968 = (PROT_DIR / "0968_xxx_dat.BIN").read_bytes()
    band = band_entries()

    args = sys.argv[1:]
    if args:
        states = [(Path(a).name, Path(a)) for a in args]
    else:
        states = resolve_scenarios(CORT_LABELS)

    any_resident = False
    for label, path in states:
        slot_b = extract(path, SLOT_B_BASE, SLOT_B_BASE + SLOT_B_LEN)
        tracker = int.from_bytes(
            extract(path, TRACKER_VA, TRACKER_VA + 4), "little")
        formation = extract(path, FORMATION_VA, FORMATION_VA + 1)[0]
        stage = extract(path, STAGE_VA, STAGE_VA + 1)[0]

        pct_0968 = match_pct(slot_b, e968, OWN_EXTENT_0968)
        best = max(band, key=lambda e: match_pct(slot_b, e[2], SLOT_B_LEN))
        best_pct = match_pct(slot_b, best[2], SLOT_B_LEN)

        resident = tracker == 0x49 and pct_0968 > 99.0
        any_resident |= resident
        print(f"{label}")
        print(f"  state file        {path}")
        print(f"  tracker 0x8007BC4C = {tracker:#04x}"
              f"   formation head = {formation:#04x}"
              f"   stage byte = {stage:#04x}")
        print(f"  slot B vs 0968[0:0xA28]: {pct_0968:5.1f}%"
              f"   best band match: {best[0]:04d} at {best_pct:5.1f}%")
        print(f"  0968 RESIDENT: {'YES' if resident else 'no'}")
    if any_resident:
        print("\n=> residency pair observed (tracker 0x49 + 0968 bytes at "
              "0x801F69D8): the capture leg of the thread is closed.")
    else:
        print("\n=> no examined state shows 0968 resident. Mid-cast states "
              "cannot: every cast stager is a full 0x1000-byte slot-B page "
              "and evicts the stage overlay. Needed: a state at the "
              "evolved-Cort battle entry before any special-attack/summon.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
