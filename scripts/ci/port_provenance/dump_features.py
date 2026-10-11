"""What a dump is evidence of: its distinctive constants and callees, and the
other modules that claim the same ones.
"""

from __future__ import annotations

from collections import defaultdict

from port_provenance.dumps import Dump, consensus, extent_of, trusted_jals
from port_provenance.tags import Tag, module_tokens
from port_provenance.thresholds import (
    CALLEE_DF_MAX,
    OVERLAY_BASE,
    RAM_HI,
    RAM_LO,
    SPLIT_DF_MAX,
)


def features(
    dumps: list[Dump],
    df: dict[int, int],
    cdf: dict[int, int],
    df_max: int,
    callee_max: int | None = None,
) -> tuple[set[int], set[int]]:
    """(distinctive data addresses, distinctive callees) over an address's dumps."""
    cm = CALLEE_DF_MAX if callee_max is None else callee_max
    g: set[int] = set()
    j: set[int] = set()
    for d in consensus(dumps):
        g |= {
            a for a in d.formed if df.get(a, 0) <= df_max and RAM_LO <= a <= RAM_HI
        }
        j |= {x for x in trusted_jals(d) if cdf.get(x, 0) <= cm}
    return g, j


def elsewhere_claims(
    tags: list[Tag],
    by_addr: dict[str, list[Dump]],
    df: dict[int, int],
    cdf: dict[int, int],
) -> dict[tuple[str, str], list[tuple[int, str, str]]]:
    """For each (module, routine), the rare tables it shares with other modules.

    Standalone, this was a signal of its own and it did not work. Reporting every
    pair of PORT-tagged routines that share a rare table produced hundreds of
    findings, because sharing a subsystem's own tables is the normal case and a
    host dispatcher module pairs with everything it dispatches. Requiring a
    *consensus* to contradict did not rescue it either: the largest groups are
    fifteen battle modules where "the odd one out by name" is arbitrary.

    What survives is its use as corroboration. A routine that already shares
    nothing with its own module's siblings, and whose rare table is claimed by a
    module with an unrelated name, is the shop shape exactly - and that
    conjunction is rare enough to act on. So this returns evidence, and
    `find_module_orphans` decides whether there is a finding to attach it to.

    Overlay-band addresses are VA-aliased across images, so a claim only counts
    when the two routines were dumped from a common image.
    """
    prof: dict[tuple[str, str], tuple[set[int], set[str], list[tuple[int, int]]]] = {}
    for t in tags:
        dumps = by_addr.get(t.addr)
        if not dumps or (t.module, t.addr) in prof:
            continue
        prof[(t.module, t.addr)] = (
            features(dumps, df, cdf, SPLIT_DF_MAX)[0],
            {d.image.split(".")[0].split(" ")[0] for d in consensus(dumps)},
            [extent_of(d) for d in consensus(dumps)],
        )

    users: dict[int, list[tuple[str, str]]] = defaultdict(list)
    for key, (g, _img, extent) in prof.items():
        for a in g:
            if not any(lo <= a < hi for lo, hi in extent):
                users[a].append(key)

    out: dict[tuple[str, str], list[tuple[int, str, str]]] = defaultdict(list)
    for data_addr, keys in users.items():
        for rel, addr in keys:
            for orel, oaddr in keys:
                if orel == rel or oaddr == addr:
                    continue
                if module_tokens(rel) & module_tokens(orel):
                    continue
                if data_addr >= OVERLAY_BASE and not (
                    prof[(rel, addr)][1] & prof[(orel, oaddr)][1]
                ):
                    continue
                out[(rel, addr)].append((data_addr, oaddr, orel))
    return out
