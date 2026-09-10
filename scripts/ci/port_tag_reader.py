"""One reader for `// PORT:` / `// REF:` marker blocks in Rust sources.

Three tools read these markers - `port-catalog.py` (the status catalog),
`check-port-tags.py` (citation drift) and `check-port-provenance.py` (does the
address name the right routine) - and each grew its own scraper. All three read
the addresses off the marker's **opening line** only, so a tag whose address
list wraps loses every address past the wrap:

    /// PORT: FUN_801d6704, FUN_801cf00c,
    ///       FUN_801cef54

`port-catalog.py` credits the first two and reports the third as an unported
worklist row; `check-port-tags.py` then reads the third line as an untagged
citation. Authors work around it by repeating the marker on the next line
(`//! REF: ...` twice in `battle_tutorial.rs`), which is the shape this reader
exists to make unnecessary.

## What counts as a continuation

Only a continuation **of the address list**. A marker line is routinely
followed by prose that names other routines - a `REF:` line, a "same 24
instructions as `FUN_801D6710`" sentence, a wiring note - and reading those as
port claims would manufacture a claim the author did not make. Measured over
this repo's `crates/`, the loose rule ("every comment line up to the first
blank one") pulls 47 further addresses into the ported set, and spot-checking
them found prose and `REF:` lines, not wrapped lists.

So a following comment line continues the list only when **both** hold:

  * the text accumulated so far ends with a list separator (`,` or `;`), and
  * the following line, once its comment leader is stripped, **starts** with an
    address token (optionally after a leading separator or `and`).

Anything else ends the block. That is exactly the wrapped-list shape and
nothing else, which is why it adds no address to this tree today: the defect is
real but currently unexercised, and the reader is what keeps it that way.
"""

from __future__ import annotations

import re

# `// PORT:` / `/// PORT:` / `//! PORT:` and the same three for `REF:`.
MARKER_RE = re.compile(r"//[/!]?\s*(PORT|REF)\s*:\s*(.*)", re.IGNORECASE)

# The two token shapes that count as naming a function: Ghidra's own
# `FUN_<addr>`, and a `funcs/` dump stem `overlay_<label>_<addr>` (tags cite the
# stem where the bare VA is aliased across overlays). Bare hex does NOT count -
# tags name data globals and interior ranges in their prose, and matching those
# put phantom "ported but not dumped" rows in the catalog.
ADDR_RE = re.compile(
    r"(?:FUN_|overlay_[0-9a-zA-Z_]+?_)(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})",
    re.IGNORECASE,
)

# Leading `//` / `///` / `//!` plus one optional space.
COMMENT_LEAD_RE = re.compile(r"^\s*//[/!]?\s?")

# A continuation line of the address LIST: starts with an address token, with
# at most a leading separator or `and` in front of it.
LIST_CONT_RE = re.compile(
    r"^(?:[,;]\s*|and\s+)?(?:FUN_|overlay_[0-9a-zA-Z_]+?_)80", re.IGNORECASE
)

# A list that has not ended yet.
_OPEN_LIST_RE = re.compile(r"[,;]\s*$")


def marker_tail(lines: list[str], i: int) -> tuple[str, str, int] | None:
    """The marker at `lines[i]`, as `(kind, tail, next_index)`.

    `kind` is `PORT` or `REF` upper-cased; `tail` is the marker's text with
    every wrapped-list continuation line appended; `next_index` is the first
    line the block does not cover. Returns `None` when `lines[i]` carries no
    marker.
    """
    m = MARKER_RE.search(lines[i])
    if not m:
        return None
    kind = m.group(1).upper()
    parts = [m.group(2)]
    j = i + 1
    while j < len(lines):
        line = lines[j]
        if not line.lstrip().startswith("//"):
            break
        if MARKER_RE.search(line):
            break
        text = COMMENT_LEAD_RE.sub("", line).strip()
        if not text:
            break
        if not _OPEN_LIST_RE.search(parts[-1].rstrip()):
            break
        if not LIST_CONT_RE.match(text):
            break
        parts.append(text)
        j += 1
    return kind, " ".join(p.strip() for p in parts).strip(), j


def iter_markers(text: str):
    """Yield `(lineno, kind, tail)` for every marker block in `text`.

    `lineno` is 1-based and names the marker's own line, so a caller reporting
    a site points at the line an author wrote the tag on.
    """
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        got = marker_tail(lines, i)
        if got is None:
            i += 1
            continue
        kind, tail, nxt = got
        yield i + 1, kind, tail
        i = max(nxt, i + 1)


def addresses(tail: str) -> set[str]:
    """Lower-cased function addresses named in a marker tail."""
    return {m.group(1).lower() for m in ADDR_RE.finditer(tail)}


# ---------------------------------------------------------------------------
# Selftest. Run `python3 scripts/ci/port_tag_reader.py` (also driven by
# `port-catalog.py --selftest`).
# ---------------------------------------------------------------------------

_CASES: list[tuple[str, str, set[str]]] = [
    (
        "a wrapped list keeps every member",
        "/// PORT: FUN_801d6704, FUN_801cf00c,\n"
        "///       FUN_801cef54\npub fn a() {}\n",
        {"801d6704", "801cf00c", "801cef54"},
    ),
    (
        "a wrap with the separator on the continuation still counts",
        "// PORT: FUN_801d6704,\n// , FUN_801cf00c\n",
        {"801d6704", "801cf00c"},
    ),
    (
        "prose after the marker is not a continuation",
        "/// PORT: FUN_801d14b0\n"
        "/// The Baka overlay links the same body at FUN_801d6710.\n",
        {"801d14b0"},
    ),
    (
        "a following REF: line is its own marker, not a continuation",
        "//! PORT: FUN_801f6b70, FUN_801f747c\n"
        "//! REF: FUN_801f7628, FUN_8003cba8\n",
        {"801f6b70", "801f747c"},
    ),
    (
        "a dangling comma in prose does not open a list",
        "/// PORT: FUN_801d030c - party info panel (LV at `(+0x70, +2)`,\n"
        "/// HP under it) - see FUN_801d2094 for the list form.\n",
        {"801d030c"},
    ),
    (
        "a blank comment line ends the block",
        "/// PORT: FUN_801d6704,\n///\n///       FUN_801cf00c\n",
        {"801d6704"},
    ),
    (
        "bare hex in the tail is not a port claim",
        "/// PORT: FUN_801dd35c (the `_DAT_801F0204 = N` writes, 0x801da1b8)\n",
        {"801dd35c"},
    ),
    (
        "a dump stem counts as naming its function",
        "/// PORT: overlay_battle_action_801e752c\n",
        {"801e752c"},
    ),
]


def selftest() -> int:
    """Return the number of failures; prints one line per failure."""
    bad = 0
    for name, src, want in _CASES:
        got: set[str] = set()
        for _lineno, kind, tail in iter_markers(src):
            if kind == "PORT":
                got |= addresses(tail)
        if got != want:
            bad += 1
            print(f"port_tag_reader selftest FAIL: {name}")
            print(f"  want {sorted(want)}")
            print(f"  got  {sorted(got)}")
    # The REF side of the same reader.
    refs: set[str] = set()
    for _lineno, kind, tail in iter_markers(
        "//! REF: FUN_801f7628, FUN_8003cba8,\n//! FUN_80035f04\n"
    ):
        if kind == "REF":
            refs |= addresses(tail)
    if refs != {"801f7628", "8003cba8", "80035f04"}:
        bad += 1
        print("port_tag_reader selftest FAIL: REF list wraps too")
        print(f"  got {sorted(refs)}")
    if not bad:
        print(f"port_tag_reader selftest: {len(_CASES) + 1} case(s) ok")
    return bad


if __name__ == "__main__":
    raise SystemExit(1 if selftest() else 0)
