"""Signal `module-orphan`: a tag whose routine shares nothing with the rest
of its module's ports.
"""

from __future__ import annotations

from collections import defaultdict

from port_provenance.dump_features import elsewhere_claims, features
from port_provenance.dumps import Dump, consensus, coverage, trusted_jals
from port_provenance.findings import Finding
from port_provenance.tags import Tag
from port_provenance.thresholds import (
    ADAPTIVE_MIN_TAGS,
    ADAPTIVE_TARGET,
    DF_LADDER,
    DISTINCTIVE_DF_MAX,
    DUMP_COVERAGE_MIN,
    ORPHAN_MIN_SIBLINGS,
)


def find_module_orphans(
    tags: list[Tag],
    by_addr: dict[str, list[Dump]],
    df: dict[int, int],
    cdf: dict[int, int],
) -> list[Finding]:
    """A tagged routine that corroborates none of its module's other tags.

    Ranked, not accused: a module may legitimately port an unrelated kernel, and
    a routine whose dump is short enough to form nothing distinctive lands here
    for no reason at all. What lifts a row to the top of the list is the second
    half - the same routine's rare table turning up under a module with an
    unrelated name (`elsewhere_claims`). That conjunction is the shop shape.

    The cut at which "distinctive" is measured follows the module (`DF_LADDER`),
    because a fixed one reads some modules and not others, and the modules it
    cannot read are a recognisable kind rather than a random sample - see the
    ladder's own comment.
    """
    claims = elsewhere_claims(tags, by_addr, df, cdf)
    per_file: dict[str, set[str]] = defaultdict(set)
    for t in tags:
        if t.addr in by_addr:
            per_file[t.module].add(t.addr)

    # Who calls what, over the whole corpus. Two routines a third one calls are
    # siblings in that caller's eyes whatever tables they touch, and this is the
    # one corroboration channel the feature intersection cannot express: it is
    # not a shared third party and not an edge between the two.
    callers: dict[int, set[str]] = defaultdict(set)
    for a, dumps in by_addr.items():
        for d in consensus(dumps):
            for t in trusted_jals(d):
                callers[t].add(a)

    out: list[Finding] = []
    for rel, addrs in sorted(per_file.items()):
        if len(addrs) < ORPHAN_MIN_SIBLINGS:
            continue
        srt = sorted(addrs)
        pairs = [(a, b) for i, a in enumerate(srt) for b in srt[i + 1:]]
        # Calling a sibling is the strongest corroboration there is, and the
        # feature-set intersection misses it entirely: the callee is the
        # sibling's own address, never a shared third party. `save_select.rs`
        # read as having an orphan that calls four of its own siblings.
        calls_sibling = {
            (a, b)
            for a, b in pairs
            if any(int(b, 16) in d.jals for d in consensus(by_addr[a]))
            or any(int(a, 16) in d.jals for d in consensus(by_addr[b]))
            or callers[int(a, 16)] & callers[int(b, 16)]
        }

        def cohesion(cut: int) -> tuple[dict[str, tuple[set[int], set[int]]], set[str]]:
            fe = {a: features(by_addr[a], df, cdf, cut, cut) for a in addrs}
            linked = {
                (a, b)
                for a, b in pairs
                if (a, b) in calls_sibling
                or fe[a][0] & fe[b][0]
                or fe[a][1] & fe[b][1]
            }
            return fe, {x for a, b in linked for x in (a, b)}

        adopted = DISTINCTIVE_DF_MAX
        feats, cohesive = cohesion(adopted)
        if len(addrs) >= ADAPTIVE_MIN_TAGS:
            # Raise the cut until the module reads as cohesive. A module that
            # never does is one the signal cannot read at all, and silence is
            # the honest output - not one finding per member.
            for adopted in DF_LADDER:
                feats, cohesive = cohesion(adopted)
                if len(cohesive) >= ADAPTIVE_TARGET * len(addrs):
                    break
            if len(cohesive) < ADAPTIVE_TARGET * len(addrs):
                continue
        # The floor under both paths: a *majority* of the module's
        # evidence-bearing tags must corroborate before a member of the minority
        # can be called foreign. A tag whose routine forms nothing distinctive
        # cannot corroborate anything, so counting it against the module's
        # cohesion measures the corpus, not the module - which is why the
        # denominator here is the informative tags and not all of them.
        informative = {a for a in addrs if feats[a][0] or feats[a][1]}
        if len(cohesive) < 2 or 2 * len(cohesive) <= len(informative):
            continue
        for a in sorted(addrs - cohesive):
            g, j = feats[a]
            # A routine with no formed data of its own has one bit of evidence -
            # a single callee it does not share - and one bit does not carry a
            # claim about which subsystem the routine belongs to.
            if not g:
                continue
            # Corroboration and orphanhood need different evidence, and the
            # asymmetry is the whole reason this test sits here and not in
            # `features`. "These two touch the same table" is an existence
            # claim a fragment can settle, so a fragmentary sibling still
            # corroborates. "This one touches nothing any of them touch" is a
            # claim about the whole body, and a dump that printed a tenth of it
            # cannot make it. Putting the floor in `features` instead silenced
            # the field VM's own wrong tag, because its module's dispatcher
            # sibling is dumped as a fragment.
            if all(
                coverage(d) < DUMP_COVERAGE_MIN for d in consensus(by_addr[a])
            ):
                continue
            elsewhere = claims.get((rel, a), [])
            # Rank by how isolated the routine is and how specific its evidence
            # is, NOT by how much of it there is: an earlier formula multiplied
            # by the size of the feature set and simply sorted the big routines
            # to the top, where being big is not evidence of anything.
            rarest = min((df.get(x, 99) for x in g), default=99)
            rank = (
                (10.0 if elsewhere else 0.0)
                + 5.0 * len(cohesive) / len(addrs)
                + 2.0 * min(len({o for _, o, _ in elsewhere}), 3)
                + 1.0 / max(rarest, 1)
            )
            tag = next(t for t in tags if t.addr == a and t.module == rel)
            lines = [
                f"FUN_{a} shares no distinctive data address and no callee with "
                f"any of the {len(cohesive)} corroborating siblings tagged in "
                f"this module ({', '.join(sorted(cohesive))}); "
                f"distinctive = formed by at most {adopted} dumped routine(s).",
                "    its distinctive data: "
                + (", ".join(f"0x{x:08x}" for x in sorted(g)) or "(none)"),
                "    its callees: "
                + (", ".join(f"0x{x:08x}" for x in sorted(j)) or "(none)"),
            ]
            for data_addr, oaddr, orel in sorted(elsewhere)[:6]:
                lines.append(
                    f"    0x{data_addr:08x} is also formed by FUN_{oaddr}, "
                    f"PORT-tagged in {orel}"
                )
            out.append(
                Finding("module-orphan", f"module-orphan:{rel}:{a}", a,
                        f"{tag.file}:{tag.line}", rank, lines)
            )
    return out
