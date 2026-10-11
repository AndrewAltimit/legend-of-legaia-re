"""Which coverage exports are on disk, and which canonical ladders are missing.
"""

from __future__ import annotations

from pathlib import Path

from replay_port_coverage.ladders import CANONICAL_LADDER_NAMES
from replay_port_coverage.paths import COV_DIR, COV_GLOB, LEGACY_JSON


def discover_inputs() -> tuple[list[Path], list[str]]:
    """`(paths, missing_canonical_labels)` for a bare invocation.

    Every `target/cov-*.json` is joined, not just the canonical set, so a
    ladder added later is picked up without editing this file. The legacy
    single-file path is honoured only when the glob finds nothing at all.
    """
    found = sorted(COV_DIR.glob(COV_GLOB))
    if not found and LEGACY_JSON.is_file():
        # The pre-per-ladder path: one merged export whose contents cannot be
        # attributed to a ladder. Every canonical name is reported unjoined,
        # which is the honest reading - the per-ladder table cannot be built
        # from it, so nothing here can say which ladders it covers.
        return [LEGACY_JSON], list(CANONICAL_LADDER_NAMES)
    labels = {p.stem.removeprefix("cov-") for p in found}
    missing = [name for name in CANONICAL_LADDER_NAMES if name not in labels]
    return found, missing
