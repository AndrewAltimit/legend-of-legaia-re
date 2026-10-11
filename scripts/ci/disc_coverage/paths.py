"""Where the gate reads and writes: the dump corpus, the extracted disc, the
overlay map, the attribution table, the baseline and the output directory.
"""

from __future__ import annotations

import os

# scripts/ci/disc_coverage/paths.py -> repo root is four levels up.
REPO = os.path.dirname(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))


DEFAULT_FUNCS = os.path.join(REPO, "ghidra", "scripts", "funcs")
DEFAULT_EXTRACTED = os.path.join(REPO, "extracted")
DEFAULT_OUT = os.path.join(REPO, "target", "disc-coverage")
OVERLAY_MAP = os.path.join(REPO, "crates", "asset", "data", "static-overlays.toml")
BASELINE = os.path.join(REPO, "scripts", "ci", "disc-coverage-baseline.json")
ATTRIBUTION = os.path.join(
    REPO, "scripts", "ghidra-analysis", "dump-extent-attribution.csv")
