"""The disclosure grammar: `NOT WIRED`, `WIRED:` and `REPLACED-BY:` markers on
an item or on a module.
"""

import re

# The disclosure marker is written in caps by convention (`NOT WIRED:` /
# `**NOT WIRED**`). Matching case-insensitively pulls in ordinary prose - one
# module doc says "Consumers (not wired here):" while describing retail, which
# is a statement about the *game*, not a disclosure about the port.
NOT_WIRED_RE = re.compile(r"NOT\s+WIRED")
# Same marker with a backtick or a quote immediately on each side. Prose that
# *names* the marker ("this correct `NOT WIRED:` read as stale", "the audit's
# \"tagged NOT WIRED but live\" row") is a disclosure again as far as the
# unanchored search above is concerned - which is how a port that was wired and
# then documented as wired went on reporting itself inert.
#
# This does NOT change the classification. The spellings a real disclosure is
# written in here are far looser than the module-blanket ones - `// NOT WIRED:`,
# `//! ## NOT WIRED`, `// NOT WIRED, AND UNWIREABLE`, `/// NOT WIRED. "No
# caller" would be...` - so tightening the search would silently un-disclose
# real inert ports, and 75 of the 240 matching comment lines in the tree are
# mentions of one kind or another. It is a DIAGNOSTIC: when every occurrence in
# an anchor's block sits inside backticks or quotes, the stale-tag row says so,
# because the fix is in the prose (rewrite, never quote the marker) and not in
# the wiring.
_COMMENT_LEADER_RE = re.compile(r"^[ \t]*//[/!]?[ \t]?", re.MULTILINE)
# A backtick span may NOT cross a newline and a quoted one may. The asymmetry
# is the corpus's: an inline code span is short (`NOT WIRED:`) while a quoted
# sentence wraps, and letting backticks wrap mis-pairs them across a whole
# module doc - which reported `gameover_banner.rs`'s genuine `# NOT WIRED`
# heading as a quotation. Both are length-capped for the same reason.
_QUOTED_SPAN_RE = re.compile(r"`[^`\n]{0,200}`|\"[^\"]{0,200}?\"", re.DOTALL)


def _not_wired_all_quoted(block: str) -> bool:
    """Every `NOT WIRED` in `block` sits inside backticks or double quotes.

    The comment leaders come off first, because a quoted phrase in a doc block
    wraps across lines and a span scan run over the raw comment would see the
    leader inside the quote.
    """
    prose = _COMMENT_LEADER_RE.sub("", block)
    hits = list(NOT_WIRED_RE.finditer(prose))
    if not hits:
        return False
    spans = [(m.start(), m.end()) for m in _QUOTED_SPAN_RE.finditer(prose)]
    return all(
        any(lo <= h.start() and h.end() <= hi for lo, hi in spans) for h in hits
    )
# A module-wide disclosure has to *open* a doc line to count. Some module docs
# mention the marker while describing where the per-item notes live ("Individual
# items carry a `NOT WIRED:` note"), and reading that as a blanket disclosure
# would suppress every real gap in the file.
# The leading run allows the three spellings a module disclosure is written
# in: `//! NOT WIRED:`, `//! **NOT WIRED**` and the markdown-heading form
# `//! # NOT WIRED`. Only `#`, `*` and space may precede it, so a sentence
# that merely mentions the marker still does not count as a blanket
# disclosure.
MODULE_NOT_WIRED_RE = re.compile(r"^\s*//!\s*[#*\s]*NOT\s+WIRED", re.MULTILINE)
# Per-item wired marker: `WIRED:` opening a doc line. The precise counterpart
# of `MODULE_NOT_WIRED_RE` on the other side of the ledger - an item whose own
# doc block carries it opts out of a `//! NOT WIRED` module blanket. Anchoring
# right after the comment leader (plus the same `#` / `*` markdown allowance)
# is what keeps `NOT WIRED:` lines from matching: their `NOT ` sits where this
# pattern requires `WIRED` to start. Deliberately caps-only, like the
# disclosure marker, so prose ("wired: see below") does not count.
WIRED_ITEM_RE = re.compile(r"^\s*//[/!]?\s*[#*\s]*WIRED\s*:", re.MULTILINE)
# Third class, beside `live` and inert-with-`NOT WIRED:`. `REPLACED-BY: <what>`
# says the routine's *job* is performed in the port by a different, named
# mechanism, so no host will ever call this port and counting it as "not yet
# wired" states a gap that does not exist. See
# `docs/tooling/port-catalog.md#replaced-by` for what may carry the tag.
#
# Two deliberate strictnesses, both learned from the disclosure markers above:
#
#  * it must OPEN a comment line (same anchoring as `WIRED_ITEM_RE`), so prose
#    that merely says a thing was replaced by another thing is not a class
#    change - `ambient_motion.rs` had a bullet whose text opened `NOT WIRED:`
#    while describing retail *opcodes*, and that one sentence blanket-disclosed
#    a live module;
#  * the mechanism text is captured and must be NON-EMPTY. A bare
#    `REPLACED-BY:` claims an exemption while naming nothing, so it does not
#    count and the anchor stays in whichever class it was already in. The
#    captured text is what `--live-audit` prints beside the row, which is what
#    makes the claim auditable at all.
REPLACED_BY_RE = re.compile(r"^\s*//[/!]?\s*[#*\s]*REPLACED-BY\s*:(.*)", re.MULTILINE)
MODULE_REPLACED_BY_RE = re.compile(
    r"^\s*//!\s*[#*\s]*REPLACED-BY\s*:(.*)", re.MULTILINE
)


# Leading `//` / `///` / `//!` plus one optional space, so a continuation line
# can be reduced to its prose.
COMMENT_LEAD_RE = re.compile(r"^\s*//[/!]?\s?")


def _replaced_by_text(block: str, pattern: re.Pattern = None) -> str:
    """The mechanism a `REPLACED-BY:` block names, or `""` when it names none.

    Reads the marker line **and its continuation lines** - every comment line
    up to the first blank one - because the mechanism is a sentence and a
    sentence wraps. Taking only the marker line put half-clauses in the audit
    table ("`crate::scene::ProtIndex` - same replacement as every"), which is
    exactly the row a reader cannot argue with.
    """
    m = (pattern or REPLACED_BY_RE).search(block)
    if not m:
        return ""
    parts = [m.group(1).strip()]
    for line in block[m.start() :].splitlines()[1:]:
        if not line.lstrip().startswith("//"):
            break
        text = COMMENT_LEAD_RE.sub("", line).strip()
        if not text:
            break
        parts.append(text)
    return " ".join(p for p in parts if p).strip()
