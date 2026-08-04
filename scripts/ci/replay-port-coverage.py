#!/usr/bin/env python3
"""Join a replay's runtime coverage against the port catalog.

Every other reachability number in this repo is computed by the engine about
the engine: `port-catalog.py --live` walks a *static* call graph from the host
roots and asks whether a `// PORT:`-tagged Rust symbol is reachable in
principle. That graph is deliberately permissive (see the two-graph split in
`docs/tooling/port-catalog.md`), so "live" is an upper bound - it says
reachable, not reached.

This script supplies the missing denominator: what a **pad-driven playthrough
actually executes**. It runs (or consumes) `cargo llvm-cov` output for a replay
test and joins the per-function execution counts against the catalog's
address -> (file, line) anchors, reporting three sets:

  inert-entered   an address the static graph calls NOT reachable from any host
                  root, whose anchor ran anyway. The graph was wrong, or the
                  tag is on the wrong symbol. Each one is a finding.

  disclosed-entered
                  an anchor carrying a `NOT WIRED:` disclosure that ran anyway.
                  These are the dangerous ones: a passing oracle traversed code
                  the source says nothing reaches, so the oracle may be
                  certifying a stub.

  live-unentered  an address the static graph calls live that the run never
                  entered. Not a defect - this is the prioritisation list, and
                  it is only meaningful relative to how far the replay gets.

Usage:
    # consume an existing export
    scripts/ci/replay-port-coverage.py --json target/replay-cov.json

    # produce one first (slow: instrumented build of the engine crates)
    cargo llvm-cov --release -p legaia-engine-shell \\
        --test critical_path_replay --json --output-path target/replay-cov.json

Skips (exit 0) when the coverage JSON is absent, so a disc-free or
coverage-toolless CI run is a pass, matching the `LEGAIA_DISC_BIN` convention.
"""

from __future__ import annotations

import argparse
import bisect
import importlib.util
import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
CATALOG = REPO / "scripts" / "ci" / "port-catalog.py"
DEFAULT_JSON = REPO / "target" / "replay-cov.json"
DEFAULT_OUT = REPO / "target" / "port-catalog" / "replay-port-entry.md"


def load_catalog_module():
    """Import `port-catalog.py` (hyphenated, so not a normal import)."""
    spec = importlib.util.spec_from_file_location("port_catalog", CATALOG)
    if spec is None or spec.loader is None:
        raise SystemExit(f"cannot load {CATALOG}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def catalog_address_sets() -> tuple[set[str], set[str]]:
    """`(live, not_live)` address sets, from the catalog's own liveness pass.

    Shelled out rather than reimplemented: the verdict logic (module-scope and
    type-scope anchors, test-file exclusion, the receiver-gated sibling graph)
    is subtle enough that a second copy would drift, and the whole point of
    this script is to check the catalog rather than to restate it.
    """

    def run(flag: str) -> set[str]:
        proc = subprocess.run(
            [sys.executable, str(CATALOG), flag, "--no-write"],
            capture_output=True,
            text=True,
            cwd=REPO,
        )
        if proc.returncode != 0:
            raise SystemExit(f"port-catalog.py {flag} failed:\n{proc.stderr}")
        out = set()
        for line in proc.stdout.splitlines():
            token = line.split(None, 1)[0] if line.split() else ""
            if len(token) == 8 and all(c in "0123456789abcdef" for c in token):
                out.add(token.lower())
        return out

    return run("--live-only"), run("--not-live")


class FileCoverage:
    """Executed-line lookup for one source file, from llvm-cov regions.

    A function's regions are collapsed to its line span plus whether any region
    executed. An anchor is then resolved to a function the same way
    `collect_port_anchors` resolves it to a symbol: a tag inside a body belongs
    to the enclosing function, a tag above an item belongs to the next one.
    """

    def __init__(self):
        # (line_start, line_end, executed) per function in this file.
        self.spans: list[tuple[int, int, bool]] = []
        self._starts: list[int] = []

    def add_function(self, line_start: int, line_end: int, executed: bool):
        self.spans.append((line_start, line_end, executed))

    def finalize(self):
        self.spans.sort(key=lambda s: s[0])
        self._starts = [s[0] for s in self.spans]

    def any_executed(self) -> bool:
        return any(s[2] for s in self.spans)

    def verdict_at(self, line: int) -> bool | None:
        """`True`/`False` if a function covers or follows `line`, else `None`."""
        # 1. Enclosing function (tag written inside a body).
        enclosing = [s for s in self.spans if s[0] <= line <= s[1]]
        if enclosing:
            # Innermost wins: nested closures report their own spans.
            enclosing.sort(key=lambda s: s[1] - s[0])
            return enclosing[0][2]
        # 2. Otherwise the next function starting at/after the tag line.
        idx = bisect.bisect_left(self._starts, line)
        if idx < len(self.spans):
            return self.spans[idx][2]
        return None


def load_coverage(path: Path) -> dict[str, FileCoverage]:
    """Parse an `llvm-cov export` JSON into per-file function coverage."""
    raw = json.loads(path.read_text())
    files: dict[str, FileCoverage] = defaultdict(FileCoverage)
    for block in raw.get("data", []):
        for fn in block.get("functions", []):
            filenames = fn.get("filenames") or []
            regions = fn.get("regions") or []
            if not filenames or not regions:
                continue
            # regions: [line_start, col_start, line_end, col_end, count,
            #           file_id, expanded_file_id, kind]
            per_file: dict[int, list[tuple[int, int, int]]] = defaultdict(list)
            for r in regions:
                if len(r) < 6:
                    continue
                per_file[r[5]].append((r[0], r[2], r[4]))
            for file_id, rs in per_file.items():
                if file_id >= len(filenames):
                    continue
                name = filenames[file_id]
                lo = min(r[0] for r in rs)
                hi = max(r[1] for r in rs)
                executed = any(r[2] > 0 for r in rs)
                try:
                    rel = str(Path(name).resolve().relative_to(REPO))
                except ValueError:
                    rel = name
                files[rel].add_function(lo, hi, executed)
    for cov in files.values():
        cov.finalize()
    return files


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--json", type=Path, default=DEFAULT_JSON)
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT)
    ap.add_argument(
        "--fail-on-disclosed",
        action="store_true",
        help="exit non-zero if a NOT WIRED anchor was executed by the run",
    )
    args = ap.parse_args()

    if not args.json.is_file():
        print(f"[skip] {args.json} missing - run cargo llvm-cov first")
        return 0

    cov = load_coverage(args.json)
    if not cov:
        print(f"[skip] {args.json} carries no function regions")
        return 0

    catalog = load_catalog_module()
    srcs = catalog.load_rust_sources()
    anchors = catalog.collect_port_anchors(srcs)
    live, not_live = catalog_address_sets()

    inert_entered: list[tuple[str, dict]] = []
    disclosed_entered: list[tuple[str, dict]] = []
    live_entered: set[str] = set()
    live_unentered: list[tuple[str, dict]] = []
    # An address whose every anchor file is absent from this binary's coverage
    # was never *observable* - it is compiled into a different target (wasm-only
    # crates, other binaries). Counting it as "never entered" would inflate the
    # worklist with rows this run could not have exercised either way, and would
    # make the zero buckets below look better-founded than they are.
    unobservable: list[tuple[str, dict]] = []

    for addr, sites in sorted(anchors.items()):
        entered_site = None
        observable = False
        for site in sites:
            fc = cov.get(site["file"])
            if fc is None:
                continue
            observable = True
            if site["kind"] == "module":
                ran = fc.any_executed()
            else:
                v = fc.verdict_at(site["line"])
                ran = bool(v)
            if ran:
                entered_site = site
                break
        if entered_site is not None:
            if addr in not_live:
                inert_entered.append((addr, entered_site))
            if entered_site.get("not_wired_tag"):
                disclosed_entered.append((addr, entered_site))
            if addr in live:
                live_entered.add(addr)
        elif not observable:
            unobservable.append((addr, sites[0]))
        elif addr in live:
            live_unentered.append((addr, sites[0]))

    # Non-vacuity: the two zero-valued buckets above are only meaningful if the
    # join actually looked at those addresses. Report the observable share so a
    # zero produced by a lookup miss cannot be read as a clean result.
    not_live_observable = sum(
        1 for a in not_live if a in anchors and any(s["file"] in cov for s in anchors[a])
    )

    lines: list[str] = []
    w = lines.append
    w("# Replay port-entry report")
    w("")
    w(
        "Runtime join: which `// PORT:`-tagged addresses a pad-driven replay "
        "actually executed, against the static liveness verdict. See the "
        "script header for why the static number alone is an upper bound."
    )
    w("")
    w(f"- ported addresses with anchors: **{len(anchors)}**")
    w(f"- statically live: **{len(live)}**, of which entered by the run: **{len(live_entered)}**")
    w(f"- statically not-live: **{len(not_live)}**, of which entered anyway: **{len(inert_entered)}**")
    w(f"- `NOT WIRED`-disclosed anchors executed: **{len(disclosed_entered)}**")
    w(f"- not observable in this binary (excluded above): **{len(unobservable)}**")
    w("")
    w(
        f"Non-vacuity: **{not_live_observable} of {len(not_live)}** not-live addresses "
        "have at least one anchor file present in the coverage data, so the "
        "inert-entered count is a measurement over that many addresses rather "
        "than a lookup miss."
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
        "The static graph reports no host root reaches these, yet the replay "
        "executed the anchor. Either the graph is wrong or the tag sits on the "
        "wrong symbol - each row is a finding, not a metric.",
    )
    table(
        "Disclosed `NOT WIRED` anchors executed",
        disclosed_entered,
        "The source disclaims these as unreached, and a **passing** oracle ran "
        "them anyway. Highest-priority rows: an oracle that traverses "
        "undisclosed-stub code can certify behaviour nothing implements.",
    )
    table(
        "Live but never entered",
        live_unentered,
        "Statically reachable, not reached by this run. Not defects - this is "
        "the wiring worklist ordered by what a playthrough actually needs, and "
        "it shrinks as the replay reaches further.",
    )

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("\n".join(lines) + "\n")

    print(f"ported anchors        : {len(anchors)}")
    print(f"live / entered        : {len(live)} / {len(live_entered)}")
    print(f"not-live / entered    : {len(not_live)} / {len(inert_entered)}")
    print(f"  (observable         : {not_live_observable} / {len(not_live)})")
    print(f"NOT WIRED executed    : {len(disclosed_entered)}")
    print(f"live never entered    : {len(live_unentered)}")
    print(f"not observable        : {len(unobservable)}")
    print(f"report -> {args.out.relative_to(REPO)}")

    if args.fail_on_disclosed and disclosed_entered:
        print(
            f"\nFAIL: {len(disclosed_entered)} NOT WIRED anchor(s) executed by a "
            "passing replay.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
