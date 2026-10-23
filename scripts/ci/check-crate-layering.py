#!/usr/bin/env python3
"""The engine's kernel crates depend on each other only along the edges
`scripts/ci/crate-layering.toml` allows.

Every crate names the crate that owns the code it uses, so a `Cargo.toml`
edge is the layering. An edge added for one symbol looks exactly like an edge
a crate's role calls for, and nothing else reads the difference: the compiler
accepts any acyclic graph. This gate reads each kernel crate's manifest and
fails on

- an engine-layer dependency its row does not allow,
- a row entry the manifest no longer declares (the table stays the graph),
- a non-engine workspace dependency outside a row's `foreign` set,
- a format, tool or patcher crate that depends on an engine-layer crate,
- a cycle in the table, a row for a crate that does not exist, or a
  `[[coupling]]` whose edge is gone.

`--ranks` prints the stack the table derives. Exits 1 on any finding.
"""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
TABLE = Path(__file__).resolve().parent / "crate-layering.toml"

# Engine-layer crates that are not named `engine-*`.
ENGINE_LAYER_EXTRA = {
    "render-kernels",
    "parity",
    "asset-viewer",
    "studio",
    "web-minigames",
    "web-play",
    "web-viewer",
}


def is_engine_layer(crate: str) -> bool:
    return crate.startswith("engine-") or crate in ENGINE_LAYER_EXTRA


def manifest_deps(manifest: dict) -> tuple[set[str], set[str]]:
    """Workspace crates a manifest names: `(normal + build, dev)`, as
    directory names (`legaia-foo` -> `foo`). Target-specific tables count."""

    def names(table: dict) -> set[str]:
        return {k.removeprefix("legaia-") for k in table if k.startswith("legaia-")}

    normal: set[str] = set()
    dev: set[str] = set()
    scopes = [manifest] + list(manifest.get("target", {}).values())
    for scope in scopes:
        normal |= names(scope.get("dependencies", {}))
        normal |= names(scope.get("build-dependencies", {}))
        dev |= names(scope.get("dev-dependencies", {}))
    return normal, dev


def derive_ranks(rows: dict[str, dict]) -> tuple[dict[str, int], list[str]]:
    """Rank = 1 + the highest rank among `deps`. Returns the ranks and the
    crates left over when the table has a cycle."""
    ranks: dict[str, int] = {}
    pending = set(rows)
    while pending:
        ready = [
            c
            for c in pending
            if all(d in ranks for d in rows[c].get("deps", []) if d in rows)
        ]
        if not ready:
            return ranks, sorted(pending)
        for c in ready:
            below = [ranks[d] for d in rows[c].get("deps", []) if d in rows]
            ranks[c] = 1 + max(below) if below else 0
            pending.discard(c)
    return ranks, []


def main() -> int:
    table = tomllib.loads(TABLE.read_text())
    rows: dict[str, dict] = table.get("crates", {})
    couplings: list[dict] = table.get("coupling", [])
    bad: list[str] = []

    declared: dict[str, tuple[set[str], set[str]]] = {}
    for path in sorted(REPO.glob("crates/*/Cargo.toml")):
        declared[path.parent.name] = manifest_deps(tomllib.loads(path.read_text()))

    for crate, row in rows.items():
        if crate not in declared:
            bad.append(f"{crate}: has a row but crates/{crate}/Cargo.toml does not exist")
            continue
        for dep in row.get("deps", []) + row.get("dev", []):
            if dep not in rows:
                bad.append(
                    f"{crate}: row allows `{dep}`, which has no row of its own "
                    "(a kernel crate depends only on kernel crates)"
                )
        normal, dev = declared[crate]
        allowed = set(row.get("deps", []))
        allowed_dev = allowed | set(row.get("dev", []))
        engine_normal = {d for d in normal if is_engine_layer(d)}
        engine_dev = {d for d in dev if is_engine_layer(d)}
        for dep in sorted(engine_normal - allowed):
            bad.append(
                f"{crate} -> {dep}: [dependencies] edge not in the table. Move the "
                "items that need it to a crate below both, or add the edge to "
                f"`deps` of [crates.{crate}] with the role that justifies it"
            )
        for dep in sorted(engine_dev - allowed_dev):
            bad.append(
                f"{crate} -> {dep}: [dev-dependencies] edge not in the table "
                f"(a test of {dep}'s code belongs in crates/{dep}/tests; "
                f"otherwise add it to `dev` of [crates.{crate}])"
            )
        for dep in sorted(allowed - engine_normal):
            bad.append(f"{crate} -> {dep}: allowed by the table but not declared; delete it from `deps`")
        for dep in sorted(set(row.get("dev", [])) - engine_dev):
            bad.append(f"{crate} -> {dep}: allowed as dev by the table but not declared; delete it from `dev`")
        if "foreign" in row:
            foreign = {d for d in normal if not is_engine_layer(d)}
            want = set(row["foreign"])
            for dep in sorted(foreign - want):
                bad.append(f"{crate} -> {dep}: non-engine dependency outside the row's `foreign` set")
            for dep in sorted(want - foreign):
                bad.append(f"{crate} -> {dep}: in the row's `foreign` set but not declared; delete it")

    for crate, (normal, dev) in declared.items():
        if is_engine_layer(crate):
            continue
        for dep in sorted(d for d in normal | dev if is_engine_layer(d)):
            bad.append(f"{crate} -> {dep}: a crate outside the engine layer depends on an engine-layer crate")

    ranks, cyclic = derive_ranks(rows)
    if cyclic:
        bad.append(f"the table has a cycle among: {', '.join(cyclic)}")

    for c in couplings:
        src, dst = c.get("from", "?"), c.get("to", "?")
        if not c.get("reason") or not c.get("items"):
            bad.append(f"[[coupling]] {src} -> {dst}: needs `items` and a `reason`")
        if src not in rows or dst not in rows[src].get("deps", []):
            bad.append(f"[[coupling]] {src} -> {dst}: the table has no such edge; delete the coupling")

    if "--ranks" in sys.argv[1:]:
        by_rank: dict[int, list[str]] = {}
        for crate, rank in ranks.items():
            by_rank.setdefault(rank, []).append(crate)
        for rank in sorted(by_rank):
            print(f"rank {rank}: {', '.join(sorted(by_rank[rank]))}")

    if bad:
        print("[crate-layering] FAIL", file=sys.stderr)
        for line in bad:
            print(f"  {line}", file=sys.stderr)
        print(f"  table: {TABLE.relative_to(REPO)}", file=sys.stderr)
        return 1
    edges = sum(len(r.get("deps", [])) for r in rows.values())
    print(
        f"[crate-layering] OK - {len(rows)} kernel crate(s), {edges} allowed edge(s), "
        f"{len(couplings)} named coupling(s)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
