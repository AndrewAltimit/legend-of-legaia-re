#!/usr/bin/env python3
"""display-list.py - read a frame's libgpu ordering table from either emulator.

`mednafen-state display-list` does the decoding, but it can only open a mednafen
state or a raw main-RAM dump: `legaia-pcsxr` depends on `legaia-mednafen`, so the
mednafen crate cannot read a PCSX-Redux `.sstate` without a dependency cycle.
This wrapper closes that gap the same way `widget-draw-sweep.py` does - dispatch
on the file, unpack a `.sstate` through `pcsxr-state extract`, and hand the raw
main-RAM image to the decoder.

Usage:

    scripts/mednafen/display-list.py <state.mc0|state.sstate|ram.bin> [-- ...]

Everything after the state path is forwarded to `mednafen-state display-list`,
so `--coincident`, `--list`, `--ot-addr`, `--min-area` and friends work
unchanged:

    scripts/mednafen/display-list.py foo.sstate --coincident
    scripts/mednafen/display-list.py foo.sstate --list --top 40

What the output is for: a RAM image **is** the frame's display list, so this
answers "does retail actually draw this?" with no emulator run. Two cautions the
tool prints but that are worth knowing before reading a report:

- The packet **pool** holds stale packets from earlier frames. Only the chain
  reachable from an ordering table is the live frame - a pool census will
  over-count. The tool walks the OT for exactly this reason.
- Retail double-buffers, so the ordering tables come in pairs holding frame N
  and frame N-1. The tool walks one by default; `--all-ots` merges them and
  makes every surface appear twice.
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]


def tool(name: str) -> Path:
    binary = REPO / "target" / "release" / name
    if binary.is_file():
        return binary
    found = shutil.which(name)
    if not found:
        raise SystemExit(
            f"{name} not found - build it with:\n"
            f"    cargo build --release -p legaia-mednafen -p legaia-pcsxr"
        )
    return Path(found)


def main() -> int:
    ap = argparse.ArgumentParser(add_help=True, description=__doc__.split("\n")[0])
    ap.add_argument("state", type=Path, help="mednafen .mc{0..9}, PCSX-Redux .sstate, or raw .bin")
    args, passthrough = ap.parse_known_args()

    if not args.state.is_file():
        raise SystemExit(f"no such file: {args.state}")

    decoder = tool("mednafen-state")

    # A PCSX-Redux state has to be unpacked first; everything else the decoder
    # opens directly.
    if ".sstate" in args.state.name:
        with tempfile.TemporaryDirectory() as tmp:
            ram = Path(tmp) / "ram.bin"
            subprocess.run(
                [
                    str(tool("pcsxr-state")), "extract", str(args.state),
                    "--start", "0x80000000", "--end", "0x80200000", "--out", str(ram),
                ],
                check=True,
            )
            print(f"[display-list.py] unpacked {args.state.name} -> raw main RAM")
            return subprocess.run(
                [str(decoder), "display-list", str(ram), *passthrough]
            ).returncode

    return subprocess.run(
        [str(decoder), "display-list", str(args.state), *passthrough]
    ).returncode


if __name__ == "__main__":
    sys.exit(main())
