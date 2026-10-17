"""Tier 15 analysis: does an engine crate read a switch from the process
environment, which the browser host does not have?

Rows: `ui_host_drift.rows.env_reads`.
"""

import re
from pathlib import Path

from ui_host_drift.roots import REPO
from ui_host_drift.rows.env_reads import (
    ENV_ALLOWED_NAMES,
    ENV_ALLOWED_PREFIXES,
    ENV_KERNEL_ROOTS,
    ENV_NATIVE_HOST_PATHS,
)
from ui_host_drift.rust_text import is_test_source, strip_all_comments

# Where this tier's rows are declared, as its own messages cite it.
ROWS_FILE = "scripts/ci/ui_host_drift/rows/env_reads.py"

# `env::var(`, `env::var_os(`, `env::vars(`, `env::vars_os(`, however the
# path is spelled (`std::env::`, or `use std::env;` then `env::`).
ENV_CALL_RE = re.compile(r"\benv\s*::\s*(var|var_os|vars|vars_os)\s*\(")
LITERAL_ARG_RE = re.compile(r'\s*"([^"\\]*)"')


def env_reads(src: str) -> list[tuple[str, str | None]]:
    """Every environment read in `src` (comments stripped), as
    `(call, name)`. `name` is the literal variable name, or `None` when the
    argument is not a string literal (or the call takes none)."""
    text = strip_all_comments(src)
    out: list[tuple[str, str | None]] = []
    for m in ENV_CALL_RE.finditer(text):
        lit = LITERAL_ARG_RE.match(text, m.end())
        out.append((m.group(1), lit.group(1) if lit else None))
    return out


def env_name_allowed(name: str | None) -> bool:
    """Is a read of `name` allowed inside an engine crate?"""
    if name is None:
        return False
    if name in ENV_ALLOWED_NAMES:
        return True
    return any(name.startswith(p) for p in ENV_ALLOWED_PREFIXES)


def env_read_violations(src: str) -> list[tuple[str, str | None]]:
    """The reads in `src` the allow-list does not cover."""
    return [(c, n) for c, n in env_reads(src) if not env_name_allowed(n)]


def _native_host_path(path: Path) -> bool:
    return any(path == p or p in path.parents for p in ENV_NATIVE_HOST_PATHS)


def check_env_reads() -> tuple[list[str], int]:
    """Tier 15: no engine crate a browser host links reads a gameplay switch
    from the environment. Returns `(problems, files_scanned)`."""
    problems: list[str] = []
    scanned = 0
    for root in ENV_KERNEL_ROOTS:
        if not root.exists():
            problems.append(f"MISSING ROOT {root.relative_to(REPO)}: drop or fix the row in {ROWS_FILE}")
            continue
        for path in sorted(root.rglob("*.rs")):
            if is_test_source(path) or _native_host_path(path):
                continue
            scanned += 1
            src = path.read_text(encoding="utf-8", errors="replace")
            for call, name in env_read_violations(src):
                rel = path.relative_to(REPO)
                shown = f'"{name}"' if name is not None else "a non-literal name"
                problems.append(
                    f"ENGINE ENV READ {rel}: `env::{call}` of {shown}. A wasm32 "
                    f"build has no environment, so the browser can never "
                    f"throw this switch. Put it on an engine field "
                    f"(`WorldToggles`) and have each host write it - the "
                    f"native session from its environment in "
                    f"crates/engine-session/src/native.rs, the play page from "
                    f"a `LegaiaRuntime` setter - or, for a diagnostic, name it "
                    f"`LEGAIA_DIAG_*` and declare it (tier 6). Allow-list: "
                    f"{ROWS_FILE}."
                )
    return problems, scanned
