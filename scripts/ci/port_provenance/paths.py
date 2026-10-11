"""Where the sweep reads: crates, the dump corpus, the extracted disc, the
waiver file and the catalog's `live` column.
"""

from __future__ import annotations

from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent.parent
CRATES_DIR = REPO / "crates"
WAIVERS = Path(__file__).resolve().parent.parent / "port-provenance-waivers.toml"
LIVE_CSV = REPO / "target" / "port-catalog" / "catalog.csv"
OVERLAY_MAP = CRATES_DIR / "asset" / "data" / "static-overlays.toml"

# Both corpora are gitignored, so they are absent in a git worktree and in CI.
# `--funcs` / `--extracted` rebind them to another checkout's copies; main()
# assigns these before anything reads them.
DEFAULT_FUNCS = REPO / "ghidra" / "scripts" / "funcs"
DEFAULT_EXTRACTED = REPO / "extracted"
FUNCS_DIR = DEFAULT_FUNCS
EXTRACTED = DEFAULT_EXTRACTED
