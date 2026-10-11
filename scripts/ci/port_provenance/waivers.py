"""The waiver file and the catalog's set of live ports.
"""

from __future__ import annotations

import csv

try:  # py311+
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - py<3.11
    tomllib = None  # type: ignore[assignment]

from port_provenance.paths import LIVE_CSV, WAIVERS

# ---------------------------------------------------------------------------
# Waivers + live set
# ---------------------------------------------------------------------------


def load_waivers() -> dict[str, str]:
    if tomllib is None or not WAIVERS.exists():
        return {}
    with WAIVERS.open("rb") as fh:
        doc = tomllib.load(fh)
    out: dict[str, str] = {}
    for entry in doc.get("waiver", []):
        key = entry.get("key")
        if key:
            out[key] = entry.get("reason", "")
    return out


def load_live() -> set[str]:
    """Addresses port-catalog.py last reported as reachable from a host root."""
    if not LIVE_CSV.exists():
        return set()
    out: set[str] = set()
    with LIVE_CSV.open() as fh:
        for row in csv.DictReader(fh):
            if row.get("ported") == "1" and row.get("live") == "1":
                out.add(row["addr"].lower())
    return out
