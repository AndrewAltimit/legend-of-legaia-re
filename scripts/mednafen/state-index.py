#!/usr/bin/env python3
"""Index every emulator save state on this machine by scene x game mode.

The save-state corpus grows one capture at a time and is not indexed by
anything: mednafen names its slots `<title>.<hash>.mc{0..9}`, PCSX-Redux names
its `SCUS94254.sstate{0..9}`, and the repo's capture runs drop `autosave_a/b`
pairs plus scene-tagged `snap_*` files under `captures/`. None of those names is
authoritative about which scene the state is actually in - a `snap_*` filename
carries the scene the *probe* tagged it with, and an `autosave_*` carries
nothing at all.

This sweep answers it from the bytes: both `mednafen-state identify` and
`pcsxr-state identify` read the same Legaia anchors out of the state's main RAM
(`legaia_mednafen::game_anchors`), so the two corpora index into one table. That
makes "is there a state inside scene X?" a lookup instead of a play session,
which is the question that gates every display-list read.

Usage:

    scripts/mednafen/state-index.py                     # sweep default roots
    scripts/mednafen/state-index.py --scene teien       # only rows in one scene
    scripts/mednafen/state-index.py --json out.json     # machine-readable
    scripts/mednafen/state-index.py --root /some/dir    # extra search root

Memory cards are skipped by design: `~/.mednafen/sav/*.mcr` are 128 KiB PSX
memory-card images (`MC` magic), not save states. They carry save blocks, not
main RAM, so no scene anchor and no display list can be read from one. The
sweep reports how many it skipped so the distinction stays visible.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

# Where states accumulate. Every root is optional - a machine that never ran
# PCSX-Redux simply contributes no rows.
DEFAULT_ROOTS = [
    Path.home() / ".mednafen" / "mcs",
    Path.home() / ".config" / "pcsx-redux",
    Path.home() / "Tools" / "pcsx-redux",
    REPO / "captures",
]

MEDNAFEN_SUFFIXES = tuple(f".mc{i}" for i in range(10))
# PCSX-Redux writes `.sstate`, `.sstate0`..`.sstate9`, and occasionally
# `.sstate.sstate` when a name already carrying the suffix is re-saved.
PCSXR_MARKER = ".sstate"

# PSX memory card: exactly 128 KiB opening with the "MC" block-allocation magic.
MEMCARD_BYTES = 131072


def classify(path: Path) -> str | None:
    """Return 'mednafen' / 'pcsx-redux' / None for a candidate file."""
    name = path.name
    if name.endswith(MEDNAFEN_SUFFIXES):
        return "mednafen"
    if PCSXR_MARKER in name:
        return "pcsx-redux"
    return None


def is_memory_card(path: Path) -> bool:
    try:
        if path.stat().st_size != MEMCARD_BYTES:
            return False
        with path.open("rb") as fh:
            return fh.read(2) == b"MC"
    except OSError:
        return False


def collect(roots: list[Path]) -> tuple[dict[str, list[Path]], int]:
    """Walk the roots and bucket every state file by emulator."""
    found: dict[str, list[Path]] = {"mednafen": [], "pcsx-redux": []}
    memcards = 0
    seen: set[Path] = set()
    for root in roots:
        if not root.exists():
            continue
        for path in sorted(root.rglob("*")):
            if not path.is_file():
                continue
            resolved = path.resolve()
            if resolved in seen:
                continue
            if is_memory_card(path):
                memcards += 1
                seen.add(resolved)
                continue
            kind = classify(path)
            if kind is None:
                continue
            seen.add(resolved)
            found[kind].append(path)
    return found, memcards


def run_identify(binary: Path, paths: list[Path], chunk: int = 40) -> list[dict]:
    """Invoke `<binary> identify --json` in chunks; tolerate a failing chunk."""
    rows: list[dict] = []
    for i in range(0, len(paths), chunk):
        batch = paths[i : i + chunk]
        cmd = [str(binary), "identify", "--json", *[str(p) for p in batch]]
        proc = subprocess.run(cmd, capture_output=True, text=True)
        # A non-zero exit still leaves the per-state rows it managed to print on
        # stdout, so parse regardless and only warn.
        if proc.returncode != 0 and not proc.stdout.strip():
            print(
                f"[state-index] {binary.name} failed on a batch of "
                f"{len(batch)}: {proc.stderr.strip()[:200]}",
                file=sys.stderr,
            )
            continue
        for line in proc.stdout.splitlines():
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return rows


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--root", action="append", type=Path, default=[],
                    help="extra directory to sweep (repeatable)")
    ap.add_argument("--scene", help="only print rows whose scene matches")
    ap.add_argument("--json", type=Path, help="also write the full index as JSON")
    ap.add_argument("--target-dir", type=Path,
                    default=REPO / "target" / "release",
                    help="where mednafen-state / pcsxr-state live")
    args = ap.parse_args()

    med_bin = args.target_dir / "mednafen-state"
    pcsx_bin = args.target_dir / "pcsxr-state"
    for b in (med_bin, pcsx_bin):
        if not b.exists():
            print(
                f"[state-index] missing {b} - build with:\n"
                f"    cargo build --release -p legaia-mednafen -p legaia-pcsxr",
                file=sys.stderr,
            )
            return 2

    # The PCSX-Redux reader locates main RAM by searching for a string known to
    # live in the loaded SCUS region, so it needs the extracted executable.
    if "LEGAIA_SCUS" not in os.environ:
        candidate = REPO / "extracted" / "SCUS_942.54"
        if candidate.exists():
            os.environ["LEGAIA_SCUS"] = str(candidate)

    roots = DEFAULT_ROOTS + list(args.root)
    found, memcards = collect(roots)

    rows: list[dict] = []
    rows += run_identify(med_bin, found["mednafen"])
    rows += run_identify(pcsx_bin, found["pcsx-redux"])

    for r in rows:
        if not r.get("scene"):
            r["scene"] = "(none)"

    if args.scene:
        shown = [r for r in rows if r.get("scene") == args.scene]
    else:
        shown = rows
    shown.sort(key=lambda r: (r.get("scene", ""), r.get("file", "")))

    print(f"[state-index] roots: {', '.join(str(r) for r in roots if r.exists())}")
    print(
        f"[state-index] {len(found['mednafen'])} mednafen + "
        f"{len(found['pcsx-redux'])} pcsx-redux states; "
        f"{memcards} memory cards skipped (not states)"
    )
    print()
    print(f"{'SCENE':<10} {'MODE':<14} {'PLAYER':<16} {'EMU':<11} FILE")
    for r in shown:
        if "error" in r:
            print(f"{'!':<10} {'unreadable':<14} {'-':<16} "
                  f"{r.get('emulator', '?'):<11} {r['file']}")
            continue
        pos = "-" if r.get("player") is None else f"x={r['player'][0]} z={r['player'][1]}"
        print(f"{r['scene']:<10} {r.get('mode_label', '?'):<14} {pos:<16} "
              f"{r.get('emulator', '?'):<11} {r['file']}")

    # Scene coverage roll-up: the actual product of this sweep.
    print()
    counts: dict[str, int] = {}
    for r in rows:
        if "error" in r:
            continue
        counts[r["scene"]] = counts.get(r["scene"], 0) + 1
    print(f"[state-index] {len(counts)} distinct scenes covered")
    for scene, n in sorted(counts.items(), key=lambda kv: (-kv[1], kv[0])):
        print(f"    {scene:<12} {n}")

    if args.json:
        args.json.write_text(json.dumps(rows, indent=2))
        print(f"\n[state-index] wrote {args.json}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
