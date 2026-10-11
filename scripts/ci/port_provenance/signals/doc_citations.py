"""Signals `doc-citation` and `dual-label`: a doc row citing a dump that is
not the routine it names, and one address under two names.
"""

from __future__ import annotations

import re
from collections import defaultdict
from pathlib import Path

from port_provenance.citation_support import _unsupported
from port_provenance.dumps import Dump
from port_provenance.findings import Finding
from port_provenance.paths import REPO
from port_provenance.rows import DEFINING_DOC_ROOTS
from port_provenance.tags import CITED_ADDR_RE, CODE_ADDR, MD_LINK_RE


def find_doc_row_citations(by_addr: dict[str, list[Dump]]) -> list[Finding]:
    """A `docs/reference/functions/` row citing an address its routine lacks.

    The function directory is the canonical label for every dumped routine, and
    a wrong label there outlives the tag that copied it - the shop mis-reading
    survived in `functions/menus.md` after the port and the subsystem page were
    both corrected. Each row's leading cell names the routine; every retail
    address in the rest of the row is evidence the row offers for that routine,
    and is checked against its dump the same way a tag line's is.
    """
    docs = REPO / "docs" / "reference" / "functions"
    if not docs.is_dir():
        return []
    row_re = re.compile(r"^\|\s*`([0-9a-fA-F]{8})`\s*\|(.*)$")
    out: list[Finding] = []
    for page in sorted(docs.glob("*.md")):
        rel = str(page.relative_to(REPO))
        for lineno, line in enumerate(page.read_text().splitlines(), start=1):
            m = row_re.match(line)
            if not m:
                continue
            subject = m.group(1).lower()
            dumps = by_addr.get(subject)
            if not dumps:
                continue
            missing = _unsupported(
                m.group(2), {subject}, dumps, set(by_addr), subject
            )
            if not missing:
                continue
            out.append(
                Finding(
                    "doc-citation",
                    f"doc-citation:{rel}:{subject}:{'+'.join(missing)}",
                    subject,
                    f"{rel}:{lineno}",
                    float(len(missing)),
                    [
                        f"row for `{subject.upper()}` cites "
                        f"{', '.join('0x' + x for x in missing)}, absent from every "
                        f"dump of it ({', '.join(d.stem for d in dumps)}).",
                    ],
                )
            )
    return out


def _link_targets(page: Path, line: str) -> set[str]:
    """Repo-relative pages a markdown line links to, `#anchors` stripped."""
    out: set[str] = set()
    for m in MD_LINK_RE.finditer(line):
        target = m.group(1).split("#")[0]
        if not target or target.startswith(("http://", "https://", "mailto:")):
            continue
        try:
            resolved = (page.parent / target).resolve().relative_to(REPO)
        except (OSError, ValueError):
            continue
        out.add(str(resolved))
    return out


def find_dual_labels(by_addr: dict[str, list[Dump]]) -> list[Finding]:
    """One routine given a defining description on two unrelated docs pages.

    A routine gets exactly one canonical description. A `###` section in a
    subsystem page and a row in the function directory are the same claim filed
    twice on purpose; **two subsystem pages under unrelated names** are two
    different claims about the same bytes, and at most one can be right.

    This is the shape that outlived the port fix for `FUN_801D5DE0`. That
    routine's 151 instructions are byte-identical in all three of its dumps, and
    it was described as the world-map tile cursor state machine, as the shop
    stock list and as the casino prize list - on three pages, none of which
    mentioned the others.

    Page relatedness is decided on filename tokens, the same rule the module
    comparison uses, so `field-menu.md` and `menus.md` are one topic and
    `world-map.md` and `menus.md` are two. That rule alone over-fires by a wide
    margin, because the function directory is named for coarse topics
    (`menus`, `script-vms`, `battle`) and the write-ups for fine ones
    (`save-screen`, `field-locomotion`, `battle-action`), so the index entry and
    the page it indexes always read as "unrelated names". Two exclusions cut
    that class without touching the shape the signal exists for:

    - **A site that links to the counterpart page is a pointer, not a rival.**
      A directory row whose own text sends the reader to the page it is said to
      contradict is filing that page's claim, not competing with it. Scoped to
      the pair: a row that points at `save-screen.md` is still an independent
      claim against `world-map.md`. `FUN_801D5DE0`'s carriers cited nobody, so
      it keeps firing.
    - **Only defining pages count** (`DEFINING_DOC_ROOTS`). A thread ledger or a
      tooling page naming an address describes a question or an instrument, not
      the routine.
    """
    docs = REPO / "docs"
    if not docs.is_dir():
        return []
    head_re = re.compile(r"^#{2,4}\s+(.*)$")
    row_re = re.compile(r"^\|\s*`?(?:FUN_)?([0-9a-fA-F]{8})`?\s*\|")
    # {addr: {page: [(site label, pages this site links to)]}}
    sites: dict[str, dict[str, list[tuple[str, set[str]]]]] = defaultdict(
        lambda: defaultdict(list)
    )
    for page in sorted(docs.rglob("*.md")):
        rel = str(page.relative_to(REPO))
        if not rel.startswith(DEFINING_DOC_ROOTS):
            continue
        is_directory_page = "reference/functions/" in rel
        for lineno, line in enumerate(page.read_text().splitlines(), start=1):
            found: set[str] = set()
            m = head_re.match(line)
            if m:
                found |= {
                    x.group(1).lower() for x in CITED_ADDR_RE.finditer(m.group(1))
                } | {
                    x.group(1).lower()
                    for x in re.finditer(rf"`({CODE_ADDR})`", m.group(1))
                }
            elif is_directory_page:
                m = row_re.match(line)
                if m:
                    found.add(m.group(1).lower())
            if not found:
                continue
            links = _link_targets(page, line)
            for a in found:
                if a in by_addr:
                    sites[a][rel].append((f"{rel}:{lineno}", links))

    out: list[Finding] = []
    for addr, per_page in sites.items():
        pages = sorted(per_page)
        if len(pages) < 2:
            continue
        toks = {p: {t for t in re.split(r"[^a-z0-9]+", Path(p).stem.lower())
                    if len(t) >= 3} for p in pages}

        def points_at(src: str, dst: str) -> bool:
            """Every one of `src`'s sites for this address links to `dst`."""
            return all(dst in links for _, links in per_page[src])

        rivals = [
            (a, b)
            for i, a in enumerate(pages)
            for b in pages[i + 1:]
            if not (toks[a] & toks[b])
            and not points_at(a, b)
            and not points_at(b, a)
        ]
        if not rivals:
            continue
        lines = [
            f"FUN_{addr} carries a defining description on {len(pages)} docs "
            f"pages with unrelated names, neither pointing at the other; at "
            f"most one can be right:"
        ]
        for pg in pages:
            lines.append("    " + ", ".join(label for label, _ in per_page[pg]))
        lines.append(
            "    rival pair(s): "
            + "; ".join(f"{a} vs {b}" for a, b in rivals)
        )
        out.append(
            Finding("dual-label", f"dual-label:{addr}", addr,
                    ", ".join(pages), 8.0 + len(pages), lines)
        )
    return out
