#!/usr/bin/env python3
"""Join a replay's runtime coverage against the port catalog.

Every other reachability number in this repo is computed by the engine about
the engine: `port-catalog.py --live` walks a *static* call graph from the host
roots and asks whether a `// PORT:`-tagged Rust symbol is reachable in
principle. That graph is deliberately permissive (see the two-graph split in
`docs/tooling/port-catalog.md`), so "live" is an upper bound - it says
reachable, not reached.

This script supplies the missing denominator: what a **pad-driven run actually
executes**. It consumes `cargo llvm-cov` output for one or more replay ladders
and joins the per-function execution counts against the catalog's
address -> (file, line) anchors, reporting three sets:

  inert-entered   an address the static graph calls NOT reachable from any host
                  root, whose anchor ran anyway. The graph was wrong, or the
                  tag is on the wrong symbol. Each one is a finding.

  disclosed-entered
                  an anchor carrying a `NOT WIRED:` disclosure that ran anyway.
                  These are the dangerous ones: a passing oracle traversed code
                  the source says nothing reaches, so the oracle may be
                  certifying a stub.

  live-unentered  an address the static graph calls live that no run entered.
                  Not a defect - this is the prioritisation list, and it is only
                  meaningful relative to how far the ladders get.

## The denominator is a union of ladders, not one session

`--json` is **repeatable**, and the difference matters more than it looks. The
repo drives several pad-only ladders, each its own test binary and each at full
score: `critical_path_replay` (the world spine), `menu_replay` (the pause menu
and save UI), `minigame_replay` (the five minigame doors), `v0_1_playthrough`
(the cold-boot field anchor), and `play_compose_ladder` (the browser play
page's draw composition - see below). A join over only the
first reports the menu and minigame subsystems as never-entered - and those two
are the largest clusters in the live-unentered list, so a worklist ordered off
a single-binary run is ordered against a measurement that structurally excluded
its own top rows.

## One ladder is a rendering host

The four `engine-shell` ladders drive the headless `BootSession`, which builds
no draw list - so `engine-ui` reported zero executed regions across their whole
union, and the largest NO-LADDER cluster in `docs/tooling/reach-triage.md` was
invisible to this report by construction. `play_compose_ladder`
(`crates/web-viewer/tests/`) closes that shape: the browser play page's
composition is *library* code, so the ladder feeds pad words per tick and calls
the page's per-frame read surface (`play_overlay_draws_json`, the menu / battle
/ fishing / dev-menu overlays, the screen-prim route, the battle 3D + FX
exports). It is still pad-only - nothing is seated or poked - but its frames
are composed, which is exactly the half of a rendering host the native window
keeps locked in its `bin/` target.

So the headline number is the **union** across every `--json` given, and the
per-source table below it reports what each ladder contributed and how much of
that no other ladder reached. A union across separate sessions is not one
continuous playthrough and the report says so; what it does measure honestly is
"code some pad-driven run entered" versus "code no pad-driven run entered".

## The union is the default, and a short join says so

With no `--json`, the script discovers `target/cov-*.json` and joins **all** of
them, so the bare invocation is the union rather than one binary. It also knows
the canonical ladder set ([`CANONICAL_LADDERS`]) and names any member whose
export is missing, in the report and on stdout - because a union over three of
the four is not a smaller version of the number, it is a different number, and
the earlier single-binary default understated the denominator by 1.7x without
saying anything at all.

The cost is why this stays a manual step rather than a pre-commit gate: each
export is an instrumented release build plus a full disc-gated ladder run, and
the ladders are minutes each. `--fail-on-disclosed` is the gateable part, and
it is the part that needs no complete union to be meaningful.

Usage:
    # produce one export per ladder (slow: instrumented release build of the
    # engine crates). One export each rather than one multi---test run: the
    # per-ladder table is only possible when the exports are separate, and
    # that table is what makes dropping a ladder a visible cost. The package
    # differs per ladder ([`CANONICAL_LADDERS`] carries it): the engine-shell
    # ladders and the web-viewer composition ladder are different crates, and
    # `--test` links each crate's LIBRARY - which is why the web-viewer route
    # can execute composition code the engine-shell `bin/` never exposes.
    for t in critical_path_replay menu_replay minigame_replay v0_1_playthrough; do
        cargo llvm-cov --release -p legaia-engine-shell \\
            --test "$t" --json --output-path "target/cov-$t.json"
    done
    cargo llvm-cov --release -p legaia-web-viewer --test play_compose_ladder \\
        --json --output-path target/cov-play_compose_ladder.json
    # NOTE the missing --release on these two; see "a release export loses
    # executed code" below. The flag is kept above only because the five
    # exports it produced are what the current numbers were measured from.
    cargo llvm-cov -p legaia-mdec --test w1a_fmv_ladder \\
        --json --output-path target/cov-w1a_fmv_ladder.json
    cargo llvm-cov -p legaia-engine-core --test w1a_narration_ladder \\
        --json --output-path target/cov-w1a_narration_ladder.json

    # then join them - no arguments needed, the union is the default
    scripts/ci/replay-port-coverage.py

## A release export loses executed code, silently

`-C instrument-coverage` counters are emitted per function, but an optimised
build inlines the small ones away and the out-of-line copy's record is left at
**zero** - indistinguishable, in this join, from a function no run called. It is
not a rare shape: on one `w1a_fmv_ladder` run, `advance_slice` (40 calls),
`slice_word_count` (39) and `mdec_output_control` / `skip_requested` /
`mdec_control_flags` (1-3 each) all reported `count = 0` under `--release` and
their real counts under the default profile, from the same ladder over the same
disc. Two of them are `mdec` rows this report had listed as *live but never
entered*, which was an artifact of the export rather than a gap in the ladders.

So a `--release` row in the never-entered list is a **hypothesis**, not a
measurement: re-export that ladder without `--release` before reading it as
work. The flag survives above only because the five exports it produced are what
the existing numbers came from; re-taking them is the fix, and it changes the
headline.

Skips (exit 0) when every coverage JSON is absent, so a disc-free or
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
COV_DIR = REPO / "target"
COV_GLOB = "cov-*.json"
LEGACY_JSON = COV_DIR / "replay-cov.json"
DEFAULT_OUT = REPO / "target" / "port-catalog" / "replay-port-entry.md"

# The pad-only ladders that make up the canonical denominator, in the order
# the usage block exports them, as `(test_name, cargo_package)`. Membership is
# "drives the engine with `set_pad` and nothing else" - a ladder that seats the
# player is measuring a different thing (see `critical_path_replay`'s module
# docs on the spine oracle). `play_compose_ladder` additionally *composes* a
# frame per tick from the browser play page's builders, which is what makes
# the union see a rendering host at all (see the module docstring).
#
# This list exists so a *partial* union is visible. Four of the five is not a
# conservative version of the number; it is a number about four ladders, and
# without naming the absentee it reads exactly like the full one.
CANONICAL_LADDERS = [
    ("critical_path_replay", "legaia-engine-shell"),
    ("menu_replay", "legaia-engine-shell"),
    ("minigame_replay", "legaia-engine-shell"),
    ("v0_1_playthrough", "legaia-engine-shell"),
    ("play_compose_ladder", "legaia-web-viewer"),
    # --- lane W1-A ---
    # Two ladders for content no pad-only *engine-shell* ladder can reach.
    #
    # `w1a_fmv_ladder` plays every retail `fmv_id` through the retail STR chain
    # (dispatch slot -> file seek -> `StrPlayer` -> `MdecDecoder` -> pad skip).
    # No other union member plays a movie at all: the headless ladders have no
    # cutscene rung and the browser play page's FMV arm auto-skips, so the whole
    # `mdec` crate reported zero executed regions - a fact about the harness,
    # not the port. Its fourth rung additionally *spawns* `CARGO_BIN_EXE_mdec`,
    # which is how the MDEC-hardware half (whose only host is the `mdec
    # str-plan` subcommand, in a `bin/` target) runs under coverage at all.
    #
    # That rung also settles a structural claim this report has been read
    # against: "a `bin/` target is unreachable from any `#[test]`" is true of
    # *calls* and false of *coverage*. `LLVM_PROFILE_FILE` is inherited, so a
    # spawned `CARGO_BIN_EXE_*` writes its own profile and it is merged in -
    # measured: `mdec::cmd_str_plan` reports 40 executions from a test that
    # only spawned it. Every `bin/`-resident row on this report's never-entered
    # list is therefore reachable by a ladder that runs the subcommand.
    #
    # `w1a_narration_ladder` drives the two text presenters a pad run reaches
    # and no ladder did: the opening-prologue subtitle roller and the inline
    # field-VM conversation path (as opposed to the pre-decoded dialog panel
    # every other ladder drives).
    #
    # Both are disc-gated and skip-pass without `LEGAIA_DISC_BIN`, exactly like
    # the ladders above; a disc-free export therefore contributes no executed
    # regions rather than failing, which is the same shape the rest of the union
    # already has.
    #
    # Both are exported WITHOUT `--release`, and that is not a detail: see the
    # module docstring's "a release export loses executed code". These two
    # ladders are where that was measured.
    ("w1a_fmv_ladder", "legaia-mdec"),
    ("w1a_narration_ladder", "legaia-engine-core"),
    # --- lane W1-C ---
    ("w1c_battle_render_ladder", "legaia-web-viewer"),
    ("w1c_arts_swing_ladder", "legaia-engine-shell"),
]
CANONICAL_LADDER_NAMES = [name for name, _pkg in CANONICAL_LADDERS]


def discover_inputs() -> tuple[list[Path], list[str]]:
    """`(paths, missing_canonical_labels)` for a bare invocation.

    Every `target/cov-*.json` is joined, not just the canonical set, so a
    ladder added later is picked up without editing this file. The legacy
    single-file path is honoured only when the glob finds nothing at all.
    """
    found = sorted(COV_DIR.glob(COV_GLOB))
    if not found and LEGACY_JSON.is_file():
        # The pre-per-ladder path: one merged export whose contents cannot be
        # attributed to a ladder. Every canonical name is reported unjoined,
        # which is the honest reading - the per-ladder table cannot be built
        # from it, so nothing here can say which ladders it covers.
        return [LEGACY_JSON], list(CANONICAL_LADDER_NAMES)
    labels = {p.stem.removeprefix("cov-") for p in found}
    missing = [name for name in CANONICAL_LADDER_NAMES if name not in labels]
    return found, missing


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
        # One llvm-cov export carries a record for every binary in the target
        # dir - `cargo llvm-cov --test X` still merges the coverage *mappings*
        # of the sibling test binaries built earlier into the same report, as
        # zero-count duplicates of the same functions. Collapse identical
        # spans with OR before sorting, or an unexecuted duplicate from a
        # binary the run never launched shadows the executed record and the
        # entered count collapses (measured: 265 -> 122 on the same run the
        # moment a second test binary existed in the target dir).
        merged: dict[tuple[int, int], bool] = {}
        for lo, hi, ex in self.spans:
            merged[(lo, hi)] = merged.get((lo, hi), False) or ex
        self.spans = [(lo, hi, ex) for (lo, hi), ex in merged.items()]
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
    args = ap.parse_args()

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

    inert_entered: list[tuple[str, dict]] = []
    disclosed_entered: list[tuple[str, dict]] = []
    live_entered: set[str] = set()
    live_unentered: list[tuple[str, dict]] = []
    # An address whose every anchor file is absent from *every* source's
    # coverage was never observable - it is compiled into a different target
    # (wasm-only crates, other binaries). Counting it as "never entered" would
    # inflate the worklist with rows no run could have exercised either way,
    # and would make the zero buckets below look better-founded than they are.
    unobservable: list[tuple[str, dict]] = []
    # label -> the live addresses that source entered. Drives the per-source
    # contribution table: an address several ladders reach is credited to each,
    # and the `unique` column is what would be lost by dropping that ladder.
    per_source_live: dict[str, set[str]] = {label: set() for label, _ in sources}

    def ran_in(cov: dict[str, FileCoverage], site: dict) -> bool | None:
        """`None` when this source cannot observe the site at all."""
        fc = cov.get(site["file"])
        if fc is None:
            return None
        if site["kind"] == "module":
            return fc.any_executed()
        return bool(fc.verdict_at(site["line"]))

    for addr, sites in sorted(anchors.items()):
        entered_site = None
        observable = False
        for label, cov in sources:
            for site in sites:
                ran = ran_in(cov, site)
                if ran is None:
                    continue
                observable = True
                if ran:
                    if entered_site is None:
                        entered_site = site
                    if addr in live:
                        per_source_live[label].add(addr)
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
    # zero produced by a lookup miss cannot be read as a clean result. Observed
    # across the union - an address one ladder's binary cannot see may still be
    # compiled into another's.
    all_files = {f for _, cov in sources for f in cov}
    not_live_observable = sum(
        1 for a in not_live if a in anchors and any(s["file"] in all_files for s in anchors[a])
    )

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
    w(f"- statically not-live: **{len(not_live)}**, of which entered anyway: **{len(inert_entered)}**")
    w(f"- `NOT WIRED`-disclosed anchors executed: **{len(disclosed_entered)}**")
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
        "Disclosed `NOT WIRED` anchors executed",
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
        "actually needs, and it shrinks as the ladders reach further.",
    )

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("\n".join(lines) + "\n")

    print(f"sources               : {', '.join(label for label, _ in sources)}")
    print(f"ported anchors        : {len(anchors)}")
    print(f"live / entered        : {len(live)} / {len(live_entered)}")
    print(f"not-live / entered    : {len(not_live)} / {len(inert_entered)}")
    print(f"  (observable         : {not_live_observable} / {len(not_live)})")
    print(f"NOT WIRED executed    : {len(disclosed_entered)}")
    print(f"live never entered    : {len(live_unentered)}")
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


if __name__ == "__main__":
    raise SystemExit(main())
