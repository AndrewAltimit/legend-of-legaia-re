"""`--check` / `--update-baseline`: the worklist and ported counts against
`scripts/ci/port-catalog-baseline.json`.
"""

import json

from port_catalog.paths import BASELINE

# --- Ratchet -----------------------------------------------------------------
#
# The catalog drove the site's landing-page numbers while being unable to fail:
# no --check, no --strict, no baseline, and the only non-zero exit in main() was
# an unknown --feature name. A measurement that cannot report a regression is a
# dashboard, and every wave that widened a worklist did so silently.
#
# What may be ratcheted is constrained by this file's design. It builds TWO call
# graphs; the receiver-gated one exists solely for the stale-`NOT WIRED` test
# (see build_rust_graph), and sharpening the shared permissive graph has been
# tried and reverted twice. So the ratchet keys on quantities that are
# properties of the tags, the docs and the dump corpus - not of graph
# resolution. `n_stale`, the one figure read off the strict graph, is
# deliberately absent.
#
# `disclosure_gap` is included despite coming from the permissive graph, because
# the permissive graph OVER-reports reachability: a port it calls inert really is
# inert, so an inert anchor with no `NOT WIRED:` tag is a lower bound on the real
# disclosure gap. It only appears in a snapshot taken with --live, and is only
# compared when both sides have it.
RATCHET_MAX = [  # may fall freely, may not rise
    ("worklist", "port"),
    ("worklist", "dump"),
    ("worklist", "ported_not_documented"),
    ("worklist", "ported_not_dumped"),
    ("live", "disclosure_gap"),
]
RATCHET_MIN = [("totals", "ported")]  # may rise freely, may not fall


def snapshot(rows: list[dict]) -> dict:
    """The ratchetable figures, in the same shape the baseline file carries."""
    dd_not_p = [r for r in rows if r["dumped"] and r["documented"] and not r["ported"]]
    out: dict = {
        "totals": {"ported": sum(1 for r in rows if r["ported"])},
        "worklist": {
            "port": sum(1 for r in dd_not_p if not r["ignored"]),
            "dump": sum(
                1
                for r in rows
                if r["refs"] > 0 and not r["dumped"] and not r["ignored"]
            ),
            "ported_not_documented": sum(
                1 for r in rows if r["ported"] and not r["documented"]
            ),
            "ported_not_dumped": sum(
                1 for r in rows if r["ported"] and not r["dumped"]
            ),
        },
    }
    if any(r["live_known"] for r in rows):
        pairs = [a for r in rows if r["ported"] for a in r["anchors"]]
        out["live"] = {
            "disclosure_gap": sum(
                1
                for a in pairs
                if not a["live"] and not a["not_wired_tag"] and not a["replaced_tag"]
            )
        }
    return out


def run_ratchet(rows: list[dict], args) -> int:
    """`--check` / `--update-baseline`. Returns a process exit status."""
    current = snapshot(rows)

    if args.update_baseline:
        # A snapshot only carries what this run computed, so writing it verbatim
        # DELETES every key the run could not produce. Without `--live` that is
        # the whole `live` block, and one `--update-baseline` would have erased
        # a ratchet nobody would then notice was gone - the same silence this
        # ratchet exists to end.
        old = json.loads(BASELINE.read_text()) if BASELINE.exists() else {}
        carried: list[str] = []
        for section, values in old.items():
            for key, was in values.items():
                if key not in current.get(section, {}):
                    current.setdefault(section, {})[key] = was
                    carried.append(f"{section}/{key}={was}")
        BASELINE.write_text(json.dumps(current, indent=2, sort_keys=True) + "\n")
        print(f"[port-catalog] baseline updated: {BASELINE}")
        if carried:
            print(
                "[port-catalog] carried forward (not computed this run): "
                + ", ".join(sorted(carried))
                + " - re-run with --live to refresh them."
            )
        return 0

    if not BASELINE.exists():
        # Not a pass. Nothing was compared, and a bare OK here would be the
        # same failure the rest of this change is about.
        print(
            f"[port-catalog] NOT RATCHETED - no baseline at {BASELINE}. Nothing "
            f"was compared. Run --update-baseline once."
        )
        return 0

    base = json.loads(BASELINE.read_text())
    bad: list[str] = []
    skipped: list[str] = []
    for section, key in RATCHET_MAX + RATCHET_MIN:
        was = base.get(section, {}).get(key)
        now = current.get(section, {}).get(key)
        if was is None:
            continue
        if now is None:
            # Named, never silent: `live.disclosure_gap` is absent from a run
            # without --live, and "the slow pass was not run" must not read the
            # same as "the slow pass found nothing".
            skipped.append(f"{section}/{key} (baselined at {was})")
            continue
        rising = (section, key) in RATCHET_MAX
        if (now > was) if rising else (now < was):
            arrow = "grew" if rising else "shrank"
            bad.append(f"{section}/{key}: {was} -> {now} ({arrow})")
    for s in skipped:
        print(f"[port-catalog] NOT COMPARED THIS RUN: {s} - re-run with --live")
    if skipped and not args.allow_uncompared:
        # A baselined figure the default path never computes is not a ratchet.
        # `live/disclosure_gap` was baselined at 0 and compared by nothing that
        # ran automatically, so it drifted to 1 and stayed there until somebody
        # ran `--live` by hand. Printing a line and exiting 0 is what let that
        # happen, so the default is now non-zero and the caller that cannot
        # afford the slow pass has to say so at the call site.
        print(
            "[port-catalog] NOT RATCHETED: "
            f"{len(skipped)} baselined figure(s) went uncompared. Run "
            "`--check --live` (about 20s). The pre-commit hook spends that only "
            "when the commit stages crates/, and passes --allow-uncompared "
            "otherwise - nothing else may."
        )
        return 1
    if bad:
        print("[port-catalog] REGRESSION:")
        for b in bad:
            print(f"   {b}")
        print(
            "[port-catalog] a worklist may shrink and the ported count may grow, "
            "not the reverse. If the move is intended - a newly dumped subsystem "
            "widening the worklist, say - re-run with --update-baseline and say "
            "why in the commit message."
        )
        print(
            "[port-catalog] validate any surprise against the per-row pages "
            "(--missing-ports, --live-audit), never against the count alone."
        )
        return 1
    print(
        f"[port-catalog] OK - {len(RATCHET_MAX) + len(RATCHET_MIN) - len(skipped)}"
        f"/{len(RATCHET_MAX) + len(RATCHET_MIN)} figure(s) compared, none regressed."
    )
    return 0
