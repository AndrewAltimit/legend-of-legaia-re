"""`--page-audit`: rows of `docs/tooling/reach-triage.md` that are no longer
live.
"""

from __future__ import annotations

import csv
import re

from replay_port_coverage.paths import REPO


def page_audit() -> int:
    """Which addresses on `reach-triage.md` have left it without a ladder.

    This page is the per-row verdict for the *live but never entered* set, so a
    row belongs on it only while its address is `live`. There are three ways off
    it: a ladder enters the address (this script's own report), the wiring lands
    (`port-catalog.py --live-audit`), and a third that neither instrument
    announces - **the static verdict moves**. A re-key, a `NOT WIRED:`
    disclosure or a `REPLACED-BY:` declaration can make an address read *inert*,
    at which point `--live-audit` owns the row and this page does not.

    Answering that needs no coverage export, which is the point: an export is a
    full instrumented build per ladder, and this is a join between one committed
    page and `target/port-catalog/catalog.csv`. Run it before spending the
    export - a row that has already left is not work the ladders can convert.
    """
    page = REPO / "docs" / "tooling" / "reach-triage.md"
    csv_path = REPO / "target" / "port-catalog" / "catalog.csv"
    if not page.is_file():
        print(f"[skip] {page} missing")
        return 0
    if not csv_path.is_file():
        print(f"[skip] {csv_path} missing - run "
              "`python3 scripts/ci/port-catalog.py --live` first")
        return 0
    cited = sorted({m.lower() for m in
                    re.findall(r"`(80[0-9a-fA-F]{6})`", page.read_text())})
    rows = {}
    with csv_path.open() as fh:
        for row in csv.DictReader(fh):
            rows[row["addr"].lower()] = row

    # `live` is written only by a run that did the reachability pass. Every
    # other mode - the default, `--dashboard`, `--missing-ports` - leaves the
    # column EMPTY, and an empty column reads exactly like "not live": the join
    # then reports every cited address as having left the page, which is the
    # loudest possible output and says nothing. Refuse the join instead, the way
    # `check-port-provenance.py` refuses an empty dump corpus.
    ported = [r for r in rows.values() if r.get("ported") == "1"]
    if ported and not any(r.get("live") == "1" for r in ported):
        print(f"[skip] {csv_path} has {len(ported)} ported row(s) and no `live` "
              "column - it was written without the reachability pass. Re-run "
              "`python3 scripts/ci/port-catalog.py --live` and try again; "
              "joining against an unpopulated column would call every cited "
              "address dead.")
        return 0

    left: list[tuple[str, str]] = []
    unported: list[str] = []
    absent: list[str] = []
    for a in cited:
        row = rows.get(a)
        if row is None:
            absent.append(a)
        elif row.get("ported") != "1":
            unported.append(a)
        elif row.get("live") != "1":
            why = ("REPLACED-BY" if row.get("replaced_tag") == "1"
                   else "NOT WIRED" if row.get("not_wired_tag") == "1"
                   else "no disclosure")
            left.append((a, why))

    print(f"reach-triage.md cites {len(cited)} address(es); "
          f"{len(cited) - len(left) - len(unported) - len(absent)} are still "
          f"live ports.")
    if left:
        print(f"\n{len(left)} address(es) are ported but NO LONGER live - the "
              "row belongs to --live-audit, not to this page:")
        for a, why in left:
            print(f"  {a}  ({why})")
    if unported:
        print(f"\n{len(unported)} address(es) carry no PORT tag any more:")
        for a in unported:
            print(f"  {a}")
    if absent:
        print(f"\n{len(absent)} address(es) are in no catalog row at all "
              "(cited as retail addresses, never ported):")
        print("  " + " ".join(absent[:24])
              + (f" ... +{len(absent) - 24}" if len(absent) > 24 else ""))
    if not (left or unported):
        print("\nEvery cited address is still a live port; nothing has taken "
              "the static-verdict exit.")
    return 0
