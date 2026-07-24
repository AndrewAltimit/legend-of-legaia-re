#!/usr/bin/env python3
"""Refresh the committed progress metrics the site landing page renders.

The site is built by `site/_gen.py` on a machine that has **no disc data** -
`extracted/` and the Ghidra dump corpus are both gitignored. So the landing
page cannot compute its own numbers at deploy time. This script is the local
refresh step: run it on a machine that has the disc, commit the resulting JSON,
and the deployment pipeline just renders what is committed.

Sources, and what each denominator actually is - the labels matter more than the
numbers, because the two families are not comparable:

  DISC-DENOMINATED (`scripts/ci/disc-coverage.py`)
    Measured against the game's own bytes. These can fall as well as rise and
    are the only figures that can say how much of the game is left.

  CORPUS-DENOMINATED (`scripts/ci/port-catalog.py`)
    Measured against the set of addresses this project has identified. Useful
    for steering work; structurally unable to see a subsystem nobody has cited.
    Never present one of these as "percent of the game".

Usage:
    python3 scripts/ci/update-progress-metrics.py          # refresh + write
    python3 scripts/ci/update-progress-metrics.py --print  # show, don't write
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(REPO, "scripts", "ci", "progress-metrics.json")
DISC_COVERAGE = os.path.join(REPO, "scripts", "ci", "disc-coverage.py")
PORT_CATALOG = os.path.join(REPO, "scripts", "ci", "port-catalog.py")


def load_disc_coverage():
    spec = importlib.util.spec_from_file_location("disc_coverage", DISC_COVERAGE)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    extents, _unparsed = mod.read_dump_extents(mod.DEFAULT_FUNCS)
    if not extents:
        return None, None
    scus = mod.scus_report(mod.DEFAULT_EXTRACTED, extents)
    data = mod.data_report(mod.DEFAULT_EXTRACTED)
    return scus, data


def run_port_catalog():
    """Parse the catalog's own summary block rather than re-deriving it."""
    try:
        proc = subprocess.run(
            [sys.executable, PORT_CATALOG, "--live-audit"],
            cwd=REPO, capture_output=True, text=True, timeout=3600)
    except (OSError, subprocess.TimeoutExpired):
        return None
    text = proc.stdout + proc.stderr

    def grab(pattern):
        m = re.search(pattern, text)
        return int(m.group(1)) if m else None

    return {
        "ported": grab(r"ported \(// PORT: tag\)\s*:\s*(\d+)"),
        "worklist": grab(r"remaining port worklist\s*:\s*(\d+)"),
        "live": grab(r"ported \+ live.*?:\s*(\d+)"),
        "inert": grab(r"ported, NOT live \(inert\)\s*:\s*(\d+)"),
        "documented_gap": grab(r"ported but NOT documented \(provenance gap\)\s*:\s*(\d+)"),
        "dump_worklist": grab(r"cited but NOT dumped\s+\(dump worklist\)\s*:\s*(\d+)"),
    }


def build(scus, data, cat):
    tracks = []

    if scus:
        tracks.append({
            "key": "decompilation",
            "label": "Decompilation",
            "pct": round(scus["pct"], 1),
            "headline": "%.1f%% of SCUS_942.54's code" % scus["pct"],
            "detail": "Share of the main executable's code bytes that sit inside a "
                      "Ghidra-dumped function. Overlay images are excluded - they "
                      "alias in address space, so address attribution alone cannot "
                      "support a figure.",
            "denominator": "disc bytes",
            "href": "tooling/disc-coverage.html",
        })

    if data:
        tracks.append({
            "key": "formats",
            "label": "Asset formats",
            "pct": round(data["pct_parsed"], 1),
            "headline": "%.1f%% of PROT.DAT" % data["pct_parsed"],
            "detail": "Share of disc asset bytes whose container resolves to a "
                      "documented format. This is format *recognition*, not a "
                      "byte-for-byte parse, so read it as an upper bound. "
                      "%.1f%% remains unexplained." % data["pct_unexplained"],
            "denominator": "disc bytes",
            "href": "formats/index.html",
        })

    if cat and cat.get("ported") is not None:
        ported, worklist = cat["ported"], cat.get("worklist") or 0
        identified = ported + worklist
        tracks.append({
            "key": "port",
            "label": "Engine port",
            "pct": round(100.0 * ported / identified, 1) if identified else 0.0,
            "headline": "%d functions ported" % ported,
            "detail": "Of the retail functions identified as port sites, the share "
                      "carrying a clean-room Rust implementation. %d remain on the "
                      "worklist. Measured against what the project has identified, "
                      "not against the whole game." % worklist,
            "denominator": "identified port sites",
            "href": "subsystems/engine.html",
        })

        live, inert = cat.get("live"), cat.get("inert")
        if live is not None and inert is not None and (live + inert):
            tracks.append({
                "key": "wiring",
                "label": "Port wiring",
                "pct": round(100.0 * live / (live + inert), 1),
                "headline": "%d of %d ported functions reachable" % (live, live + inert),
                "detail": "A ported function still needs a host that calls it. This "
                          "is the share reachable from a real entry point; the "
                          "remaining %d are implemented but not yet hosted, and each "
                          "one says so in its source." % inert,
                "denominator": "ported functions",
                "href": "subsystems/engine.html",
            })

    return {
        "_comment": "Committed build input for site/_gen.py. The site builds without "
                    "disc data, so these cannot be computed at deploy time. Refresh "
                    "locally with scripts/ci/update-progress-metrics.py and commit.",
        "tracks": tracks,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--print", dest="show", action="store_true")
    ap.add_argument("--skip-catalog", action="store_true",
                    help="skip the slow port-catalog pass and keep its committed tracks")
    args = ap.parse_args()

    scus, data = load_disc_coverage()
    if scus is None:
        print("[progress] SKIPPED - no dump corpus / extracted tree; nothing to refresh.")
        return 0

    cat = None if args.skip_catalog else run_port_catalog()
    if args.skip_catalog and os.path.exists(OUT):
        prev = json.load(open(OUT))
        keep = {t["key"]: t for t in prev.get("tracks", [])}
        out = build(scus, data, None)
        have = {t["key"] for t in out["tracks"]}
        for key in ("port", "wiring"):
            if key in keep and key not in have:
                out["tracks"].append(keep[key])
    else:
        out = build(scus, data, cat)

    order = {"decompilation": 0, "formats": 1, "port": 2, "wiring": 3}
    out["tracks"].sort(key=lambda t: order.get(t["key"], 99))

    text = json.dumps(out, indent=2) + "\n"
    if args.show:
        sys.stdout.write(text)
        return 0
    with open(OUT, "w") as fh:
        fh.write(text)
    print("[progress] wrote %s" % OUT)
    for t in out["tracks"]:
        print("  %-14s %5.1f%%  (%s)" % (t["label"], t["pct"], t["denominator"]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
