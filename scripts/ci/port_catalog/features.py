"""Feature views: the dump-citation call graph, a bounded walk over it, and
the feature definitions in `scripts/ci/features.toml`.
"""

import re
import tomllib
from collections import defaultdict, deque

from port_catalog.paths import FEATURES_TOML


def build_call_graph(sources: dict[str, list[str]]) -> dict[str, set[str]]:
    """Build forward call-graph edges: `{src_addr: set(cited_addr)}`.

    Each dump filename ends with an 8-hex-digit address that identifies the
    function the dump documents. For every (cited_addr, [src_files]) entry in
    `sources`, we derive an edge `src_file_addr -> cited_addr`.

    Note: edges only exist between addresses that are *dumped*. Undumped
    helpers have no outgoing edges - the BFS frontier widens only as more
    dumps land.
    """
    graph: dict[str, set[str]] = defaultdict(set)
    addr_re = re.compile(r"([0-9a-fA-F]{8})$", re.IGNORECASE)
    for cited_addr, src_files in sources.items():
        for src in src_files:
            m = addr_re.search(src)
            if not m:
                continue
            src_addr = m.group(1).lower()
            if src_addr == cited_addr:
                continue
            graph[src_addr].add(cited_addr)
    return graph


def bfs_reachable(
    graph: dict[str, set[str]],
    roots: list[str],
    stop_at: set[str] | None = None,
    max_depth: int | None = None,
) -> set[str]:
    """Return the set of addresses reachable from `roots` via citation edges.

    `stop_at` addresses are kept in the result but their callees aren't followed
    - useful for stopping at shared-infrastructure boundaries (e.g. the generic
    CD loader) so a feature filter doesn't pull in everything reachable.
    """
    visited: set[str] = set()
    stop_at = stop_at or set()
    queue: deque[tuple[str, int]] = deque()
    for r in roots:
        rl = r.lower()
        if rl not in visited:
            visited.add(rl)
            queue.append((rl, 0))
    while queue:
        addr, depth = queue.popleft()
        if addr in stop_at:
            continue
        if max_depth is not None and depth >= max_depth:
            continue
        for nxt in graph.get(addr, ()):
            if nxt not in visited:
                visited.add(nxt)
                queue.append((nxt, depth + 1))
    return visited


def load_features() -> dict[str, dict]:
    """Load feature definitions from scripts/ci/features.toml. Returns {} if missing."""
    if not FEATURES_TOML.exists():
        return {}
    with FEATURES_TOML.open("rb") as f:
        data = tomllib.load(f)
    out = {}
    for name, body in data.items():
        if not isinstance(body, dict):
            continue
        out[name] = {
            "description": body.get("description", ""),
            "roots": [r.lower() for r in body.get("roots", [])],
            "stop_at": {a.lower() for a in body.get("stop_at", [])},
            "max_depth": body.get("max_depth"),
        }
    return out
