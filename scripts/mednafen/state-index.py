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

Every file is classified by its **content**, never its extension, because in
this project the extension is actively misleading: `.mcr` names two opposite
things. `~/.mednafen/sav/*.mcr` and `saves/library/cards/*.mcr` are 128 KiB PSX
memory-card images (`MC` magic) carrying save blocks - no main RAM, so no scene
anchor and no display list. But `saves/library/mednafen/*.mcr` are **save
states** (gzip magic), because the backup helper keeps the source slot's
extension. A sweep that dispatches on the suffix drops the entire curated
library, which is the corpus most likely to hold the state you actually want.

So: sniff the magic, and for a gzip stream decompress the first block to tell a
mednafen `MDFNSVST` container from a PCSX-Redux protobuf. Memory cards are
skipped and counted, so the distinction stays visible in the output.
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
    # The curated library: states other oracles in this repo are pinned
    # against, deliberately captured at interesting moments. Named by
    # sha256 with the *source slot's* extension retained, so the mednafen
    # ones are `.mcr` despite being save states.
    REPO / "saves" / "library" / "mednafen",
    REPO / "saves" / "library" / "pcsx-redux",
]

# PSX memory card: exactly 128 KiB opening with the "MC" block-allocation magic.
MEMCARD_BYTES = 131072
GZIP_MAGIC = b"\x1f\x8b"
MEDNAFEN_MAGIC = b"MDFNSVST"
# Every save state embeds 2 MiB of main RAM, so even a well-compressed one runs
# north of a megabyte (observed: ~1.7 MB gzipped, ~19 MB bare). The floor is what
# keeps a source checkout out of the index - one of the default roots is
# `~/Tools/pcsx-redux`, which is the *emulator's own source tree*, and its files
# carry "PCSX" in their license headers. Sniffing content without a size floor
# indexes several hundred `.cpp` and `.md` files as save states.
MIN_STATE_BYTES = 512 * 1024


def classify(path: Path) -> str | None:
    """Return 'mednafen' / 'pcsx-redux' / 'card' / None by sniffing content.

    Extensions cannot be trusted here - `.mcr` is both a memory card and a
    mednafen save state depending on which directory it came from - so every
    decision below is made on bytes.
    """
    try:
        size = path.stat().st_size
        with path.open("rb") as fh:
            head = fh.read(4)
            if head[:2] == b"MC" and size == MEMCARD_BYTES:
                return "card"
            if size < MIN_STATE_BYTES:
                return None
            if head[:2] == GZIP_MAGIC:
                # Both emulators gzip; decompress a block to tell them apart.
                import gzip

                try:
                    with gzip.open(path, "rb") as gz:
                        prefix = gz.read(4096)
                except OSError:
                    return None
                if prefix.startswith(MEDNAFEN_MAGIC):
                    return "mednafen"
                if b"PCSX" in prefix[:64]:
                    return "pcsx-redux"
                return None
            # Bare (ungzipped) PCSX-Redux protobuf, as written by a Lua probe:
            # a length-delimited field 1 (`0x0A`) whose payload names the core.
            if head[:1] == b"\x0a":
                fh.seek(0)
                if b"PCSX" in fh.read(32):
                    return "pcsx-redux"
    except OSError:
        return None
    return None


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
            kind = classify(path)
            if kind is None:
                continue
            seen.add(resolved)
            if kind == "card":
                memcards += 1
                continue
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
