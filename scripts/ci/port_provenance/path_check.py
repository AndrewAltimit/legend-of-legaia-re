"""Row self-check: is every path `port_provenance.rows` names still on disk?

`module_of` folds a file into its split module by a string prefix, so a
`SPLIT_MODULES` row whose directory was renamed or moved matches no file and
says nothing: its tags fall back to one module per file, their finding keys
change, and the waivers keyed on the parent file stop matching - all without
an error. A row is live exactly when its directory exists and its parent file
(`<row>.rs` beside it, or `src/lib.rs` for a row naming a crate's `src`)
exists.
"""

from __future__ import annotations

from port_provenance.paths import REPO
from port_provenance.rows import DEFINING_DOC_ROOTS, SPLIT_MODULES
from port_provenance.tags import split_parent


def check_declared_paths() -> list[str]:
    """One line per row that names something not on disk."""
    problems: list[str] = []
    for stem in sorted(SPLIT_MODULES):
        if not (REPO / stem).is_dir():
            problems.append(
                f"SPLIT_MODULES names `{stem}`, which is not a directory - no "
                f"file folds into it. Point the row at where the module is."
            )
        elif not (REPO / split_parent(stem)).is_file():
            problems.append(
                f"SPLIT_MODULES names `{stem}`, but its parent file "
                f"`{split_parent(stem)}` does not exist - the findings would be keyed on "
                f"a file that is not there."
            )
    for root in DEFINING_DOC_ROOTS:
        if not (REPO / root).is_dir():
            problems.append(
                f"DEFINING_DOC_ROOTS names `{root}`, which is not a directory - "
                f"`doc-citation` and `dual-label` read no page under it."
            )
    return problems
