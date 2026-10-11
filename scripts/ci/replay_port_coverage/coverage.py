"""llvm-cov exports, and the join from a port anchor to the lines that ran.
"""

from __future__ import annotations

import bisect
import json
from collections import defaultdict
from pathlib import Path

from replay_port_coverage.paths import REPO


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

    def span_verdict(self, lo: int, hi: int) -> bool | None:
        """Verdict for a known source span; `None` if no record overlaps it.

        `None` is the case `verdict_at` cannot express: a generic function is
        only emitted where it is instantiated, so a binary that never names it
        carries no record for it at all, and "the next function after the tag"
        is then a different routine whose coverage is not this one's.
        """
        hits = [ex for a, b, ex in self.spans if a <= hi and lo <= b]
        if not hits:
            return None
        return any(hits)

    def executed_overlapping(self, lo: int, hi: int) -> bool:
        """Whether any executed span overlaps the line range `[lo, hi]`.

        The lookup for a *known* function span (an `fn` the catalog parsed from
        source), where `verdict_at`'s next-function approximation is both
        unnecessary and wrong - the source span and the llvm-cov span differ by
        attribute lines, so overlap is the honest join.
        """
        return any(ex for a, b, ex in self.spans if a <= hi and lo <= b)


def _reduced_path(path: Path) -> Path:
    return path.with_name(path.name + ".reduced")


def _load_reduced(path: Path) -> dict[str, FileCoverage] | None:
    """The finalized spans of an export, if a current reduction is on disk.

    A raw export is tens of MB and the union is dozens of them, so a bare join
    spent minutes re-parsing JSON it had already read. The reduction keeps
    exactly what the join consumes - per file, the merged `(lo, hi, executed)`
    spans - and is trusted only while it is newer than its export.
    """
    red = _reduced_path(path)
    try:
        if red.stat().st_mtime < path.stat().st_mtime:
            return None
        data = json.loads(red.read_text())
    except (OSError, ValueError):
        return None
    files: dict[str, FileCoverage] = {}
    for rel, spans in data.items():
        fc = FileCoverage()
        fc.spans = [(lo, hi, bool(ex)) for lo, hi, ex in spans]
        fc._starts = [sp[0] for sp in fc.spans]
        files[rel] = fc
    return files


def load_coverage(path: Path) -> dict[str, FileCoverage]:
    """Parse an `llvm-cov export` JSON into per-file function coverage."""
    cached = _load_reduced(path)
    if cached is not None:
        return cached
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
    try:
        _reduced_path(path).write_text(
            json.dumps({rel: fc.spans for rel, fc in files.items()})
        )
    except OSError:
        pass
    return files


def join_anchor_coverage(
    catalog,
    anchors: dict[str, list[dict]],
    srcs: dict,
    sources: list[tuple[str, dict[str, "FileCoverage"]]],
    live: set[str],
    not_live: set[str],
) -> dict:
    """Join every anchor against the coverage union; return the buckets.

    Split out of `main` so `--selftest` can drive it on a synthetic corpus.
    `catalog` is the imported `port-catalog.py` module - the item-reference
    rule comes from there (`item_reference_patterns` / `item_reference_hit`)
    so this join can never drift from the liveness verdict it is checking.
    """
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
    # Item anchors (const / static / type alias) with no executed attributable
    # reference. See the module docstring: not "entered", not "never entered".
    not_observable_const: list[tuple[str, dict]] = []
    per_source_live: dict[str, set[str]] = {label: set() for label, _ in sources}

    all_files = {f for _, cov in sources for f in cov}

    def fn_executed_labels(f) -> set[str] | None:
        """Labels of sources that executed `f`; `None` if none can see it."""
        src = srcs[f.path]
        rel = str(f.path.relative_to(catalog.REPO))
        lo = f.line
        hi = src.line_of(max(f.body_end - 1, f.body_start))
        seen, labels = False, set()
        for label, cov in sources:
            fc = cov.get(rel)
            if fc is None:
                continue
            seen = True
            if fc.executed_overlapping(lo, hi):
                labels.add(label)
        return labels if seen else None

    # Pre-resolve every item-kind anchor once, across the union: the verdict
    # depends on the referencing functions' coverage, not on the anchor's own
    # file. `(executed_labels, observable)` per (path, symbol).
    item_keys: dict[tuple, str | None] = {}
    for sites in anchors.values():
        for site in sites:
            if site["kind"] == "item":
                item_keys[(site["path"], site["symbol"])] = site["type_name"]
    item_exec: dict[tuple, tuple[set[str], bool]] = {}
    for (p, n), ty in item_keys.items():
        bare, qual = catalog.item_reference_patterns(p, n, ty)
        labels: set[str] = set()
        observable = str(p.relative_to(catalog.REPO)) in all_files
        for src in srcs.values():
            for f in src.fns:
                if f.is_test:
                    continue
                body = src.stripped[f.body_start : f.body_end]
                if not catalog.item_reference_hit(f.path, body, p, bare, qual):
                    continue
                got = fn_executed_labels(f)
                if got is None:
                    continue
                observable = True
                labels |= got
        item_exec[(p, n)] = (labels, observable)

    # Per-file impl-block types and per-(file, type) non-test methods, for the
    # type-kind verdict below (mirrors the catalog's `type_scope` + its
    # no-`impl`-in-file fallback to module scope).
    impl_types: dict = {}
    methods_of: dict[tuple, list] = defaultdict(list)
    for p, src in srcs.items():
        impl_types[p] = {ty for _a, _b, ty, *_ in src.impl_spans}
        for f in src.fns:
            if not f.is_test and f.impl_type:
                methods_of[(p, f.impl_type)].append(f)

    def anchored_fn(site: dict):
        """The parsed `fn` a function anchor names: the next one of that name
        below the tag, else the one of that name the tag sits inside."""
        named = [f for f in srcs[site["path"]].fns if f.name == site["symbol"]]
        below = [f for f in named if f.line >= site["line"]]
        if below:
            return min(below, key=lambda f: f.line)
        above = [f for f in named if f.line < site["line"]]
        return max(above, key=lambda f: f.line) if above else None

    def facade_subtree(rel: str) -> str | None:
        """`foo/` for a `foo.rs` whose module body lives in that directory."""
        if not rel.endswith(".rs") or rel.endswith(("/mod.rs", "/lib.rs", "/main.rs")):
            return None
        return rel[: -len(".rs")] + "/"

    def ran_in(cov: dict[str, "FileCoverage"], site: dict) -> bool | None:
        """`None` when this source cannot observe the site at all."""
        fc = cov.get(site["file"])
        if fc is None:
            # A `//! PORT:` tag on a facade - a `foo.rs` that only declares the
            # submodules under `foo/` - names a file with no function, so no
            # binary ever carries a record for it and the address read "not
            # observable" however often the module ran. A module tag already
            # means "anything in this module ran"; for a facade the module is
            # its subtree, so that is where the verdict is read.
            if site["kind"] != "module":
                return None
            prefix = facade_subtree(site["file"])
            if prefix is None:
                return None
            sub = [c for f, c in cov.items() if f.startswith(prefix)]
            if not sub:
                return None
            return any(c.any_executed() for c in sub)
        if site["kind"] == "module":
            return fc.any_executed()
        if site["kind"] == "type":
            src = srcs[site["path"]]
            for f in methods_of[(site["path"], site["type_name"])]:
                lo = f.line
                hi = src.line_of(max(f.body_end - 1, f.body_start))
                if fc.executed_overlapping(lo, hi):
                    return True
            # A type the file never `impl`s (a plain data struct, a trait
            # whose implementations live elsewhere) has no method here to
            # observe - fall back to the file, as the liveness pass does.
            if site["type_name"] not in impl_types[site["path"]]:
                return fc.any_executed()
            return False
        f = anchored_fn(site) if site["kind"] == "fn" else None
        if f is not None:
            src = srcs[f.path]
            return fc.span_verdict(
                f.line, src.line_of(max(f.body_end - 1, f.body_start))
            )
        return bool(fc.verdict_at(site["line"]))

    for addr, sites in sorted(anchors.items()):
        entered_site = None
        observable = False
        const_site = None
        # A `//! PORT:` module tag reads "anything in this file ran", so when
        # the same address ALSO carries a function-level tag in that file,
        # the module tag would credit the address off any sibling - a trig
        # table, a helper another module calls - while the routine the
        # function tags name never ran. `FUN_801CFA48` read entered that way:
        # `effect_ribbon.rs`'s only executed code was the `RetailTrig` LUT
        # `effect_default_arm` borrows, and no ribbon was ever built. The
        # function tags are the more precise witness, so they decide.
        fn_tag_files = {s["file"] for s in sites if s["kind"] == "fn"}
        for label, cov in sources:
            for site in sites:
                if site["kind"] == "item":
                    continue
                if site["kind"] == "module" and site["file"] in fn_tag_files:
                    continue
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
        if entered_site is None:
            for site in sites:
                if site["kind"] != "item":
                    continue
                labels, item_obs = item_exec[(site["path"], site["symbol"])]
                if labels:
                    entered_site = site
                    if addr in live:
                        for label in labels:
                            per_source_live[label].add(addr)
                    break
                if item_obs and const_site is None:
                    const_site = site
        if entered_site is not None:
            if addr in not_live:
                inert_entered.append((addr, entered_site))
            # `REPLACED-BY:` disclaims at least as hard as `NOT WIRED:` does -
            # it says no host is owed, not merely that none exists - so an
            # oracle that runs one is the same finding. Reading only
            # `not_wired_tag` here would have hidden every replaced anchor from
            # this bucket the moment the third class landed.
            if entered_site.get("not_wired_tag") or entered_site.get("replaced_tag"):
                disclosed_entered.append((addr, entered_site))
            if addr in live:
                live_entered.add(addr)
        elif observable:
            if addr in live:
                # Name the function tag when there is one: it is the site the
                # verdict was taken from (see `fn_tag_files` above).
                shown = next((x for x in sites if x["kind"] == "fn"), sites[0])
                live_unentered.append((addr, shown))
        elif const_site is not None:
            not_observable_const.append((addr, const_site))
        else:
            unobservable.append((addr, sites[0]))

    # Non-vacuity: the zero-valued buckets above are only meaningful if the
    # join actually looked at those addresses. Report the observable share so a
    # zero produced by a lookup miss cannot be read as a clean result. Observed
    # across the union - an address one ladder's binary cannot see may still be
    # compiled into another's. An item anchor counts through its references.
    def addr_observable(a: str) -> bool:
        for s in anchors[a]:
            if s["kind"] == "item":
                if item_exec[(s["path"], s["symbol"])][1]:
                    return True
            elif s["file"] in all_files:
                return True
            elif s["kind"] == "module":
                prefix = facade_subtree(s["file"])
                if prefix and any(f.startswith(prefix) for f in all_files):
                    return True
        return False

    not_live_observable = sum(1 for a in not_live if a in anchors and addr_observable(a))

    return {
        "inert_entered": inert_entered,
        "disclosed_entered": disclosed_entered,
        "live_entered": live_entered,
        "live_unentered": live_unentered,
        "unobservable": unobservable,
        "not_observable_const": not_observable_const,
        "per_source_live": per_source_live,
        "not_live_observable": not_live_observable,
    }
