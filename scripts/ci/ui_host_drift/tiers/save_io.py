"""Tier 14 analysis: does save-rack I/O reach the card only through the save
screen?

Rows: `ui_host_drift.rows.save_io`.
"""

import re

from ui_host_drift.roots import REPO
from ui_host_drift.rows.save_io import (
    PAGE_CARD_RACK,
    PAGE_CARD_RACK_RULES,
    SAVE_IO_ROUTES,
)
from ui_host_drift.rust_text import (
    fn_body,
    is_test_source,
    signature_end,
    strip_all_comments,
)

SAVE_IO_FN_RE = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*[<(]")
SAVE_IO_TEST_FN_RE = re.compile(r"#\[test\]\s*(?:#\[[^\]]*\]\s*)*fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*[<(]")
# `#[cfg(test)]` and any conjunction that requires it
# (`#[cfg(all(test, not(target_arch = "wasm32")))]`) - never `any(test, ..)`,
# which also builds outside tests.
SAVE_IO_TEST_MOD_RE = re.compile(
    r"#\[cfg\((?:test|all\((?:[^()]|\([^()]*\))*?\btest\b(?:[^()]|\([^()]*\))*\))\)\]\s*"
    r"(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{"
)


def save_io_calls(text: str, primitives: set[str]) -> list[tuple[str, str]]:
    """`(primitive, enclosing fn)` for every shipped call of a primitive.

    A call inside a `#[cfg(test)]` module or a `#[test]` fn is not shipped
    code and is skipped; a call outside any fn reports the enclosing fn as
    `<item>`. A primitive's own definition is not a call.
    """
    text = strip_all_comments(text)
    skip: list[tuple[int, int]] = []
    for m in SAVE_IO_TEST_MOD_RE.finditer(text):
        brace = m.end() - 1
        skip.append((brace, brace + len(fn_body(text, brace))))
    for m in SAVE_IO_TEST_FN_RE.finditer(text):
        brace = signature_end(text, m.start(1) - 3)
        if brace >= 0:
            skip.append((brace, brace + len(fn_body(text, brace))))
    fns: list[tuple[int, int, str]] = []
    for m in SAVE_IO_FN_RE.finditer(text):
        brace = signature_end(text, m.start())
        if brace < 0:
            continue
        fns.append((brace, brace + len(fn_body(text, brace)), m.group(1)))
    out: list[tuple[str, str]] = []
    for prim in sorted(primitives):
        for m in re.finditer(rf"\b{re.escape(prim)}\s*\(", text):
            pos = m.start()
            if re.search(r"\bfn\s+$", text[max(0, pos - 8) : pos]):
                continue
            if any(a <= pos < b for a, b in skip):
                continue
            owner = "<item>"
            best = -1
            for a, b, name in fns:
                if a <= pos < b and a > best:
                    best, owner = a, name
            out.append((prim, owner))
    return out


def check_save_io_routes() -> tuple[list[str], int]:
    """Tier 14. `(problems, calls_checked)`."""
    problems: list[str] = []
    checked = 0
    for route in SAVE_IO_ROUTES:
        root = REPO / route["root"]
        prims: dict[str, set[str]] = route["primitives"]
        seen: set[str] = set()
        for path in sorted(root.rglob("*.rs")):
            if is_test_source(path):
                continue
            for prim, owner in save_io_calls(path.read_text(encoding="utf-8"), set(prims)):
                checked += 1
                seen.add(prim)
                if owner not in prims[prim]:
                    problems.append(
                        f"SAVE BYPASS {route['host']} {path.relative_to(REPO)}: "
                        f"`{owner}` calls `{prim}`, which only "
                        f"{', '.join(sorted(prims[prim]))} may call. A save or load "
                        f"reaches the rack through the retail save screen's commit "
                        f"(`SaveScreenFlow::commit`) and nowhere else - open the "
                        f"screen instead of moving the bytes."
                    )
        for prim in sorted(set(prims) - seen):
            problems.append(
                f"SAVE ROUTE {route['host']}: no shipped call of `{prim}` under "
                f"{route['root']} - the applier was renamed or the save path moved. "
                f"Update SAVE_IO_ROUTES, or the tier checks nothing."
            )
    # The other half of "the screen always has a card": the native window's
    # port 1 is its save directory whatever the player does, so the page's
    # port 1 must start with the browser card in it, and every fresh runtime
    # (the disc load, a trap recovery) must get the rack put back. Without
    # that the retail Save screen opens on two empty ports and does nothing.
    page = REPO / PAGE_CARD_RACK
    if not page.is_file():
        problems.append(f"SAVE ROUTE web: {PAGE_CARD_RACK} is missing")
    else:
        text = page.read_text(encoding="utf-8")
        for label, pattern in PAGE_CARD_RACK_RULES:
            if not re.search(pattern, text):
                problems.append(f"SAVE ROUTE web {PAGE_CARD_RACK}: {label}")
    return problems, checked
