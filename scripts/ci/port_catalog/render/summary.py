"""The plain-text summary every non-markdown run prints.
"""


def summarize(rows: list[dict]) -> str:
    n = len(rows)
    n_dumped = sum(1 for r in rows if r["dumped"])
    n_documented = sum(1 for r in rows if r["documented"])
    n_ported = sum(1 for r in rows if r["ported"])
    # Cross-cuts that the table is meant to surface
    dd = sum(1 for r in rows if r["dumped"] and r["documented"])
    ddp = sum(1 for r in rows if r["dumped"] and r["documented"] and r["ported"])
    dd_not_p = sum(
        1 for r in rows if r["dumped"] and r["documented"] and not r["ported"]
    )
    dd_not_p_ignored = sum(
        1
        for r in rows
        if r["dumped"] and r["documented"] and not r["ported"] and r["ignored"]
    )
    # An ignore row retires a dump-worklist row exactly as it retires a port
    # one. The `worklist_*` sections hold ADDRESS claims - "no routine begins at
    # this VA" - and an address claim is precisely a reason NOT to dump: an
    # address that starts no function cannot be dumped without fabricating an
    # entry point (docs/tooling/dump-corpus-integrity.md). Counting those rows
    # anyway left the ignore file unable to close the one worklist it is most
    # directly about, so a newly imported image could only ever raise it.
    cited_not_dumped = sum(
        1 for r in rows if r["refs"] > 0 and not r["dumped"] and not r["ignored"]
    )
    ported_not_documented = sum(
        1 for r in rows if r["ported"] and not r["documented"]
    )
    ported_not_dumped = sum(1 for r in rows if r["ported"] and not r["dumped"])
    live_block: list[str] = []
    if any(r["live_known"] for r in rows):
        ported_rows = [r for r in rows if r["ported"]]
        n_live = sum(1 for r in ported_rows if r["live"])
        n_inert = len(ported_rows) - n_live
        n_replaced = sum(1 for r in ported_rows if not r["live"] and r["replaced_tag"])
        pairs = [(r, a) for r in ported_rows for a in r["anchors"]]
        from port_catalog.render.live_audit import (
            DISCLOSED,
            LIVE,
            REPLACED,
            STALE,
            TAGGED_PERMISSIVE_ONLY,
            UNDISCLOSED,
            anchor_class,
        )

        # One partition, the same one the audit report's sections use: every
        # anchor is counted on exactly one line below.
        counts: dict[str, int] = {}
        for _, a in pairs:
            c = anchor_class(a)
            counts[c] = counts.get(c, 0) + 1
        n_a_live = counts.get(LIVE, 0)
        n_a_replaced = counts.get(REPLACED, 0)
        n_a_disclosed = counts.get(DISCLOSED, 0)
        n_a_undisclosed = counts.get(UNDISCLOSED, 0)
        # Stale-tag is the one question read off the receiver-gated graph:
        # a spurious edge here manufactures a false accusation against a
        # correct disclosure. See build_rust_graph.
        n_stale = counts.get(STALE, 0)
        n_a_split = counts.get(TAGGED_PERMISSIVE_ONLY, 0)
        live_block = [
            "",
            f"ported + live  (reachable from a host root)     : {n_live}",
            f"ported, NOT live (inert)                        : {n_inert}",
            f"  of which infra-replaced (`REPLACED-BY:`)      : {n_replaced}",
            f"  wiring worklist (inert, a host is owed)       : {n_inert - n_replaced}",
            "",
            f"PORT tag sites (anchors)                        : {len(pairs)}",
            f"  live (untagged)                               : {n_a_live}",
            f"  inert, `NOT WIRED:` tag present               : {n_a_disclosed}",
            f"  inert, `REPLACED-BY:` (no host is owed)       : {n_a_replaced}",
            f"  inert, no tag  -> disclosure gap              : {n_a_undisclosed}",
            f"  tagged NOT WIRED / REPLACED-BY but live       : {n_stale}",
            f"  tagged, live on the permissive graph only     : {n_a_split}",
        ]
    return "\n".join(
        [
            f"total addresses tracked       : {n}",
            f"dumped                        : {n_dumped}",
            f"documented (in docs/)         : {n_documented}",
            f"ported (// PORT: tag)         : {n_ported}",
            "",
            f"dumped + documented           : {dd}",
            f"dumped + documented + ported  : {ddp}",
            f"dumped + documented, NOT ported                 : {dd_not_p}",
            f"  of which ignored (PsyQ / BIOS / libgte / ...) : {dd_not_p_ignored}",
            f"  remaining port worklist                       : {dd_not_p - dd_not_p_ignored}",
            "",
            f"cited but NOT dumped  (dump worklist)           : {cited_not_dumped}",
            f"ported but NOT documented (provenance gap)      : {ported_not_documented}",
            f"ported but NOT dumped     (provenance gap)      : {ported_not_dumped}",
        ]
        + live_block
    )
