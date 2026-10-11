"""Tier 8 analysis: does each host own the engine type, or only read it?

Rows: `ui_host_drift.rows.owned_types`.
"""

import re

from ui_host_drift.roots import HOSTS, REPO
from ui_host_drift.rows.owned_types import (
    OWNED_TYPES,
    SESSION_ALIAS_SRC,
    SESSION_SRC,
    SESSION_TYPE,
)
from ui_host_drift.rust_text import brace_block, is_test_source, strip_comments

# Ownership evidence, in the host's own comment-stripped source: a field
# declaration whose type mentions the name, or a construction of it. A bare
# mention is deliberately NOT ownership - `use` lines, match arms and doc
# links are exactly what the page had for `Camera` while owning nothing.
def owns_type(text: str, name: str) -> bool:
    field = re.search(
        # A field DECLARATION on its own line, and never through a `&`: a
        # borrowed parameter is the shape this must not accept, and a
        # multi-line signature puts one on its own line too.
        rf"^\s*(?:pub(?:\([^)]*\))?\s+)?[a-z_][a-z_0-9]*\s*:\s*[^,;=()&]*\b{name}\b",
        text,
        re.MULTILINE,
    )
    if field:
        return True
    if re.search(rf"\b{name}\s*::\s*(?:new|default|from_\w+|with_\w+)\s*\(", text):
        return True
    # A struct literal is a construction - but `-> Camera {` is a return type
    # and `impl Trait for Camera {` is an impl block, and both put the same
    # two tokens next to each other. The control suite pins all three.
    for hit in re.finditer(rf"\b{name}\s*\{{", text):
        prefix = text[max(0, hit.start() - 40):hit.start()]
        if re.search(r"->\s*$", prefix) or re.search(r"\b(?:impl|struct|enum|for)\b[^\n]*$", prefix):
            continue
        return True
    return False


def session_part_types() -> set[str]:
    """The type names [`SESSION_TYPE`]'s own field declarations mention."""
    if not SESSION_SRC.is_file():
        return set()
    src = strip_comments(SESSION_SRC.read_text(encoding="utf-8"))
    m = re.search(rf"\bpub struct {SESSION_TYPE}\b[^{{;]*\{{", src)
    if not m:
        return set()
    start, end = brace_block(src, m.end() - 1)
    field_types = re.findall(
        r"^\s*(?:pub(?:\([^)]*\))?\s+)?[a-z_][a-z_0-9]*\s*:\s*([^,\n]*)",
        src[start:end],
        re.MULTILINE,
    )
    return set(re.findall(r"\b([A-Z][A-Za-z0-9_]*)\b", " ".join(field_types)))


def session_aliases() -> set[str]:
    """The aliases of the session type the session crate itself declares
    (`pub type NativeSession = BootSession<AudioOut>`)."""
    if not SESSION_ALIAS_SRC.is_file():
        return set()
    src = strip_comments(SESSION_ALIAS_SRC.read_text(encoding="utf-8"))
    return set(
        re.findall(rf"\bpub type\s+(\w+)\s*=\s*[\w:]*\b{SESSION_TYPE}\b", src)
    )


def holds_session(text: str) -> bool:
    """A field or construction of the session type or of an alias the session
    crate declares (the native window's `NativeSession`), or a type alias of
    it declared by the host (the page's `PageSession`)."""
    return (
        owns_type(text, SESSION_TYPE)
        or any(owns_type(text, alias) for alias in session_aliases())
        or bool(re.search(rf"\btype\s+\w+\s*=\s*[\w:]*\b{SESSION_TYPE}\b", text))
    )


def host_shipped_sources() -> dict[str, list[tuple[str, str]]]:
    """Per host label, every shipped (non-test) source as `(rel path, text)`."""
    out: dict[str, list[tuple[str, str]]] = {}
    for host, roots in HOSTS.items():
        rows: list[tuple[str, str]] = []
        for root in roots:
            if not root.is_dir():
                continue
            for path in sorted(root.rglob("*.rs")):
                if is_test_source(path):
                    continue
                rel = path.relative_to(REPO).as_posix()
                rows.append((rel, strip_comments(path.read_text(encoding="utf-8"))))
        out[host] = rows
    return out


def check_owned_types(shipped: dict[str, list[tuple[str, str]]]) -> tuple[list[str], list[str]]:
    """Every [`OWNED_TYPES`] row: both hosts must own the type.

    Returns `(problems, pending)`; a row carrying `blocked_on` reports instead
    of failing, and a `blocked_on` row that has gone clean fails.
    """
    problems: list[str] = []
    pending: list[str] = []
    for row in OWNED_TYPES:
        name = str(row["type"])
        defined_in = REPO / str(row["defined_in"])
        if not defined_in.is_file() or not re.search(
            rf"pub (?:struct|enum) {name}\b", defined_in.read_text(encoding="utf-8")
        ):
            problems.append(
                f"STALE OWNED TYPE {name}: {row['defined_in']} no longer defines it "
                f"(renamed or moved?). Re-point or drop the row."
            )
            continue
        missing: list[str] = []
        via_session = name in session_part_types()
        for host, rows in shipped.items():
            if any(owns_type(text, name) for _rel, text in rows):
                continue
            if via_session and any(holds_session(text) for _rel, text in rows):
                continue
            missing.append(host)
        blocked = row.get("blocked_on")
        if missing and not blocked:
            problems.append(
                f"UNOWNED TYPE {name}: {', '.join(missing)} reaches this type's "
                f"outputs without holding one ({row['why']}). Give that host the "
                f"type, or record why not with `blocked_on`."
            )
        elif missing:
            pending.append(f"{name}: not owned by {', '.join(missing)} - {blocked}")
        elif blocked:
            problems.append(
                f"STALE blocked_on ({name}): both hosts own the type now, so the "
                f"marker describes a gap that is closed. Drop `blocked_on`."
            )
    return problems, pending
