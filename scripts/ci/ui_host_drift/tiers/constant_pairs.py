"""Tier 2 analysis: do paired host constants carry equal values?

Rows: `ui_host_drift.rows.constant_pairs`.
"""

import re

from ui_host_drift.roots import REPO
from ui_host_drift.rows.constant_pairs import CONSTANT_PAIRS
from ui_host_drift.rust_text import (
    BLOCK_COMMENT_RE,
    LINE_COMMENT_RE,
    USE_STMT_RE,
    first_difference,
)

# Where this tier's rows are declared, as its own messages cite it.
ROWS_FILE = "scripts/ci/ui_host_drift/rows/constant_pairs.py"

CONST_DECL_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+{}\b", re.MULTILINE)


def const_initialiser(text: str, name: str) -> str | None:
    """The source of `const NAME ... = <here>;`, or None if not declared.

    Scans for the terminating `;` at nesting depth zero rather than taking the
    line, because these initialisers are multi-line array and block
    expressions. String and char literals are skipped so a `;` inside one
    cannot end the scan early.
    """
    m = CONST_DECL_RE.pattern.format(re.escape(name))
    decl = re.search(m, text, re.MULTILINE)
    if not decl:
        return None
    eq = text.find("=", decl.end())
    if eq < 0:
        return None
    i, depth, n = eq + 1, 0, len(text)
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
        if c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            i = j + 1
            continue
        if c == "'":
            lit = re.match(r"'(?:\\.|[^\\'])'", text[i:])
            if lit:
                i += lit.end()
                continue
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == ";" and depth == 0:
            return text[eq + 1 : i]
        i += 1
    return None


def normalise_value(src: str) -> str:
    """Reduce an initialiser to its value tokens.

    Drops comments, drops `use ...;` (both tables open with an alias import
    whose spelling is a host's own business), drops trailing commas before a
    closer, and collapses whitespace - so formatting, rustfmt directives and
    import aliasing cannot read as a value change, while a single changed
    digit always does.
    """
    src = BLOCK_COMMENT_RE.sub(" ", src)
    src = LINE_COMMENT_RE.sub(" ", src)
    src = USE_STMT_RE.sub(" ", src)
    src = re.sub(r",\s*(?=[)\]}])", "", src)
    src = re.sub(r"\s+", "", src)
    return src


def check_constant_pairs() -> list[str]:
    """Compare every [`CONSTANT_PAIRS`] entry; return one message per problem."""
    problems: list[str] = []
    for pair in CONSTANT_PAIRS:
        values: dict[str, str] = {}
        missing = False
        for host in ("native", "web"):
            rel, name = pair[host]  # type: ignore[index]
            path = REPO / rel
            if not path.is_file():
                problems.append(f"CONSTANT {name}: host source {rel} is missing.")
                missing = True
                continue
            raw = const_initialiser(path.read_text(encoding="utf-8"), name)
            if raw is None:
                problems.append(
                    f"CONSTANT {name}: no `const {name}` in {rel}. Renamed or "
                    f"deleted? Update CONSTANT_PAIRS in "
                    f"{ROWS_FILE} - an unresolvable pair checks nothing."
                )
                missing = True
                continue
            values[host] = normalise_value(raw)
        if missing or len(values) != 2:
            continue
        if values["native"] != values["web"]:
            nrel, nname = pair["native"]  # type: ignore[index]
            wrel, wname = pair["web"]  # type: ignore[index]
            nat, web = values["native"], values["web"]
            problems.append(
                f"CONSTANT DRIFT {nname} != {wname} ({pair['what']}):\n"
                f"      native {nrel}:\n        {first_difference(nat, web)}\n"
                f"      web    {wrel}:\n        {first_difference(web, nat)}\n"
                f"      Both hosts feed these to the same engine-ui builder, so "
                f"the screen now renders differently on the two hosts. Make them "
                f"agree, or move the value into a crate both hosts depend on."
            )
    return problems
