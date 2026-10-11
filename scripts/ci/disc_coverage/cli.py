"""Command line: measure, write the report and worklist, then ratchet.
"""

from __future__ import annotations

import argparse
import json
import os
import sys

from disc_coverage.attribution import read_attribution
from disc_coverage.dumps import dump_names_by_extent, read_dump_extents
from disc_coverage.images import data_report, overlay_reports, scus_report
from disc_coverage.paths import (
    ATTRIBUTION,
    BASELINE,
    DEFAULT_EXTRACTED,
    DEFAULT_FUNCS,
    DEFAULT_OUT,
    REPO,
)
from disc_coverage.ratchet import attribution_lag, snapshot
from disc_coverage.render import render
from disc_coverage.thresholds import LAG_EXTENTS_SHOWN
from disc_coverage.worklist import emit_worklist

# `--help` prints the package docstring.
from disc_coverage import __doc__


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--funcs", default=DEFAULT_FUNCS)
    ap.add_argument("--extracted", default=DEFAULT_EXTRACTED)
    ap.add_argument("--attribution", default=ATTRIBUTION,
                    help="byte-level extent attribution CSV; absent = attribute "
                         "overlay extents by address alone")
    ap.add_argument("--out", default=DEFAULT_OUT)
    ap.add_argument("--md", action="store_true",
                    help="write the markdown report to stdout as well")
    ap.add_argument("--check", action="store_true",
                    help="fail if any figure regressed below the committed baseline")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--tolerance", type=float, default=0.5,
                    help="percentage points a figure may drop before --check fails")
    ap.add_argument("--quiet", action="store_true",
                    help="drop the per-image rollup and print only the ratchet "
                         "verdict (for the pre-commit hook)")
    args = ap.parse_args()

    # Disc-gated, exactly like the LEGAIA_DISC_BIN tests: both inputs are
    # gitignored, so a checkout without disc data must pass rather than fail.
    if not os.path.isdir(args.funcs) or not os.path.isdir(args.extracted):
        print("[disc-coverage] SKIPPED - no dump corpus and/or no extracted/ tree.")
        print("[disc-coverage] Both are gitignored; this gate only measures locally.")
        return 0

    extents, rejects = read_dump_extents(args.funcs)
    if not extents:
        print("[disc-coverage] SKIPPED - dump corpus present but empty.")
        return 0

    # Optional. Without the CSV every overlay extent stays ambiguous by address,
    # which is the pre-attribution behaviour and still an honest upper bound.
    attrib = read_attribution(args.attribution)

    scus = scus_report(args.extracted, extents)
    overlays, amb_totals, unambiguous = overlay_reports(
        args.extracted, extents, attrib)
    data = data_report(args.extracted)

    if scus is None and not overlays:
        print("[disc-coverage] SKIPPED - no extractable images found.")
        return 0

    report = render(scus, overlays, amb_totals, data, rejects, bool(attrib))
    os.makedirs(args.out, exist_ok=True)
    md_path = os.path.join(args.out, "disc-coverage.md")
    with open(md_path, "w") as fh:
        fh.write(report)
    work_csv, work_md = emit_worklist(
        args.out, ([scus] if scus else []) + overlays)
    if args.md:
        sys.stdout.write(report)

    # Informational rollup. --quiet drops it so the hook shows only the
    # ratchet verdict - including the NOT MEASURED / NOT RATCHETED lines,
    # which are the ones a redirect used to swallow.
    if not args.quiet:
        if scus:
            print("[disc-coverage] SCUS_942.54 code: %.1f%% (%d/%d bytes)" % (
                scus["pct"], scus["covered"], scus["code_denominator"]))
        for r in overlays:
            amb = r.get("ambiguous_bytes_pct", 0.0)
            if amb >= 50.0:
                print("[disc-coverage] overlay %-22s >= %5.1f%%, no upper bound "
                      "(%.1f%% of its covered bytes are VA-ambiguous)"
                      % (r["name"], r["pct_floor"], amb))
            else:
                print("[disc-coverage] overlay %-22s %5.1f%%%s" % (
                    r["name"], r["pct"],
                    "" if amb == 0 else " (<=, >= %.1f%%, %.1f%% VA-ambiguous)"
                    % (r["pct_floor"], amb)))
        if data:
            print("[disc-coverage] PROT data parsed to a named format: %.1f%% "
                  "(unexplained %.1f%%; %.1f%% of content, filler excluded)"
                  % (data["pct_parsed"], data["pct_unexplained"],
                     data["pct_content_parsed"]))
        print("[disc-coverage] wrote %s" % md_path)
        print("[disc-coverage] wrote %s" % work_md)
        print("[disc-coverage] wrote %s" % work_csv)

    current = snapshot(scus, overlays, data)
    if args.update_baseline:
        with open(BASELINE, "w") as fh:
            json.dump(current, fh, indent=2, sort_keys=True)
            fh.write("\n")
        print("[disc-coverage] baseline updated: %s" % BASELINE)
        return 0

    if args.check:
        if not os.path.exists(BASELINE):
            # Not a pass. The ratchet has no reference, so it measured nothing
            # this run, and a bare "OK" here would be the exact shape this file
            # exists to prevent: a green line standing in for an absent
            # comparison.
            print("[disc-coverage] NOT RATCHETED - no baseline at %s. Nothing "
                  "was compared. Run --update-baseline once." % BASELINE)
            return 0
        base = json.load(open(BASELINE))
        bad = []
        absent = []
        lagged = []
        by_name = {r["name"]: r for r in overlays}
        for section in ("code", "code_floor", "data"):
            for key, was in base.get(section, {}).items():
                now = current.get(section, {}).get(key)
                if now is None:
                    # A missing image is a local extraction gap, not a
                    # regression - but it is also not a pass for that key, and
                    # an image dropping out of the corpus entirely would
                    # otherwise read identically to a clean run. Name it.
                    absent.append("%s/%s (baselined at %.2f%%)"
                                  % (section, key, was))
                    continue
                if now >= was - args.tolerance:
                    continue
                # A floor drop is the one figure a *new dump* can cause, so it
                # is triaged before it is failed: if the drop disappears once
                # the image's unattributed extents are removed, the corpus grew
                # and the attribution CSV did not.
                lag = (attribution_lag(by_name[key], extents, attrib,
                                       unambiguous, was, args.tolerance)
                       if section == "code_floor" and key in by_name else None)
                if lag:
                    lagged.append((key, was, now, lag))
                else:
                    bad.append("%s/%s: %.2f%% -> %.2f%%" % (section, key, was, now))
        for a in absent:
            print("[disc-coverage] NOT MEASURED THIS RUN: %s - the image is "
                  "absent from this tree, so the ratchet skipped it" % a)
        if lagged:
            names = dump_names_by_extent(args.funcs)
            todo = sorted({k for _, _, _, l in lagged for k in l["extents"]},
                          key=lambda k: k[0] - k[1])
            print("[disc-coverage] ATTRIBUTION LAG - not a coverage loss. %d "
                  "distinct dumped extent(s) have no row in %s, so they raise "
                  "an image's denominator without raising its floor:"
                  % (len(todo), os.path.relpath(args.attribution, REPO)))
            for key, was, now, lag in lagged:
                print("   %-22s floor %.2f%% -> %.2f%%, but %.2f%% over the "
                      "corpus the CSV knows (%d extent(s), %d B)"
                      % (key, was, now, lag["pct_floor_without"],
                         len(lag["extents"]), lag["bytes"]))
            print("[disc-coverage] extents to attribute, largest first "
                  "(VA-aliased ones count once here and land in several "
                  "images' spans above):")
            for a, b in todo[:LAG_EXTENTS_SHOWN]:
                stems = names.get((a, b)) or ["(no dump name)"]
                tail = (" +%d more" % (len(stems) - 3)) if len(stems) > 3 else ""
                print("   0x%08x..0x%08x %6d B  %s"
                      % (a, b, b - a, ", ".join(stems[:3]) + tail))
            if len(todo) > LAG_EXTENTS_SHOWN:
                print("   ... and %d more" % (len(todo) - LAG_EXTENTS_SHOWN))
            print("[disc-coverage] re-run "
                  "scripts/ghidra-analysis/attribute-dump-extents.py and commit "
                  "the CSV; the baseline needs no change.")
        if bad:
            print("[disc-coverage] REGRESSION:")
            for b in bad:
                print("   " + b)
            print("[disc-coverage] coverage may only go up. If a dump was "
                  "legitimately removed, re-run with --update-baseline and say "
                  "why in the commit message.")
            return 1
        total_keys = sum(len(base.get(s, {}))
                         for s in ("code", "code_floor", "data"))
        print("[disc-coverage] OK - %d/%d baselined figure(s) compared, none "
              "regressed beyond %.2f pp.%s"
              % (total_keys - len(absent), total_keys, args.tolerance,
                 " %d floor figure(s) moved only with the attribution lag "
                 "above." % len(lagged) if lagged else ""))
    return 0
