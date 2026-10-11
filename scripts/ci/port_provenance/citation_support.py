"""Which retail addresses cited in a comment or a doc row no dump of the
routine carries.
"""

from __future__ import annotations

import re

from port_provenance.call_sites import site_relation
from port_provenance.dumps import Dump, extent_of
from port_provenance.rows import SITE_ACCEPTED
from port_provenance.tags import CITED_ADDR_RE, CODE_ADDR, FUNC_REF_RE


def _unsupported(
    text: str,
    claimed: set[str],
    dumps: list[Dump],
    entries: set[str],
    subject: str = "",
) -> list[str]:
    """Cited retail addresses in `text` that no dump of the routine carries.

    "Carries" is read generously on purpose - inside the routine's own extent, a
    formed data address, a `jal` target, or a literal occurrence anywhere in the
    dump file. The last one covers `j`/branch targets, Ghidra's data-reference
    annotations and the globals the register tracker gives up on, and it is what
    keeps the signal at a handful of findings instead of hundreds.

    Citations of **other function entries** are skipped outright. Prose about a
    routine names its callers, its siblings and the dispatcher that reaches it,
    and none of those has to appear in its own bytes; flagging them buried the
    signal under cross-references that were all correct. What is left is the
    claim worth checking - a data global or an interior range the routine is
    said to own and does not touch.

    "Is a function entry" is read off the **citation's written form**, not off
    whether we happen to hold a dump of it. Keying it on the dump corpus was an
    accident of implementation and it inverted the policy: a row naming a
    caller we have dumped passed, and the same row naming a caller we have not
    read as unsupported evidence. Two thirds of this signal's output was that
    shape - `FUN_8003D53C`, `FUN_80021B04`, `FUN_8004998C`, all correctly named
    as somebody else's routine, none of them in the corpus.

    What the dumps cannot answer, `site_relation` answers from the extracted
    images: a citation the subject's own bytes do not carry is still supported
    when the bytes at the cited address reach the subject, or sit in its body.
    That is a **caller-side** citation, and the corpus is structurally blind to
    it - which is why every one of them had to be waived by hand before.
    """
    func_refs = {m.group(1).lower() for m in FUNC_REF_RE.finditer(text)}
    # Every retail address the same row names, unfiltered: a co-cited `jal`
    # site is evidence for its neighbours whether or not it is itself checkable.
    co_cited = {
        int(m.group(1), 16)
        for m in CITED_ADDR_RE.finditer(text)
        if re.fullmatch(CODE_ADDR, m.group(1), re.IGNORECASE)
    }
    subj = int(subject, 16) if subject else 0
    missing: list[str] = []
    for m in CITED_ADDR_RE.finditer(text):
        c = m.group(1).lower()
        if c in claimed or c in missing:
            continue
        if not re.fullmatch(CODE_ADDR, c, re.IGNORECASE) or c in entries:
            continue
        if c in func_refs:
            continue
        ci = int(c, 16)
        ok = False
        for d in dumps:
            lo, hi = extent_of(d)
            if (
                lo <= ci < hi
                or ci in d.formed
                or ci in d.jals
                or c in d.text.lower()
            ):
                ok = True
                break
        if ok:
            continue
        rel = site_relation(ci, subj, co_cited) if subj else None
        if rel:
            SITE_ACCEPTED.append((subject, c, rel))
            continue
        missing.append(c)
    return missing
