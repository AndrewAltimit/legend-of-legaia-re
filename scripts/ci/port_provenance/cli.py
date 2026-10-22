"""Command line and report: run every signal, apply waivers, print the ranked
worklist.
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import defaultdict
from pathlib import Path

from port_provenance import paths
from port_provenance.call_sites import images
from port_provenance.dumps import load_corpus
from port_provenance.path_check import check_declared_paths
from port_provenance.paths import DEFAULT_EXTRACTED, DEFAULT_FUNCS, WAIVERS
from port_provenance.rows import IMAGE_ENTRY_ACCEPTED, SITE_ACCEPTED
from port_provenance.signals.absent_citations import find_absent_citations
from port_provenance.signals.doc_citations import (
    find_doc_row_citations,
    find_dual_labels,
)
from port_provenance.signals.module_orphans import find_module_orphans
from port_provenance.signals.non_entry import find_non_entry_tags
from port_provenance.tags import CODE_ADDR, collect_tags
from port_provenance.waivers import load_live, load_waivers

# `--help` leads with the first line of the package docstring.
from port_provenance import __doc__

# ---------------------------------------------------------------------------


def resolve_extracted(arg: str | None, funcs: Path) -> Path:
    """The extracted-disc directory a run reads.

    An explicit `--extracted` wins. Otherwise a `--funcs` that names another
    checkout's `ghidra/scripts/funcs` brings that checkout's `extracted/` with
    it: a worktree run that passed only `--funcs` used to fall back to the
    worktree's own (absent) `extracted/` and report the rows the image tests
    settle as findings.
    """
    if arg is not None:
        return Path(arg).expanduser()
    funcs = funcs.resolve()
    if funcs.parts[-3:] == ("ghidra", "scripts", "funcs"):
        sibling = funcs.parent.parent.parent / "extracted"
        if sibling.is_dir():
            return sibling
    return DEFAULT_EXTRACTED


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--addr", default="", help="single-address drill-down")
    ap.add_argument(
        "--signal",
        default="",
        choices=[
            "",
            "module-orphan",
            "absent-citation",
            "doc-citation",
            "dual-label",
            "non-entry",
        ],
        help="restrict to one signal",
    )
    ap.add_argument(
        "--live-only",
        action="store_true",
        help="only findings touching a live port (needs target/port-catalog/catalog.csv)",
    )
    ap.add_argument("--top", type=int, default=0, help="cap rows (0 = no cap)")
    ap.add_argument(
        "--show-waived", action="store_true", help="include waived findings"
    )
    ap.add_argument(
        "--strict",
        action="store_true",
        help="exit 1 when an unwaived finding remains (the ratchet)",
    )
    ap.add_argument(
        "--emit-waivers",
        action="store_true",
        help="print a TOML waiver stub for every current unwaived finding",
    )
    ap.add_argument(
        "--funcs",
        default=str(DEFAULT_FUNCS),
        help="dump corpus directory (a worktree has none - point at a checkout "
        "that does)",
    )
    ap.add_argument(
        "--extracted",
        default=None,
        help="extracted disc directory, for the caller/branch-site and "
        "image-entry tests (default: the `extracted/` of the checkout --funcs "
        "names, else this checkout's)",
    )
    ap.add_argument(
        "--allow-vacuous",
        action="store_true",
        help="exit 0 instead of 2 when the dump corpus or the extracted images "
        "are missing",
    )
    args = ap.parse_args()

    paths.FUNCS_DIR = Path(args.funcs).expanduser()
    paths.EXTRACTED = resolve_extracted(args.extracted, paths.FUNCS_DIR)

    # The rows are checked before the corpus is: a row that names nothing
    # changes which module every tag under it belongs to, so the report below
    # would be about a different grouping than the one the rows declare.
    stale_rows = check_declared_paths()
    if stale_rows:
        print(
            f"check-port-provenance: {len(stale_rows)} declared row(s) in "
            "scripts/ci/port_provenance/rows.py name a path that is not on disk:",
            file=sys.stderr,
        )
        for line in stale_rows:
            print(f"  - {line}", file=sys.stderr)
        return 1

    if not paths.FUNCS_DIR.exists() or not any(paths.FUNCS_DIR.glob("*.txt")):
        # An empty corpus makes every port look unimpeachable. Say so instead of
        # printing a clean run - a vacuous pass is the failure mode this repo
        # has hit before (check-port-tags.py's "0 warnings across 0 files").
        # A git worktree is the ordinary way to land here: the corpus is
        # gitignored, so it exists only in the checkout that dumped it.
        print(
            f"check-port-provenance: no dumps under {paths.FUNCS_DIR} - nothing to "
            "check. This is a vacuous pass, not a clean one. Re-run with "
            "`--funcs <dir>` pointing at a checkout that has the corpus, or "
            "pass --allow-vacuous to accept exit 0."
        )
        return 0 if args.allow_vacuous else 2

    by_addr, df, cdf = load_corpus()
    tags = collect_tags()

    findings = (
        find_module_orphans(tags, by_addr, df, cdf)
        + find_absent_citations(tags, by_addr)
        + find_doc_row_citations(by_addr)
        + find_dual_labels(by_addr)
        + find_non_entry_tags(tags)
    )
    # The unmatched-waiver report is a statement about the whole waiver file, so
    # it has to be computed before the display filters narrow the finding set. A
    # `--addr` drill-down otherwise reported every waiver in the file as
    # matching nothing, which reads as a repo-wide problem and is an artifact of
    # having asked about one address.
    all_keys = {f.key for f in findings}
    if args.signal:
        findings = [f for f in findings if f.signal == args.signal]
    if args.addr:
        # The doc-side signals scan every page, not the tag set, so filtering the
        # tags alone let a drill-down print the whole corpus back.
        want = args.addr.lower().removeprefix("0x")
        findings = [
            f
            for f in findings
            if f.addr == want or want in "\n".join(f.lines).lower()
        ]

    live = load_live()
    if args.live_only:
        if not live:
            print(
                "check-port-provenance: --live-only needs "
                "target/port-catalog/catalog.csv; run "
                "`python3 scripts/ci/port-catalog.py --live-only` first."
            )
            return 2
        findings = [
            f
            for f in findings
            if (f.addr and f.addr in live)
            or any(a in live for a in re.findall(CODE_ADDR, "\n".join(f.lines)))
        ]

    waivers = load_waivers()
    unwaived = [f for f in findings if f.key not in waivers]
    shown = findings if args.show_waived else unwaived
    shown.sort(key=lambda f: (-f.rank, f.signal, f.key))
    if args.top:
        shown = shown[: args.top]

    if args.emit_waivers:
        for f in sorted(unwaived, key=lambda x: x.key):
            print(f'[[waiver]]\nkey = "{f.key}"\nreason = "TODO"\n')
        return 0

    n_tags = len({(t.addr, t.file, t.line) for t in tags})
    n_addrs = len({t.addr for t in tags})
    print(
        f"check-port-provenance: {n_tags} PORT tag site(s) over {n_addrs} "
        f"address(es); {len(by_addr)} dumped address(es) parsed; "
        f"{len(df)} distinct data addresses formed corpus-wide."
    )
    imgs = images()
    if not imgs:
        # Without the images the caller/branch-site test and the image-entry
        # test are both off, and the run reports every row they settle: about
        # ninety findings that are not findings. A partial run that exits 0
        # reads as a worklist; it is the vacuous pass with a number attached.
        print(
            f"  no extracted images under {paths.EXTRACTED} - the caller/branch-site "
            "and image-entry tests are OFF, so rows citing a caller's jal site "
            "and image-qualified tags read as findings. Run legaia-extract, "
            "pass --extracted <dir>, or read this run as partial."
        )
    else:
        print(
            f"  {len(imgs)} extracted image(s) read; "
            f"{len(SITE_ACCEPTED)} citation(s) accepted as caller / branch / "
            f"in-body sites off the bytes."
        )
        for subj, cited, why in SITE_ACCEPTED:
            print(f"    FUN_{subj} <- 0x{cited}: {why}")
        accepted = sorted(set(IMAGE_ENTRY_ACCEPTED))
        print(
            f"  {len(accepted)} image-qualified tag site(s) accepted over a "
            "bare-VA not-an-entry verdict: the named image opens a routine there."
        )
        for subj, site, img in accepted:
            print(f"    FUN_{subj} @ {site}: prologue in {img}")
    for f in shown:
        mark = " [LIVE]" if f.addr and f.addr in live else ""
        waived = " [waived]" if f.key in waivers else ""
        print(f"\n{f.signal}  {f.where}{mark}{waived}")
        for line in f.lines:
            print("  " + line)
    # A waiver that matches nothing is a claim nobody can check any more: the
    # finding it excused is gone, or its key drifted when a neighbouring
    # citation started passing. Either way it should be deleted, not carried.
    unused = sorted(set(waivers) - all_keys)
    if unused:
        print(
            f"\n{len(unused)} waiver(s) in {WAIVERS.name} match no current "
            "finding - delete them, or check why the finding stopped firing:"
        )
        for key in unused:
            print(f"  {key}")

    by_signal = defaultdict(int)
    for f in unwaived:
        by_signal[f.signal] += 1
    print(
        f"\n{len(unwaived)} unwaived finding(s) "
        + (", ".join(f"{k}={v}" for k, v in sorted(by_signal.items())) or "(none)")
        + f"; {len(findings) - len(unwaived)} waived."
    )
    if not imgs and not args.allow_vacuous:
        print(
            "check-port-provenance: partial run (no extracted images); exiting "
            "2. Pass --extracted <dir>, or --allow-vacuous to accept it."
        )
        return 2
    if args.strict and unwaived:
        print(
            "check-port-provenance: --strict and unwaived findings remain. "
            "Fix the tag, or waive it in scripts/ci/port-provenance-waivers.toml "
            "with a reason."
        )
        return 1
    return 0
