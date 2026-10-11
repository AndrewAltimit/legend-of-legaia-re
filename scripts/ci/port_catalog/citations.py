"""The address grammars, and the four collectors that read them: dumped
functions, dump-to-dump citations, doc citations and `// PORT:` tags.
"""

import re
from collections import Counter, defaultdict

import port_tag_reader

from port_catalog import paths
from port_catalog.paths import CRATES_DIR, DOCS_DIR, REPO

# Address ranges that correspond to executable code:
#   SCUS_942.54   : 0x80010000 - 0x8006FFFF
#   Overlays      : 0x801C0000 - 0x8020FFFF
# Match the same shape function-coverage.py uses so the two tools share a worldview.
# IGNORECASE so PORT tags written `FUN_801DD35C` (uppercase) match as cleanly
# as the lowercase form Ghidra emits.
CODE_ADDR_RE = re.compile(r"80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4}", re.IGNORECASE)

# Addresses scraped from a `// PORT:` tag's tail. Only two token shapes count
# as a port claim:
#
#   FUN_<addr>                      -- the canonical Ghidra function name
#   overlay_<label>_<addr>          -- a funcs/ dump-file stem for an
#                                      overlay-resident function (some tags
#                                      cite the dump stem when the bare VA is
#                                      aliased across overlays)
#
# Bare hex in the tag's prose does NOT count: tags routinely name data globals
# (``PORT: the `_DAT_801F0204 = N` writes in FUN_801DD35C``) and interior
# address *ranges* (``PORT: FUN_801D9E1C (rate shifts, 0x801da1b8..0x801da200)``),
# and a loose match turned those into phantom "ported but not dumped"
# provenance-gap rows - range endpoints and data globals have no function dump.
# The function named on the same line (`FUN_...`) is still picked up, so the
# real port credit is unaffected.
PORT_ADDR_RE = re.compile(
    r"(?:FUN_|overlay_[0-9a-zA-Z_]+?_)(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})",
    re.IGNORECASE,
)

# Citations of an address by some other piece of text. Covers Ghidra's auto-named
# call forms plus raw disassembly forms. Matches `function-coverage.py` with one
# addition: a negative lookbehind on `PTR_` so we don't false-positive on
# `PTR_FUN_<addr>` symbols, which Ghidra emits for data-pointer tables that
# happen to contain function pointers - the address there is a *table base*,
# not a function entry. Same negative lookbehind on `_DAT_` for completeness.
CITATION_RE = re.compile(
    r"(?<!PTR_)(?<!_DAT_)(?:FUN_|func_0x|jal\s+0x|jalr\s+\w+,0x|->\s*func_0x)"
    r"(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})"
)

# Doc-side citation. More permissive: doc text uses `FUN_8003EBE4` (caps),
# bare `0x8003e4e8` forms, and backtick-wrapped bare addresses like
# `` `80056678` `` (the shape `docs/reference/functions.md` uses in table
# rows). The backtick form requires an OPENING backtick so we don't false-
# positive on every hex literal in the SCUS range that happens to occur in
# tables or paragraphs.
DOC_CITATION_RE = re.compile(
    r"(?:FUN_|0x|`)(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})",
    re.IGNORECASE,
)

# // PORT: FUN_801dd35c [, FUN_xxx]*  --  the only signal we trust for "ported".
# Accepts plain `//`, doc `//!`, and outer-doc `///` so the tag can live inside
# a rustdoc block (where the provenance is co-located with the human-readable
# description) or as a standalone comment.
PORT_TAG_RE = re.compile(
    r"//[/!]?\s*PORT\s*:\s*(.*)",
    re.IGNORECASE,
)


def collect_dumped() -> dict[str, str]:
    """Return {addr: dump_filename_stem} for every dump under ghidra/scripts/funcs/.

    Band-filtered to `CODE_ADDR_RE`, which is what makes this symmetric with the
    citation regexes. Without the filter the stem match admits the deliberately
    named `data_<addr>_DAT_<addr>_*.txt` data-region dumps, whose addresses sit
    outside the code band (`0x8007xxxx`) and therefore can never be matched by
    `DOC_CITATION_RE`. Those rows then read as "dumped but never documented" in
    perpetuity - unclosable by construction, because the address is a data table
    rather than a function entry and no amount of prose about it can close the
    row. `DAT_8007326c` (the TMD per-mode descriptor table) is the worked
    example: cited from `docs/formats/tmd.md` and from `CLAUDE.md`, yet it sat
    in the undocumented worklist.
    """
    out: dict[str, str] = {}
    if not paths.FUNCS_DIR.exists():
        return out
    for p in paths.FUNCS_DIR.glob("*.txt"):
        m = re.search(r"([0-9a-fA-F]{8})\.txt$", p.name)
        if m:
            addr = m.group(1).lower()
            if not CODE_ADDR_RE.fullmatch(addr):
                continue
            # Prefer overlay dumps over SCUS-named bare dumps if both exist.
            stem = p.stem
            if addr not in out or "overlay_" in stem:
                out[addr] = stem
    return out


def collect_citations() -> tuple[Counter, dict[str, list[str]]]:
    """Walk every dump and count citations of every code-range address.

    Returns (refs counter, sources map). `sources[addr]` is the list of dump
    filename stems that mention `addr` - useful for traceability columns.
    """
    refs: Counter = Counter()
    sources: dict[str, list[str]] = defaultdict(list)
    if not paths.FUNCS_DIR.exists():
        return refs, sources
    for p in sorted(paths.FUNCS_DIR.glob("*.txt")):
        if (
            p.name.endswith("_unique_index.txt")
            or p.name.endswith("_index.txt")
            or p.name.endswith("_survey.txt")
        ):
            continue
        try:
            text = p.read_text(errors="ignore")
        except (PermissionError, OSError):
            continue
        seen: set[str] = set()
        for m in CITATION_RE.finditer(text):
            a = m.group(1).lower()
            if a in seen:
                continue
            seen.add(a)
            refs[a] += 1
            sources[a].append(p.stem)
    return refs, sources


def collect_doc_citations() -> dict[str, set[str]]:
    """Return {addr: set(doc_paths)} for every code-range address cited in docs/."""
    out: dict[str, set[str]] = defaultdict(set)
    if not DOCS_DIR.exists():
        return out
    for p in DOCS_DIR.rglob("*.md"):
        try:
            text = p.read_text(errors="ignore")
        except (PermissionError, OSError):
            continue
        seen: set[str] = set()
        for m in DOC_CITATION_RE.finditer(text):
            a = m.group(1).lower()
            if a in seen:
                continue
            seen.add(a)
            rel = str(p.relative_to(REPO))
            out[a].add(rel)
    return out


def collect_ports() -> dict[str, set[str]]:
    """Return {addr: set(crate_names)} for every Rust `// PORT: FUN_<addr>` tag.

    Addresses come from `port_tag_reader`, the one reader the three tag-reading
    gates share. It takes the marker line **plus** the continuation lines of a
    wrapped address list, which a per-line scan drops - see that module for why
    only a wrapped *list* counts and prose after the marker does not.
    """
    out: dict[str, set[str]] = defaultdict(set)
    if not CRATES_DIR.exists():
        return out
    for p in CRATES_DIR.rglob("*.rs"):
        try:
            text = p.read_text(errors="ignore")
        except (PermissionError, OSError):
            continue
        # Crate name is the first path segment under crates/.
        try:
            rel = p.relative_to(CRATES_DIR)
        except ValueError:
            continue
        crate = rel.parts[0] if rel.parts else "?"
        for _lineno, kind, tail in port_tag_reader.iter_markers(text):
            if kind != "PORT":
                continue
            # Inside the tail of the tag, pick up every code-range *function*
            # address - excluding `_DAT_` / `PTR_` data globals the prose names.
            for addr in port_tag_reader.addresses(tail):
                out[addr].add(crate)
    return out
