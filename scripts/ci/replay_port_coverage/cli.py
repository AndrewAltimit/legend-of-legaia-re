"""Command line and report: join the catalog to the coverage, write the page.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from replay_port_coverage.catalog import (
    catalog_address_sets,
    disclosed_dead_addresses,
    load_catalog_module,
)
from replay_port_coverage.coverage import (
    FileCoverage,
    join_anchor_coverage,
    load_coverage,
)
from replay_port_coverage.inputs import discover_inputs
from replay_port_coverage.ladder_check import check_ladders
from replay_port_coverage.ladders import CANONICAL_LADDERS, CANONICAL_LADDER_NAMES
from replay_port_coverage.page_audit import page_audit
from replay_port_coverage.paths import COV_GLOB, DEFAULT_OUT, REPO
from replay_port_coverage.selftest import run_selftest

# `--help` leads with the first line of the package docstring.
from replay_port_coverage import __doc__


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--json",
        type=Path,
        action="append",
        dest="jsons",
        metavar="PATH",
        help=(
            "llvm-cov export for one ladder; repeat to union several. "
            f"Default: every {COV_GLOB} under target/ (the union)"
        ),
    )
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT)
    ap.add_argument(
        "--fail-on-disclosed",
        action="store_true",
        help="exit non-zero if a NOT WIRED anchor was executed by the run",
    )
    ap.add_argument(
        "--list-ladders",
        action="store_true",
        help="print `<test> <package>` per canonical ladder and exit (the "
        "export recipe reads this, so it cannot drift from the list)",
    )
    ap.add_argument(
        "--page-audit",
        action="store_true",
        help="join docs/tooling/reach-triage.md's cited addresses against "
        "target/port-catalog/catalog.csv and report the rows that are no "
        "longer live (needs no coverage export)",
    )
    ap.add_argument(
        "--selftest",
        action="store_true",
        help="run the item-verdict resolver self-test on a synthetic corpus "
        "and exit (no coverage data or repo state touched)",
    )
    args = ap.parse_args()

    if args.selftest:
        return run_selftest()

    if args.page_audit:
        return page_audit()

    # Checked before the membership is printed or joined: the export recipe
    # reads `--list-ladders`, so a ladder that names a test which is not on
    # disk has to stop here rather than become a union with a silent hole.
    stale_ladders = check_ladders()
    if stale_ladders:
        print(
            f"replay-port-coverage: {len(stale_ladders)} row(s) of "
            "CANONICAL_LADDERS (scripts/ci/replay_port_coverage/ladders.py) "
            "name a test that is not on disk:",
            file=sys.stderr,
        )
        for line in stale_ladders:
            print(f"  - {line}", file=sys.stderr)
        return 1

    if args.list_ladders:
        for name, pkg in CANONICAL_LADDERS:
            print(name, pkg)
        return 0

    if args.jsons:
        inputs = args.jsons
        given = {p.stem.removeprefix("cov-") for p in inputs}
        missing_canonical = [n for n in CANONICAL_LADDER_NAMES if n not in given]
    else:
        inputs, missing_canonical = discover_inputs()
    # A missing input is a skip, not a failure - the whole invocation is
    # optional (see the module docstring). A *present* input with no function
    # regions is also a skip, because llvm-cov emits that shape for a build
    # that produced no instrumented objects.
    sources: list[tuple[str, dict[str, FileCoverage]]] = []
    for path in inputs:
        if not path.is_file():
            print(f"[skip] {path} missing - run cargo llvm-cov first")
            continue
        cov = load_coverage(path)
        if not cov:
            print(f"[skip] {path} carries no function regions")
            continue
        sources.append((path.stem, cov))
    if not sources:
        return 0

    catalog = load_catalog_module()
    srcs = catalog.load_rust_sources()
    anchors = catalog.collect_port_anchors(srcs)
    live, not_live = catalog_address_sets()

    joined = join_anchor_coverage(catalog, anchors, srcs, sources, live, not_live)
    inert_entered = joined["inert_entered"]
    disclosed_entered = joined["disclosed_entered"]
    live_entered = joined["live_entered"]
    # Split the never-entered set: rows whose own disclosure the strict graph
    # confirms are not reach work (see `disclosed_dead_addresses`).
    dead = disclosed_dead_addresses(catalog, srcs)
    live_unentered_all = joined["live_unentered"]
    live_unentered = [(a, x) for a, x in live_unentered_all if a.lower() not in dead]
    unentered_disclosed = [
        (a, {**x, "crate": f"{x['crate']} ({dead[a.lower()]})"})
        for a, x in live_unentered_all
        if a.lower() in dead
    ]
    # A disclosed-dead address with no record in any binary is the same
    # verdict from the same two instruments: the disclosure says no host is
    # owed, the strict graph reaches nothing, and a function nothing links is
    # exactly what leaves no record. Filing it as "unmeasured" would ask for a
    # ladder whose only way to enter the row is to call what the source
    # disclaims.
    unobservable_all = joined["unobservable"]
    unobservable = [(a, x) for a, x in unobservable_all if a.lower() not in dead]
    unentered_disclosed += [
        (a, {**x, "crate": f"{x['crate']} ({dead[a.lower()]}, no record)"})
        for a, x in unobservable_all
        if a.lower() in dead
    ]
    unentered_disclosed.sort(key=lambda r: r[0])
    not_observable_const = joined["not_observable_const"]
    # label -> the live addresses that source entered. Drives the per-source
    # contribution table: an address several ladders reach is credited to each,
    # and the `unique` column is what would be lost by dropping that ladder.
    per_source_live = joined["per_source_live"]
    not_live_observable = joined["not_live_observable"]

    lines: list[str] = []
    w = lines.append
    w("# Replay port-entry report")
    w("")
    w(
        "Runtime join: which `// PORT:`-tagged addresses a pad-driven ladder "
        "actually executed, against the static liveness verdict. See the "
        "script header for why the static number alone is an upper bound."
    )
    w("")
    w(f"- ported addresses with anchors: **{len(anchors)}**")
    w(f"- statically live: **{len(live)}**, of which entered by a run: **{len(live_entered)}**")
    w(
        f"- live, never entered: **{len(live_unentered) + len(unentered_disclosed)}** - "
        f"**{len(live_unentered)}** reach worklist, "
        f"**{len(unentered_disclosed)}** disclosed and receiver-gated dead"
    )
    w(f"- statically not-live: **{len(not_live)}**, of which entered anyway: **{len(inert_entered)}**")
    w(
        "- `NOT WIRED` / `REPLACED-BY`-disclosed anchors executed: "
        f"**{len(disclosed_entered)}**"
    )
    w(f"- not observable (const anchors, no executed reference): **{len(not_observable_const)}**")
    w(f"- not observable in any of these binaries (excluded above): **{len(unobservable)}**")
    w("")
    w(
        f"Non-vacuity: **{not_live_observable} of {len(not_live)}** not-live addresses "
        "have at least one anchor file present in the coverage data, so the "
        "inert-entered count is a measurement over that many addresses rather "
        "than a lookup miss."
    )
    w("")

    # Per-source contribution. `unique` is what dropping that ladder would
    # cost: a single-ladder join is not a smaller version of this measurement,
    # it is a different one, and the column says by how much.
    w("## Per-ladder contribution")
    w("")
    w(
        "Union across the sources below. These are separate sessions, not one "
        "continuous playthrough - what the union measures is \"some pad-driven "
        "ladder entered this\" against \"no pad-driven ladder did\". `unique` "
        "counts live addresses only that source entered."
    )
    w("")
    w("| source | live entered | unique to it |")
    w("|---|---|---|")
    for label, _ in sources:
        mine = per_source_live[label]
        others: set[str] = set()
        for other, _ in sources:
            if other != label:
                others |= per_source_live[other]
        w(f"| `{label}` | {len(mine)} | {len(mine - others)} |")
    w("")
    if missing_canonical:
        w(
            "**Partial union.** No per-ladder export was joined for "
            + ", ".join(f"`{n}`" for n in missing_canonical)
            + ". Every number above is about the ladders listed, not about the "
            "canonical set, and the live-unentered worklist below is "
            "correspondingly long. Re-export the missing ladder(s) before "
            "reading a row as work."
        )
        w("")

    def table(title: str, rows: list[tuple[str, dict]], note: str):
        w(f"## {title}")
        w("")
        w(note)
        w("")
        if not rows:
            w("_none_")
            w("")
            return
        w("| address | crate | symbol | site |")
        w("|---|---|---|---|")
        for addr, site in rows:
            w(
                f"| `{addr}` | {site['crate']} | `{site['symbol']}` | "
                f"{site['file']}:{site['line']} |"
            )
        w("")

    table(
        "Inert ports entered",
        inert_entered,
        "The static graph reports no host root reaches these, yet a ladder "
        "executed the anchor. Either the graph is wrong or the tag sits on the "
        "wrong symbol - each row is a finding, not a metric.",
    )
    table(
        "Disclosed `NOT WIRED` / `REPLACED-BY` anchors executed",
        disclosed_entered,
        "The source disclaims these as unreached, and a **passing** oracle ran "
        "them anyway. Highest-priority rows: an oracle that traverses "
        "undisclosed-stub code can certify behaviour nothing implements.",
    )
    table(
        "Live but never entered",
        live_unentered,
        "Statically reachable, reached by none of the ladders above. Not "
        "defects - this is the wiring worklist ordered by what a playthrough "
        "actually needs, and it shrinks as the ladders reach further. Item "
        "anchors (const / static / type alias) never appear here - an item has "
        "no lines to enter, so its rows live in the bucket below instead.",
    )
    table(
        "Live, never entered, and disclosed dead",
        unentered_disclosed,
        "Live only through the permissive graph: the receiver-gated graph "
        "reaches none of the address's anchors, and every anchor the "
        "permissive graph does reach carries a `NOT WIRED:` or `REPLACED-BY:` "
        "disclosure. The disclosure, the strict graph and the coverage all "
        "agree (a row marked *no record* has no function record in any "
        "binary, which is what an unlinked function leaves), so these are "
        "not reach work - they are counted in the "
        "never-entered total above and listed here so they leave the worklist "
        "without leaving the report. One that a ladder enters moves to the "
        "disclosed-executed bucket.",
    )
    table(
        "Not observable in any of these binaries",
        unobservable,
        "No coverage binary in the union carries the anchor's file at all, so "
        "the join has nothing to read for these and they are outside both the "
        "entered and the never-entered counts above. **They are not "
        "never-entered rows**, and the distinction is the whole reason this "
        "table exists: a reader joining a page's address list against the "
        "never-entered set alone reads an absent address as converted, which "
        "is the one direction that turns a missing measurement into a claim of "
        "progress. Converting a row here means running a ladder that links the "
        "crate, not wiring anything.",
    )
    table(
        "Not observable (const anchors)",
        not_observable_const,
        "Item anchors whose executed verdict is reference-based (see the "
        "script header) and where no attributable referencing function "
        "executed. Not \"never entered\" - there is nothing to enter - and "
        "not a worklist: converting one of these means executing a function "
        "that *references* the item, at which point the row moves on its own.",
    )

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("\n".join(lines) + "\n")

    print(f"sources               : {', '.join(label for label, _ in sources)}")
    print(f"ported anchors        : {len(anchors)}")
    print(f"live / entered        : {len(live)} / {len(live_entered)}")
    print(f"not-live / entered    : {len(not_live)} / {len(inert_entered)}")
    print(f"  (observable         : {not_live_observable} / {len(not_live)})")
    print(f"NOT WIRED executed    : {len(disclosed_entered)}")
    print(f"live never entered    : {len(live_unentered_all)}")
    print(f"  reach worklist      : {len(live_unentered)}")
    print(f"  disclosed dead      : {len(unentered_disclosed)}")
    print(f"not observable (const): {len(not_observable_const)}")
    print(f"not observable        : {len(unobservable)}")
    for label, _ in sources:
        mine = per_source_live[label]
        others = set()
        for other, _ in sources:
            if other != label:
                others |= per_source_live[other]
        print(f"  {label:<28}: {len(mine):>4} live entered, {len(mine - others):>4} unique")
    if missing_canonical:
        print(
            "PARTIAL UNION - no per-ladder export for: "
            + ", ".join(missing_canonical),
            file=sys.stderr,
        )
    # `relative_to` raises on an --out that is not under the repo (a relative
    # path resolved against a different cwd, /tmp, ...), which would throw away
    # the whole report after writing it. Fall back to the path as given.
    try:
        shown = args.out.resolve().relative_to(REPO)
    except ValueError:
        shown = args.out
    print(f"report -> {shown}")

    if args.fail_on_disclosed and disclosed_entered:
        print(
            f"\nFAIL: {len(disclosed_entered)} NOT WIRED anchor(s) executed by a "
            "passing replay.",
            file=sys.stderr,
        )
        return 1
    return 0
