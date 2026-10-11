"""Row self-check: is every path the rows name still on disk?

Every tier reads the files its rows name, and most of them treat a file that
is not there as a file with nothing in it: a rule whose surface is gone
reports the surface clean. A Rust file that is moved or split therefore
hollows out its rows without changing a line of output. This check is the
floor under all of them - it walks `ui_host_drift.roots` and every module of
`ui_host_drift.rows`, collects each value that is a path, and fails the gate
when one does not exist.

A value is a path when it is a `pathlib.Path`, or a string that starts at a
top-level repository directory and holds nothing but path characters. Regex
sources, function names and prose never match that shape, so the walk needs
no per-table schema and a new table joins by existing.
"""

import importlib
import pkgutil
import re
from pathlib import Path

from ui_host_drift import roots, rows
from ui_host_drift.roots import REPO

# Repo-relative path strings: a known top-level directory, then path characters
# only. No spaces, no regex metacharacters, no `::`.
PATH_STRING_RE = re.compile(r"(?:crates|site|scripts|docs)/[A-Za-z0-9_./-]+")

def is_path_string(value: str) -> bool:
    return PATH_STRING_RE.fullmatch(value) is not None


def _walk(value: object, found: set[Path], seen: set[int]) -> None:
    if isinstance(value, Path):
        found.add(value)
    elif isinstance(value, str):
        if is_path_string(value):
            found.add(REPO / value)
    elif isinstance(value, dict):
        if id(value) in seen:
            return
        seen.add(id(value))
        for key, item in value.items():
            _walk(key, found, seen)
            _walk(item, found, seen)
    elif isinstance(value, (list, tuple, set, frozenset)):
        if id(value) in seen:
            return
        seen.add(id(value))
        for item in value:
            _walk(item, found, seen)


def row_modules() -> list[object]:
    """`roots` plus every module under `rows`, in name order."""
    names = sorted(m.name for m in pkgutil.iter_modules(rows.__path__))
    return [roots] + [importlib.import_module(f"{rows.__name__}.{n}") for n in names]


def declared_paths() -> dict[Path, list[str]]:
    """Every path the rows name -> the `module.TABLE` names that carry it."""
    out: dict[Path, list[str]] = {}
    for module in row_modules():
        short = module.__name__.removeprefix("ui_host_drift.")
        for name, value in sorted(vars(module).items()):
            if name.startswith("_") or not name.isupper():
                continue
            found: set[Path] = set()
            _walk(value, found, set())
            for path in found:
                out.setdefault(path, []).append(f"{short}.{name}")
    return out


def check_declared_paths() -> tuple[list[str], int]:
    """`(problems, paths checked)`."""
    problems: list[str] = []
    paths = declared_paths()
    for path in sorted(paths):
        if path.exists():
            continue
        try:
            shown = path.relative_to(REPO)
        except ValueError:
            shown = path
        problems.append(
            f"MISSING PATH {shown}: named by {', '.join(sorted(paths[path]))} and "
            f"not on disk, so every row that reads it checks nothing. If the "
            f"file moved or was split, point the row at where its content is."
        )
    return problems, len(paths)
