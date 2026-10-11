"""The port catalog as this report reads it: live and ported address sets,
and the addresses a disclosure marks dead.
"""

from __future__ import annotations

import importlib.util
import subprocess
import sys

from replay_port_coverage.paths import CATALOG, REPO


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


def disclosed_dead_addresses(catalog, srcs: dict) -> dict[str, str]:
    """Live addresses whose liveness is the permissive graph's alone and whose
    every live anchor disclaims a host - `{addr: "NOT WIRED" | "REPLACED-BY"}`.

    The catalog's `live` is the permissive graph (a name-matched call edge is
    enough), and the receiver-gated sibling graph only feeds the
    stale-disclosure test (`port-catalog.py --live-audit`). An address can
    therefore be `live` on the strength of an edge the strict graph refuses,
    while its own `PORT:` block says no host is owed a call. Such an address
    reads "live but never entered" here and nowhere else: `--live-audit` does
    not see it (it is live) and the stale test does not fire (the strict graph
    agrees with the disclosure). Three instruments then agree it is dead - the
    disclosure, the strict graph and the coverage - and listing it as reach
    work only buries the rows a ladder could convert.

    It is kept as its own bucket rather than dropped: the headline
    never-entered count is unchanged, and a row in this bucket that a ladder
    later enters surfaces as `disclosed_entered`, the loudest bucket.

    Computed in-process with the catalog's own liveness pass - the same
    functions `--live` runs, so there is no second copy of the verdict.
    """
    fns, edges = catalog.build_rust_graph(srcs)
    roots = catalog.collect_roots(srcs)
    reach = catalog.reachable_fns(fns, edges, roots)
    _, edges_s = catalog.build_rust_graph(srcs, strict=True)
    reach_s = catalog.reachable_fns(fns, edges_s, roots)
    # Anchor uids are resolved against the graph, so collect after building it.
    live_map = catalog.compute_live(
        catalog.collect_port_anchors(srcs), srcs, fns, reach, reach_s
    )
    out: dict[str, str] = {}
    for addr, row in live_map.items():
        if not row["live"] or row["live_strict"]:
            continue
        live_anchors = [a for a in row["anchors"] if a["live"]]
        if not live_anchors:
            continue
        if all(a["not_wired_tag"] or a["replaced_tag"] for a in live_anchors):
            replaced = all(a["replaced_tag"] for a in live_anchors)
            out[addr.lower()] = "REPLACED-BY" if replaced else "NOT WIRED"
    return out
