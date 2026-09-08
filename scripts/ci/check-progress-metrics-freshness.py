#!/usr/bin/env python3
"""Is the committed `progress-metrics.json` still describing this tree?

`scripts/ci/progress-metrics.json` is a **build input**, not a measurement: the
site builds on a machine with no disc data, so the landing-page tiles render
whatever was last committed. Nothing compared it to anything, and a stale file
is invisible - it is well-formed JSON with plausible numbers, and the site
renders it happily. The failure this exists to end is exactly that shape: the
tiles rendered `840 ported / 0 on the worklist / 82.4% wired` for a week while
the tree said 847 and 93.

## What it compares, and why not to the live catalog

The obvious check - run `port-catalog.py` and diff - costs a full catalog pass,
and the pre-commit hook already spends one. So the cheap check is against the
**committed ratchet baseline** `scripts/ci/port-catalog-baseline.json`, which
the hook's own `port-catalog.py --check` keeps honest on every commit that
touches `crates/`. Both files are committed, so this needs no dump corpus, no
`extracted/`, and no disc - it runs the same everywhere, which is what makes it
usable as a hook.

`--live` adds the expensive comparison for a closeout run, where the extra
pass is affordable and the corpus is present.

The wiring track (`live` / `owed`) has no baselined counterpart, so it is
checked only under `--live`. Its denominator is `live + inert - replaced`
(see `update-progress-metrics.py`); a `REPLACED-BY:` port is in neither half.

## What it deliberately does not do

It never fails a commit, and it never rewrites the JSON. Refreshing is a
disc-machine operation whose output is committed deliberately
(`update-progress-metrics.py`), and a hook that silently rewrote a build input
would put an unreviewed number on the public site.

    python3 scripts/ci/check-progress-metrics-freshness.py
    python3 scripts/ci/check-progress-metrics-freshness.py --live
    python3 scripts/ci/check-progress-metrics-freshness.py --strict
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
METRICS = os.path.join(REPO, "scripts", "ci", "progress-metrics.json")
CATALOG_BASELINE = os.path.join(REPO, "scripts", "ci", "port-catalog-baseline.json")
PORT_CATALOG = os.path.join(REPO, "scripts", "ci", "port-catalog.py")
FUNCS = os.path.join(REPO, "ghidra", "scripts", "funcs")


def tracks(doc):
    return {t.get("key"): t for t in doc.get("tracks", [])}


def port_track_counts(track):
    """`(ported, worklist)` as the committed track states them.

    Read off the rendered strings rather than off a numeric field, because the
    strings are what the site shows. A track whose headline and detail disagree
    with its own `pct` would still render the headline.
    """
    m = re.search(r"(\d+)\s+functions ported", track.get("headline", ""))
    ported = int(m.group(1)) if m else None
    m = re.search(r"(\d+)\s+remain on the worklist", track.get("detail", ""))
    worklist = int(m.group(1)) if m else None
    return ported, worklist


def wiring_track_counts(track):
    m = re.search(r"(\d+)\s+of\s+(\d+)\s+ported functions reachable",
                  track.get("headline", ""))
    if not m:
        return None, None
    return int(m.group(1)), int(m.group(2))


def live_catalog():
    """The live figures, at the cost of a full catalog pass."""
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
        "replaced": grab(r"of which infra-replaced.*?:\s*(\d+)"),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--live", action="store_true",
                    help="also run port-catalog.py (slow) and compare to it")
    ap.add_argument("--strict", action="store_true",
                    help="exit 1 on a mismatch instead of warning")
    args = ap.parse_args()

    if not os.path.exists(METRICS) or not os.path.exists(CATALOG_BASELINE):
        print("[progress-freshness] SKIPPED - progress-metrics.json and/or "
              "port-catalog-baseline.json missing; nothing to compare.")
        return 0

    metrics = json.load(open(METRICS))
    baseline = json.load(open(CATALOG_BASELINE))
    tr = tracks(metrics)
    port = tr.get("port")
    if port is None:
        print("[progress-freshness] progress-metrics.json carries no `port` "
              "track - the landing page renders nothing for it. Run "
              "scripts/ci/update-progress-metrics.py on a disc machine.")
        return 1 if args.strict else 0

    shown_ported, shown_worklist = port_track_counts(port)
    base_ported = baseline.get("totals", {}).get("ported")
    base_worklist = baseline.get("worklist", {}).get("port")

    bad = []
    if shown_ported is not None and base_ported is not None \
            and shown_ported != base_ported:
        bad.append("ported: tiles say %d, port-catalog-baseline.json says %d"
                   % (shown_ported, base_ported))
    if shown_worklist is not None and base_worklist is not None \
            and shown_worklist != base_worklist:
        bad.append("port worklist: tiles say %d, port-catalog-baseline.json "
                   "says %d" % (shown_worklist, base_worklist))

    if args.live:
        if not os.path.isdir(FUNCS):
            print("[progress-freshness] --live needs ghidra/scripts/funcs/; "
                  "the corpus is gitignored and absent here, so only the "
                  "committed-file comparison ran.")
        else:
            cat = live_catalog()
            if cat is None:
                print("[progress-freshness] --live: port-catalog.py did not "
                      "complete; only the committed-file comparison ran.")
            else:
                if cat["ported"] is not None and shown_ported != cat["ported"]:
                    bad.append("ported: tiles say %d, this tree has %d"
                               % (shown_ported, cat["ported"]))
                if cat["worklist"] is not None and shown_worklist != cat["worklist"]:
                    bad.append("port worklist: tiles say %d, this tree has %d"
                               % (shown_worklist, cat["worklist"]))
                wiring = tr.get("wiring")
                live, denom = wiring_track_counts(wiring or {})
                owed = ((cat["inert"] or 0) - (cat["replaced"] or 0)
                        if cat["inert"] is not None else None)
                if live is not None and cat["live"] is not None and owed is not None:
                    want = cat["live"] + owed
                    if live != cat["live"] or denom != want:
                        bad.append("wiring: tiles say %d of %d, this tree has "
                                   "%d of %d" % (live, denom, cat["live"], want))

    if not bad:
        print("[progress-freshness] OK - the landing-page tiles agree with "
              "%s%s." % (os.path.relpath(CATALOG_BASELINE, REPO),
                         " and with this tree's catalog" if args.live else ""))
        return 0

    print("[progress-freshness] STALE - scripts/ci/progress-metrics.json is a "
          "committed build input and it no longer matches this tree:")
    for line in bad:
        print("   " + line)
    print("[progress-freshness] refresh it on a machine with the disc: "
          "python3 scripts/ci/update-progress-metrics.py, then commit the "
          "JSON. See docs/tooling/disc-coverage.md#refreshing-the-landing-page-tiles.")
    return 1 if args.strict else 0


if __name__ == "__main__":
    sys.exit(main())
