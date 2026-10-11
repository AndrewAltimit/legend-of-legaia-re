"""Tier 10 analysis: is a World phase armed only from a debug hotkey?

Rows: `ui_host_drift.rows.entry_symmetry`.
"""

import re

from ui_host_drift.roots import REPO
from ui_host_drift.rows.entry_symmetry import (
    HOTKEY_ONLY_BLOCKED,
    HOTKEY_ONLY_WAIVERS,
    HOTKEY_SOURCES,
    SHARED_CALLER_ROOTS,
)
from ui_host_drift.rust_text import BLOCK_COMMENT_RE, is_test_source, strip_comments


def check_entry_symmetry() -> tuple[list[str], list[str], int]:
    """Tier 10 over the hotkey sources. `(problems, pending, methods_scanned)`."""
    problems: list[str] = []
    pending: list[str] = []
    hot_parts: list[str] = []
    for rel in HOTKEY_SOURCES:
        path = REPO / rel
        if not path.is_file():
            problems.append(
                f"STALE HOTKEY SOURCE {rel}: no such file (moved?). Re-point the row."
            )
            continue
        hot_parts.append(strip_comments(BLOCK_COMMENT_RE.sub("", path.read_text(encoding="utf-8"))))
    hot = "\n".join(hot_parts)
    methods = sorted(set(re.findall(r"\bworld\s*\.\s*([a-z_][a-z_0-9]*)\s*\(", hot)))

    shared_parts: list[str] = []
    for root in SHARED_CALLER_ROOTS:
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.rs")):
            if is_test_source(path):
                continue
            shared_parts.append(
                strip_comments(BLOCK_COMMENT_RE.sub("", path.read_text(encoding="utf-8")))
            )
    shared = "\n".join(shared_parts)

    hotkey_only: list[str] = []
    for name in methods:
        # A CALL, not a definition: `.name(` or `::name(`. Matching a bare
        # `name(` would count the method's own `pub fn` as its caller, which
        # makes every row satisfied and the tier vacuous.
        if not re.search(rf"[.:]\s*{name}\s*\(", shared):
            hotkey_only.append(name)

    declared = {**HOTKEY_ONLY_WAIVERS, **HOTKEY_ONLY_BLOCKED}
    for name in hotkey_only:
        reason = declared.get(name)
        if reason is None:
            problems.append(
                f"HOTKEY-ONLY PHASE world.{name}(): the native window's key arms are "
                f"its only caller outside tests - no shared-engine or browser-host "
                f"call site exists, so a player reaches this on neither host. Arm it "
                f"where the entry is shared, or waive it in "
                f"`HOTKEY_ONLY_WAIVERS` with a reason about the ARM."
            )
        elif name in HOTKEY_ONLY_BLOCKED:
            pending.append(f"BLOCKED world.{name}() - {reason}")
        else:
            pending.append(f"waived world.{name}() - {reason}")
    for name, reason in sorted(declared.items()):
        if name not in methods:
            problems.append(
                f"STALE HOTKEY WAIVER world.{name}(): the hotkey arms no longer call "
                f"it. Drop the waiver."
            )
        elif name not in hotkey_only:
            problems.append(
                f"STALE HOTKEY WAIVER world.{name}(): a shared caller exists now, so "
                f"this is not a hotkey-only phase. Drop the waiver."
            )
        if not reason.strip():
            problems.append(f"HOTKEY WAIVER world.{name}(): needs a non-empty reason.")
    return problems, pending, len(methods)
