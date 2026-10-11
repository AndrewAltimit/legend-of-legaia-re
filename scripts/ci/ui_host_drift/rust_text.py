"""Source-text helpers every tier shares: comment stripping, test-source
detection, signature and body extraction, whole-word sets.
"""

import re
from pathlib import Path

LINE_COMMENT_RE = re.compile(r"//.*$", re.MULTILINE)


BLOCK_COMMENT_RE = re.compile(r"/\*.*?\*/", re.DOTALL)
USE_STMT_RE = re.compile(r"\buse\s+[^;]*;")


def first_difference(this: str, other: str, window: int = 60) -> str:
    """`this` windowed around where it first departs from `other`.

    A 23-row table printed from its start is unreadable and, worse, useless:
    the two renderings agree for hundreds of characters, so a head-truncated
    dump shows two identical-looking lines and leaves the reader to diff by
    eye. The divergence is the only part worth printing.
    """
    i = 0
    while i < min(len(this), len(other)) and this[i] == other[i]:
        i += 1
    lo = max(0, i - window // 2)
    hi = min(len(this), i + window)
    return ("..." if lo else "") + this[lo:hi] + ("..." if hi < len(this) else "")


def signature_end(text: str, start: int) -> int:
    """Index of the body's `{` for the `fn` at `text[start]`, or -1 if none.

    Scans at bracket depth zero so a `;` inside an array return type
    (`-> [f32; 4] {`) does not read as a bodyless trait declaration, and a
    genuinely bodyless `fn f(..);` does.
    """
    i, n, depth = start, len(text), 0
    while i < n:
        c = text[i]
        if c in "([":
            depth += 1
        elif c in ")]":
            depth -= 1
        elif depth == 0 and c == "{":
            return i
        elif depth == 0 and c == ";":
            return -1
        i += 1
    return -1


# A host "has" a screen when its *shipped* code draws it. Both native roots
# carry `#[cfg(test)]` modules inside `src/` - `engine-render/src/tests/` is a
# whole directory of them - and a builder exercised only by a unit test is
# precisely the not-wired case this gate exists to surface. Counting those
# references made the gate report test coverage as wiring, which let four
# `web_missing` waivers assert "native calls it" about builders no host called.
# The two-directional waiver validation could not catch that: it re-derives the
# bucket from the same over-counted signal.


def is_test_source(path: Path) -> bool:
    """Is this file test-only code rather than shipped host code?

    Deliberately **path-only**: a `tests/` directory component or a `tests.rs`
    file name, which is the split-out `mod tests;` convention every test module
    under these roots follows (`engine-render/src/tests/` is a whole directory
    of them, and it is where all six mis-bucketed references lived).

    Sniffing file *contents* for `#[cfg(test)]` was tried and rejected: plenty
    of shipped files carry an inline test module, so a content rule drops real
    host code from the scan - it excluded `engine-render/src/lib.rs`,
    `engine-render/src/renderer.rs` and `engine-shell/.../window.rs` among
    others. That direction is the dangerous one. Over-counting "used" only
    makes the gate nag about a screen that is in fact wired; under-counting it
    invents ORPHANs and, worse, lets a waiver be written asserting a gap that
    does not exist. The path rule cannot do that, because a file under
    `src/tests/` is never a host draw site.
    """
    return "tests" in path.parts or path.name == "tests.rs"


def strip_comments(text: str) -> str:
    """Drop `//`-style comments.

    Doc comments name sibling builders constantly (`[`shop_draws_for`]`), and
    a mention in prose is not a wiring. Stripping them keeps the checker
    conservative in the safe direction: it under-reports "used", so it can
    nag about a screen that is in fact wired, but it never stays silent about
    one that is not.
    """
    return LINE_COMMENT_RE.sub("", text)


def fn_body(text: str, brace: int) -> str:
    """The source between `text[brace]` and its matching close brace.

    Rust-aware enough to brace-match: comments, string / raw-string / char
    literals are skipped so a `format!("{}", ..)` or a lone brace in a string
    cannot unbalance the scan. Everything else is counted, which is all the
    caller needs - it only greps the result for identifiers.
    """
    n = len(text)
    i, depth = brace, 0
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            nl = text.find("\n", i)
            i = n if nl < 0 else nl
            continue
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            end = text.find("*/", i + 2)
            i = n if end < 0 else end + 2
            continue
        if c == "r" and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            j = i + 1
            while j < n and text[j] == "#":
                j += 1
            if j < n and text[j] == '"':
                term = '"' + "#" * (j - i - 1)
                end = text.find(term, j + 1)
                i = n if end < 0 else end + len(term)
                continue
        if c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            i = j + 1
            continue
        if c == "'":
            m = re.match(r"'(?:\\.|[^\\'])'", text[i:])
            if m:
                i += m.end()
                continue
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return text[brace + 1 : i]
        i += 1
    return text[brace + 1 :]


WORD_RUN = re.compile(r"[0-9A-Za-z_]+")


def word_set(text: str) -> set[str]:
    r"""Every maximal `[0-9A-Za-z_]` run in `text`.

    `re.search(rf"\b{re.escape(n)}\b", text)` is true for an identifier `n`
    exactly when `n` is one of these runs - Python's `\b` is a word/non-word
    transition, so "matches as a whole word" and "is a maximal word run" are
    the same predicate. Asking it this way costs one pass over the text
    instead of one compiled search per candidate name, which is the whole
    difference between this gate costing half a minute of CPU and costing a
    second: the two host trees crossed with the engine-ui name set was 173k
    regex searches, 92% of the run.

    The run class has to be `[0-9A-Za-z_]+` and not the Rust-identifier
    `[A-Za-z_][0-9A-Za-z_]*`. The latter tokenises `1foo` as `foo` and would
    report a match where `\bfoo\b` finds none - a numeric literal silently
    promoted to a call edge, in the direction that hides drift.
    """
    return set(WORD_RUN.findall(text))


BLOCK_COMMENT_ANY_RE = re.compile(r"/\*.*?\*/", re.DOTALL)


def strip_all_comments(text: str) -> str:
    """Drop `/* */` and `//` comments - the same conservative direction as
    tier 1, applied to both languages this tier scans."""
    return LINE_COMMENT_RE.sub("", BLOCK_COMMENT_ANY_RE.sub("", text))


def brace_block(text: str, start: int) -> tuple[int, int]:
    """`(body_start, body_end)` of the brace block opening at/after `start`."""
    open_at = text.index("{", start)
    depth = 0
    for i in range(open_at, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return open_at + 1, i
    raise ValueError("unbalanced braces")
