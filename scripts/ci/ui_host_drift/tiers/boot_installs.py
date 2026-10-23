"""Tier 13 analysis: does the page's disc load install every World table the
native boot does?

Rows: `ui_host_drift.rows.boot_installs`.
"""

import re

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    import tomli as tomllib  # type: ignore[no-redef]

from ui_host_drift.roots import REPO, WAIVERS
from ui_host_drift.rows.boot_installs import BOOT_NATIVE, BOOT_WEB_ROOTS
from ui_host_drift.rust_text import (
    brace_block,
    is_test_source,
    signature_end,
    strip_all_comments,
)

BOOT_INSTALL_RE = re.compile(r"\bworld\s*\.\s*((?:install|set)_[a-z0-9_]+)\s*\(")


def boot_install_calls(text: str) -> set[str]:
    return set(BOOT_INSTALL_RE.findall(strip_all_comments(text)))


def session_from_host_installs(boot_src: str) -> set[str]:
    """The installs inside `BootSession::from_host`'s body - the ones a host
    that builds its session through `from_host` runs without a call of its
    own."""
    m = re.search(r"\bpub fn from_host\s*\(", boot_src)
    if not m:
        return set()
    brace = signature_end(boot_src, m.start())
    if brace < 0:
        return set()
    start, end = brace_block(boot_src, brace)
    return boot_install_calls(boot_src[start:end])


def check_boot_installs() -> tuple[list[str], list[str], int]:
    """Tier 13. `(problems, waived_notes, native_installs_checked)`."""
    if not BOOT_NATIVE.is_file():
        return [f"boot installs: {BOOT_NATIVE.relative_to(REPO)} missing"], [], 0
    # The session module is `session.rs` plus its `session/` children; an install in
    # any of them is the native boot's.
    native_src = "\n".join(
        p.read_text(encoding="utf-8")
        for p in [BOOT_NATIVE, *sorted(BOOT_NATIVE.with_suffix("").glob("*.rs"))]
    )
    native = boot_install_calls(native_src)
    web: set[str] = set()
    page_builds_session = False
    for path in sorted(p for root in BOOT_WEB_ROOTS for p in root.rglob("*.rs")):
        if is_test_source(path):
            continue
        text = strip_all_comments(path.read_text(encoding="utf-8"))
        web |= set(re.findall(r"\.\s*((?:install|set)_[a-z0-9_]+)\s*\(", text))
        page_builds_session |= bool(re.search(r"\bfrom_host\s*\(", text))
    if page_builds_session:
        web |= session_from_host_installs(native_src)
    waivers: dict[str, dict] = {}
    if WAIVERS.is_file():
        for row in tomllib.loads(WAIVERS.read_text(encoding="utf-8")).get("boot_install", []):
            waivers[str(row.get("method", ""))] = row
    problems: list[str] = []
    notes: list[str] = []
    for m in sorted(native):
        if m in web:
            if m in waivers:
                problems.append(
                    f"STALE BOOT-INSTALL WAIVER {m}: the page calls it now. Drop the waiver."
                )
            continue
        if m in waivers:
            if not str(waivers[m].get("reason", "")).strip():
                problems.append(f"BOOT-INSTALL WAIVER {m}: needs a non-empty `reason`.")
            notes.append(m)
            continue
        problems.append(
            f"BOOT DRIFT world.{m}: the native boot ({BOOT_NATIVE.relative_to(REPO)}) "
            f"installs it and no shipped crates/web-play source calls it. Install it "
            f"in the page's load_disc (better: fold it into one engine install both "
            f"boots call), or add a [[boot_install]] waiver with a reason."
        )
    for m in sorted(waivers):
        if m not in native:
            problems.append(
                f"STALE BOOT-INSTALL WAIVER {m}: the native boot no longer calls it. "
                f"Drop the waiver."
            )
    return problems, notes, len(native)
