"""Command line: parse the flags, collect, and hand the rows to one renderer.
"""

import argparse
from pathlib import Path

from port_catalog import paths
from port_catalog.anchors import collect_port_anchors
from port_catalog.citations import (
    collect_citations,
    collect_doc_citations,
    collect_dumped,
    collect_ports,
)
from port_catalog.features import bfs_reachable, build_call_graph, load_features
from port_catalog.live import compute_live
from port_catalog.paths import DEFAULT_FUNCS, FEATURES_TOML, OUT_DIR
from port_catalog.ratchet import run_ratchet
from port_catalog.render.catalog import render_csv, render_md
from port_catalog.render.dashboard import render_dashboard
from port_catalog.render.live_audit import render_live_audit
from port_catalog.render.summary import summarize
from port_catalog.rows import build_rows, filter_rows, load_ignore
from port_catalog.rust_graph import build_rust_graph, reachable_fns
from port_catalog.rust_source import collect_roots, load_rust_sources
from port_catalog.selftest import run_selftest

# `--help` leads with the first line of the package docstring.
from port_catalog import __doc__


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--md",
        action="store_true",
        help="emit markdown to stdout instead of CSV-to-file (CSV/MD still written to target/port-catalog/)",
    )
    ap.add_argument("--top", type=int, default=0, help="cap output rows (0 = no cap)")
    ap.add_argument("--addr", type=str, default="", help="single-address drill-down")
    ap.add_argument(
        "--missing-ports",
        action="store_true",
        help="filter: dumped + documented but not ported (port worklist)",
    )
    ap.add_argument(
        "--missing-dumps",
        action="store_true",
        help="filter: cited but not yet dumped (dump worklist)",
    )
    ap.add_argument(
        "--ported-only",
        action="store_true",
        help="filter: only addresses with a // PORT: tag",
    )
    ap.add_argument(
        "--ignored-only",
        action="store_true",
        help="filter: only addresses in scripts/ci/port-catalog-ignore.toml",
    )
    ap.add_argument(
        "--live",
        action="store_true",
        help="run the Rust reachability pass and add the `live` column "
        "(slower: parses every crates/**/src/**.rs)",
    )
    ap.add_argument(
        "--not-live",
        action="store_true",
        help="filter: ported but not reachable from a host root (implies --live)",
    )
    ap.add_argument(
        "--live-only",
        action="store_true",
        help="filter: ported and reachable from a host root (implies --live)",
    )
    ap.add_argument(
        "--live-audit",
        action="store_true",
        help="emit the live-reachability audit (stale NOT WIRED tags + "
        "undisclosed inert ports) to target/port-catalog/live-audit.md",
    )
    ap.add_argument(
        "--include-ignored",
        action="store_true",
        help="don't exclude ignore-list entries from --missing-ports / "
             "--missing-dumps (default: exclude)",
    )
    ap.add_argument(
        "--bucket",
        choices=["scus", "overlay"],
        help="filter: only SCUS-resident or only overlay-resident addresses",
    )
    ap.add_argument(
        "--no-write",
        action="store_true",
        help="don't write CSV/MD artifacts to target/port-catalog/",
    )
    ap.add_argument(
        "--feature",
        type=str,
        default="",
        help="filter to addresses reachable from a feature's roots (see scripts/ci/features.toml)",
    )
    ap.add_argument(
        "--list-features",
        action="store_true",
        help="list available features in scripts/ci/features.toml and exit",
    )
    ap.add_argument(
        "--dashboard",
        action="store_true",
        help="emit open-work rollup (per-feature stats + top-N missing-ports + "
        "ignore summary) to target/port-catalog/open-work.md and exit",
    )
    ap.add_argument(
        "--dashboard-top",
        type=int,
        default=10,
        help="per-feature top-N missing-ports cap for --dashboard (default: 10)",
    )
    ap.add_argument(
        "--check",
        action="store_true",
        help="ratchet the worklist + ported counts against "
        "scripts/ci/port-catalog-baseline.json and exit non-zero on a "
        "regression, or on a baselined figure this run did not compute "
        "(add --live for the disclosure gap)",
    )
    ap.add_argument(
        "--allow-uncompared",
        action="store_true",
        help="with --check: downgrade 'a baselined figure went uncompared' from "
        "a failure to a printed warning. For the pre-commit hook, which cannot "
        "afford --live; CI must not pass it",
    )
    ap.add_argument(
        "--update-baseline",
        action="store_true",
        help="rewrite the ratchet baseline, carrying forward any figure this "
        "run did not compute (say why in the commit message)",
    )
    ap.add_argument(
        "--funcs",
        default=str(DEFAULT_FUNCS),
        help="dump corpus directory - point at a checkout that has one when "
        "running from somewhere that does not (the corpus is gitignored)",
    )
    ap.add_argument(
        "--selftest",
        action="store_true",
        help="run the anchor-resolution / disclosure-precedence control suite "
        "on a synthetic corpus and exit (0 = pass, 2 = resolver broken)",
    )
    args = ap.parse_args()

    paths.FUNCS_DIR = Path(args.funcs).expanduser()

    if args.selftest:
        print("port-catalog anchor-resolver self-test")
        return run_selftest()

    if args.list_features:
        features = load_features()
        if not features:
            print(f"no features found in {FEATURES_TOML}")
            return 0
        print(f"features in {FEATURES_TOML}:")
        for name, body in features.items():
            roots = ", ".join(body["roots"]) or "(none)"
            stop_at = (
                f"  stop_at: {', '.join(sorted(body['stop_at']))}\n"
                if body["stop_at"]
                else ""
            )
            depth = (
                f"  max_depth: {body['max_depth']}\n"
                if body["max_depth"] is not None
                else ""
            )
            print(f"\n[{name}]")
            print(f"  description: {body['description']}")
            print(f"  roots: {roots}")
            if stop_at:
                print(stop_at.rstrip())
            if depth:
                print(depth.rstrip())
        return 0

    if (args.check or args.update_baseline) and not paths.FUNCS_DIR.exists():
        # The `dumped` column reads the gitignored Ghidra corpus. Without it
        # every row is undumped, which turns `ported but NOT dumped` from 4 into
        # the whole port set - a failure about the checkout, not about the tree.
        # Same skip-clean contract as disc-coverage.py and the disc-gated tests.
        print(
            f"[port-catalog] SKIPPED - no dump corpus at {paths.FUNCS_DIR} "
            "(gitignored; the catalog's `dumped` column, and therefore this "
            "ratchet, only measures locally)."
        )
        return 0

    dumped = collect_dumped()
    refs, sources = collect_citations()
    docs = collect_doc_citations()
    ports = collect_ports()
    ignore = load_ignore()

    want_live = args.live or args.not_live or args.live_only or args.live_audit
    live_map: dict[str, dict] | None = None
    if want_live:
        srcs = load_rust_sources()
        fns, edges = build_rust_graph(srcs)
        roots = collect_roots(srcs)
        reach = reachable_fns(fns, edges, roots)
        # Second, receiver-gated pass. Only the stale-`NOT WIRED` test reads it;
        # every `live` / `--not-live` verdict stays on the permissive graph so
        # the not-live list keeps its hard-floor property.
        #
        # Built whenever the live pass runs, NOT only for `--live-audit`. It
        # used to be audit-only, and `compute_live` falls back to
        # `live_strict = live` without it - so the summary's last line, which
        # is labelled as the receiver-gated answer, silently printed the
        # permissive one in every run that did not pass the audit flag. On one
        # tree that read `42` under `--live` and `0` under `--live-audit
        # --live`: same tree, same question, two answers, and the larger one
        # accuses 42 correct disclosures of being stale. A number that moves
        # with an output flag is not a measurement.
        #
        # The second graph costs nothing worth saving: both invocations above
        # were dominated by parsing every crate source, and the difference
        # between them was inside the run-to-run spread on a loaded machine.
        _, edges_s = build_rust_graph(srcs, strict=True)
        reach_strict = reachable_fns(fns, edges_s, roots)
        live_map = compute_live(
            collect_port_anchors(srcs), srcs, fns, reach, reach_strict
        )

    rows = build_rows(dumped, refs, sources, docs, ports, ignore=ignore, live=live_map)

    if args.check or args.update_baseline:
        return run_ratchet(rows, args)

    # Always write the global catalog so the latest state is on disk even when
    # the user is also drilling into a feature filter.
    if not args.no_write:
        render_csv(rows, OUT_DIR / "catalog.csv")
        render_md(rows, OUT_DIR / "catalog.md", "Port catalog (global)")

    if args.live_audit:
        out_path = None if args.no_write else OUT_DIR / "live-audit.md"
        md = render_live_audit(rows, out_path)
        if args.md:
            print(md)
        else:
            print(summarize(rows))
            if out_path:
                print()
                print(f"wrote {out_path}")
        return 0

    if args.dashboard:
        features = load_features()
        out_path = None if args.no_write else OUT_DIR / "open-work.md"
        md = render_dashboard(rows, sources, features, args.dashboard_top, out_path)
        if args.md:
            print(md)
        else:
            print(summarize(rows))
            if out_path:
                print()
                print(f"wrote {out_path}")
        return 0

    # Feature filter trims rows to BFS-reachable addresses + writes a
    # per-feature artifact under target/port-catalog/<feature>.{csv,md}.
    feature_meta: dict | None = None
    if args.feature:
        features = load_features()
        if args.feature not in features:
            print(
                f"unknown feature: {args.feature!r}. "
                f"Available: {', '.join(features) or '(none - populate scripts/ci/features.toml)'}"
            )
            return 1
        feature_meta = features[args.feature]
        graph = build_call_graph(sources)
        reachable = bfs_reachable(
            graph,
            feature_meta["roots"],
            stop_at=feature_meta["stop_at"],
            max_depth=feature_meta["max_depth"],
        )
        rows = [r for r in rows if r["addr"] in reachable]
        if not args.no_write:
            title = f"Port catalog - feature: {args.feature}"
            render_csv(rows, OUT_DIR / f"{args.feature}.csv")
            render_md(rows, OUT_DIR / f"{args.feature}.md", title)

    filtered = filter_rows(rows, args)

    if args.md:
        title = "Port catalog"
        if args.feature:
            title += f" - feature: {args.feature}"
        if args.missing_ports:
            title += " - port worklist (dumped + documented, not ported)"
        elif args.missing_dumps:
            title += " - dump worklist (cited, not dumped)"
        elif args.ported_only:
            title += " - ported only"
        elif args.addr:
            title += f" - {args.addr}"
        print(render_md(filtered, None, title))
        return 0

    if feature_meta is not None:
        print(f"[feature: {args.feature}]")
        print(f"  description: {feature_meta['description']}")
        print(f"  roots:       {', '.join(feature_meta['roots']) or '(none)'}")
        if feature_meta["stop_at"]:
            print(f"  stop_at:     {', '.join(sorted(feature_meta['stop_at']))}")
        print(f"  reachable:   {len(rows)} addresses")
        print()
    print(summarize(rows))
    if filtered != rows:
        print()
        print(f"filtered rows ({len(filtered)}):")
        print(f"{'addr':<10} {'bucket':<8} {'D/d/P/I':<8} {'refs':>4}  port crates / ignore / first src")
        print(f"{'-' * 10} {'-' * 8} {'-' * 8} {'-' * 4}  {'-' * 60}")
        for r in filtered:
            flags = (
                ("D" if r["dumped"] else ".")
                + ("d" if r["documented"] else ".")
                + ("P" if r["ported"] else ".")
                + ("I" if r["ignored"] else ".")
                + (("L" if r["live"] else "x") if r["live_known"] else ".")
            )
            crates = ",".join(r["port_crates"]) if r["port_crates"] else "-"
            if r["ported"]:
                tail = crates
            elif r["ignored"]:
                tail = f"[{r['ignore_category']}] {r['ignore_reason']}"
            else:
                tail = r["first_sources"][0] if r["first_sources"] else "-"
            print(f"{r['addr']:<10} {r['bucket']:<8} {flags:<8} {r['refs']:>4}  {tail}")
    if not args.no_write:
        print()
        print(f"wrote {OUT_DIR / 'catalog.csv'}")
        print(f"wrote {OUT_DIR / 'catalog.md'}")
    return 0
