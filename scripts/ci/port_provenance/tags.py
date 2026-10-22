"""`// PORT:` tags as the sweep reads them: the address grammars, the module
a tag belongs to, and the tag collector.
"""

from __future__ import annotations

import re

import port_tag_reader

from port_provenance.paths import CRATES_DIR, REPO
from port_provenance.rows import SPLIT_MODULES
from port_provenance.thresholds import STOPWORD_TOKENS

# ---------------------------------------------------------------------------
# Regexes
# ---------------------------------------------------------------------------

CODE_ADDR = r"80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4}"

PORT_TAG_RE = re.compile(r"//[/!]?\s*PORT\s*:\s*(.*)", re.IGNORECASE)
PORT_ADDR_RE = re.compile(
    rf"(?:FUN_|overlay_[0-9a-zA-Z_]+?_)({CODE_ADDR})", re.IGNORECASE
)
# Any concrete retail address a tag line cites for itself: bare `0x8...`,
# `FUN_8...`, `DAT_8...`, `_DAT_8...`, backticked. Deliberately wider than
# PORT_ADDR_RE - this one is looking for the port's own evidence, not its claim.
CITED_ADDR_RE = re.compile(r"(?:0x|FUN_|_?DAT_|PTR_)([0-9a-fA-F]{8})", re.IGNORECASE)
# The subset of the above written as a *function entry*: `FUN_8003D53C`,
# `func_0x8003d064`, `LAB_`-style entries. Prose naming another routine is a
# cross-reference, whether or not that routine has been dumped.
FUNC_REF_RE = re.compile(r"(?:\bFUN_|\bfunc_0x|\bLAB_)([0-9a-fA-F]{8})", re.IGNORECASE)

MD_LINK_RE = re.compile(r"\[[^\]]*\]\(([^)\s]+)")


def module_of(rel: str) -> str:
    """The module a source file belongs to, for grouping and finding keys."""
    for stem in SPLIT_MODULES:
        if rel.startswith(stem + "/"):
            return split_parent(stem)
    return rel


def split_parent(stem: str) -> str:
    """The file a `SPLIT_MODULES` row keys its findings on.

    A module split for size keeps `<stem>.rs` beside `<stem>/`. A row naming a
    crate's `src` directory is a module that became a whole crate; its parent
    is the crate root `src/lib.rs`, since no `src.rs` exists beside it.
    """
    if stem.endswith("/src"):
        return stem + "/lib.rs"
    return stem + ".rs"


class Tag:
    """One address claimed by one `// PORT:` line.

    `seg` is the slice of the tag's tail that belongs to **this** address: a tag
    line may claim several routines, each with its own parenthetical, and
    checking every parenthetical against every address on the line manufactured
    findings out of correct prose. `PORT: FUN_801ed308 (cases 6/7), ...,
    FUN_801ee90c (the 0x801EEA50 block), ...` reported four routines as missing
    a block only the fifth ever claimed.
    """

    __slots__ = ("addr", "file", "module", "line", "tail", "seg", "raw", "image")

    def __init__(
        self, addr: str, file: str, line: int, tail: str, seg: str, raw: str,
        image: str | None = None,
    ):
        self.addr = addr
        # `overlay_<label>_<NNNN>` when the claim is written image-qualified
        # (`overlay_cast_curse_0943_801f7624`) - the stem of the extracted
        # image file - and None for a bare `FUN_` claim.
        self.image = image
        self.file = file
        self.module = module_of(file)
        self.line = line
        self.tail = tail
        self.seg = seg
        self.raw = raw


def collect_tags() -> list[Tag]:
    """Every `// PORT:` tag in `crates/`, one record per (address, tag line)."""
    out: list[Tag] = []
    if not CRATES_DIR.exists():
        return out
    for p in sorted(CRATES_DIR.rglob("*.rs")):
        rel = str(p.relative_to(REPO))
        # A tag inside a test file is commentary on coverage, not a port claim -
        # same exclusion port-catalog.py makes when resolving anchors.
        if "/tests/" in rel or "/benches/" in rel or "/examples/" in rel:
            continue
        try:
            text = p.read_text(errors="ignore")
        except OSError:
            continue
        lines = text.splitlines()
        # The shared marker reader takes the marker line plus the continuation
        # lines of a wrapped address list, so an address past a wrap is still a
        # claim this checker weighs against the routine's dump.
        for lineno, kind, tail in port_tag_reader.iter_markers(text):
            if kind != "PORT":
                continue
            hits = list(PORT_ADDR_RE.finditer(tail))
            raw = lines[lineno - 1]
            for i, am in enumerate(hits):
                stop = hits[i + 1].start() if i + 1 < len(hits) else len(tail)
                whole = am.group(0)
                image = (
                    whole[: -(len(am.group(1)) + 1)].lower()
                    if whole.lower().startswith("overlay_")
                    else None
                )
                out.append(
                    Tag(
                        am.group(1).lower(),
                        rel,
                        lineno,
                        tail,
                        tail[am.end():stop],
                        raw,
                        image,
                    )
                )
    return out


def module_tokens(rel: str) -> set[str]:
    """Alphabetic tokens of a Rust file's path, minus the generic ones.

    Two modules "share a name" when these overlap. Used only to decide whether a
    `split-table` finding is worth showing - `shop.rs` vs `prize_exchange.rs`
    share nothing, `menu.rs` vs `menu/window_painters.rs` share plenty.
    """
    stem = rel.split("crates/", 1)[-1]
    toks = {t for t in re.split(r"[^a-z0-9]+", stem.lower()) if len(t) >= 4}
    return toks - STOPWORD_TOKENS
