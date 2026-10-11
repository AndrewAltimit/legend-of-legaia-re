"""Signal `absent-citation`: a tag citing an address its own dump never
references.
"""

from __future__ import annotations

from port_provenance.citation_support import _unsupported
from port_provenance.dumps import Dump
from port_provenance.findings import Finding
from port_provenance.tags import PORT_ADDR_RE, Tag


def find_absent_citations(
    tags: list[Tag], by_addr: dict[str, list[Dump]]
) -> list[Finding]:
    """A tag line's own cited address that appears nowhere in the dump.

    Scoped to the tag line, not the surrounding rustdoc: a doc block routinely
    and correctly names *other* routines ("the shop's own builder FUN_80030628"),
    and flagging those would drown the signal. The parenthetical on a `PORT:`
    line is different - it is the evidence the port offers for itself.
    """
    out: list[Finding] = []
    for t in tags:
        dumps = by_addr.get(t.addr)
        if not dumps:
            continue
        claimed = {t.addr}
        claimed |= {m.group(1).lower() for m in PORT_ADDR_RE.finditer(t.tail)}
        missing = _unsupported(t.seg, claimed, dumps, set(by_addr), t.addr)
        if missing:
            lines = [
                f"tag cites {', '.join('0x' + x for x in missing)}, absent from "
                f"every dump of FUN_{t.addr} "
                f"({', '.join(d.stem for d in dumps)}).",
                f"    {t.raw.strip()[:200]}",
            ]
            out.append(
                Finding(
                    "absent-citation",
                    f"absent-citation:{t.module}:{t.addr}:{'+'.join(missing)}",
                    t.addr,
                    f"{t.file}:{t.line}",
                    float(len(missing)),
                    lines,
                )
            )
    return out
