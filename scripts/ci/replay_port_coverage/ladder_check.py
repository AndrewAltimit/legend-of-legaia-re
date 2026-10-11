"""Row self-check: does every canonical ladder still name a test on disk?

`CANONICAL_LADDERS` is `(test, package)` pairs, and the export recipe runs
each one. A ladder whose test file was renamed, or whose crate was split,
exports nothing; the union then silently loses a member, which reads exactly
like a port that stopped being reached. A ladder resolves when its package is
a workspace crate and that crate has `tests/<test>.rs`.
"""

from __future__ import annotations

import tomllib
from pathlib import Path

from replay_port_coverage.ladders import CANONICAL_LADDERS
from replay_port_coverage.paths import REPO


def crate_dirs() -> dict[str, Path]:
    """Package name -> crate directory, for every `crates/*/Cargo.toml`."""
    out: dict[str, Path] = {}
    for manifest in sorted((REPO / "crates").glob("*/Cargo.toml")):
        try:
            name = tomllib.loads(manifest.read_text(encoding="utf-8"))["package"]["name"]
        except (KeyError, tomllib.TOMLDecodeError):
            continue
        out[name] = manifest.parent
    return out


def check_ladders() -> list[str]:
    """One line per ladder that names a package or a test file not on disk."""
    crates = crate_dirs()
    problems: list[str] = []
    for test, package in CANONICAL_LADDERS:
        crate = crates.get(package)
        if crate is None:
            problems.append(
                f"ladder `{test}` names package `{package}`, which no "
                f"crates/*/Cargo.toml declares"
            )
            continue
        path = crate / "tests" / f"{test}.rs"
        if not path.is_file():
            problems.append(
                f"ladder `{test}` ({package}) has no {path.relative_to(REPO)}"
            )
    return problems
