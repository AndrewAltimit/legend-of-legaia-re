"""Where the report reads coverage exports and writes its page.
"""

from __future__ import annotations

from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent.parent
CATALOG = REPO / "scripts" / "ci" / "port-catalog.py"
COV_DIR = REPO / "target"
COV_GLOB = "cov-*.json"
LEGACY_JSON = COV_DIR / "replay-cov.json"
DEFAULT_OUT = REPO / "target" / "port-catalog" / "replay-port-entry.md"
