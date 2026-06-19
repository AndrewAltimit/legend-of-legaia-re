#!/usr/bin/env python3
"""Port-tag drift checker for `crates/engine-*` Rust sources.

Catches the "I ported X but forgot to tag" pattern: when a Rust file adds a
citation `FUN_<addr>` for a function that has a Ghidra dump, the file should
either carry a matching `// PORT: FUN_<addr>` tag (claiming the port site)
or the line should carry a `// REF: FUN_<addr>` marker (claiming the mention
is intentional cross-reference and not a port).

The checker is **warn-only** by default. Pre-commit runs `--staged` so new
mentions get flagged at commit time; existing untagged citations are
grandfathered. `--scan-all` audits the entire engine codebase, `--strict`
turns warnings into a nonzero exit.

Tag shapes recognised (parallel to scripts/ci/port-catalog.py):

    // PORT: FUN_801dd35c                  -- single port
    // PORT: FUN_801dd35c, FUN_801cf244    -- multiple per tag
    /// PORT: FUN_801dd35c                 -- inside rustdoc
    //! PORT: FUN_801dd35c                 -- module-level

    // REF: FUN_80019b28                   -- cross-reference escape
    /// REF: FUN_80019b28                  -- rustdoc form
    //! REF: FUN_80019b28, FUN_80023070    -- module-level, multi-address

Both PORT and REF are **file-level**: a tag anywhere in the file claims the
address for the whole file. The idiomatic placement is a `//!` module-doc
block at the top:

    //! PORT: FUN_801E30E4
    //! REF: FUN_801E7320, FUN_801CF098

Usage:

    python3 scripts/ci/check-port-tags.py                 # default = --staged
    python3 scripts/ci/check-port-tags.py --scan-all      # full audit
    python3 scripts/ci/check-port-tags.py --strict        # exit 1 on warning
    python3 scripts/ci/check-port-tags.py --addr 80019b28 # drill-down
    python3 scripts/ci/check-port-tags.py --backfill-refs # write //! REF: blocks
"""

import argparse
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
FUNCS_DIR = REPO / "ghidra" / "scripts" / "funcs"
CRATES_DIR = REPO / "crates"

# Match the same SCUS / overlay code range port-catalog.py uses.
ADDR_RE = re.compile(
    r"FUN_(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})",
    re.IGNORECASE,
)

# Tag-bearing comment line (// , ///, //!). Accepts PORT or REF; we capture
# which one and the trailing tail (where addresses live).
TAG_RE = re.compile(
    r"//[/!]?\s*(PORT|REF)\s*:\s*(.*)",
    re.IGNORECASE,
)


def collect_dumped() -> set[str]:
    """Return the set of addresses that have a Ghidra dump under funcs/.

    Same naming convention as port-catalog.py: each dump file ends in
    `<8-hex>.txt`. The address is the last 8 hex digits before `.txt`.
    """
    out: set[str] = set()
    if not FUNCS_DIR.exists():
        return out
    addr_re = re.compile(r"([0-9a-fA-F]{8})\.txt$")
    for p in FUNCS_DIR.glob("*.txt"):
        m = addr_re.search(p.name)
        if m:
            out.add(m.group(1).lower())
    return out


def is_engine_crate_path(rel: str) -> bool:
    """Match Rust files under `crates/engine-*/`."""
    if not rel.endswith(".rs"):
        return False
    parts = rel.split("/")
    return len(parts) >= 2 and parts[0] == "crates" and parts[1].startswith("engine-")


def list_engine_rust_files() -> list[Path]:
    """Enumerate every Rust source under crates/engine-*/."""
    out: list[Path] = []
    if not CRATES_DIR.exists():
        return out
    for crate in sorted(CRATES_DIR.glob("engine-*")):
        if not crate.is_dir():
            continue
        out.extend(sorted(crate.rglob("*.rs")))
    return out


def list_staged_rust_files() -> list[Path]:
    """Files in the staging area that are engine-crate Rust sources."""
    try:
        proc = subprocess.run(
            ["git", "diff", "--cached", "--name-only", "--diff-filter=ACMR"],
            cwd=REPO,
            check=True,
            capture_output=True,
            text=True,
        )
    except subprocess.CalledProcessError:
        return []
    out: list[Path] = []
    for line in proc.stdout.splitlines():
        rel = line.strip()
        if rel and is_engine_crate_path(rel):
            p = REPO / rel
            if p.exists():
                out.append(p)
    return out


def collect_added_lines_for(path: Path) -> set[int]:
    """Return the set of line numbers added (or modified-into-existence) in
    the staged diff for `path`. Used by `--staged` to scope warnings to new
    citations only.

    A line number here is the line in the *post-image* (the file as it will
    look after the commit), matching what `enumerate(text.splitlines(), 1)`
    produces. We parse `git diff --cached --unified=0` hunk headers
    (`@@ -a,b +c,d @@`) and walk the post-image counter through `+` and ` `
    lines, treating `-` lines as deletions.
    """
    try:
        rel = path.relative_to(REPO).as_posix()
    except ValueError:
        return set()
    try:
        proc = subprocess.run(
            ["git", "diff", "--cached", "--unified=0", "--", rel],
            cwd=REPO,
            check=True,
            capture_output=True,
            text=True,
        )
    except subprocess.CalledProcessError:
        return set()
    added: set[int] = set()
    hunk_re = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")
    cur = 0
    for raw in proc.stdout.splitlines():
        if raw.startswith("@@"):
            m = hunk_re.match(raw)
            if m:
                cur = int(m.group(1))
            continue
        if raw.startswith("+++") or raw.startswith("---"):
            continue
        if raw.startswith("+"):
            added.add(cur)
            cur += 1
        elif raw.startswith(" "):
            cur += 1
        # `-` lines don't advance the post-image cursor.
    return added


def parse_tags(text: str) -> tuple[set[str], set[str]]:
    """Return `(port_tags, ref_tags)` extracted from `text`. Both file-level.

    Either tag silences citations to its addresses anywhere in the file.
    PORT means "the file ports this address"; REF means "the file mentions
    it as cross-reference, not a port site". The checker treats them as
    equivalent for warning purposes; `scripts/ci/port-catalog.py` only trusts
    PORT for the "ported" status column.
    """
    port_tags: set[str] = set()
    ref_tags: set[str] = set()
    for line in text.splitlines():
        m = TAG_RE.search(line)
        if not m:
            continue
        kind = m.group(1).upper()
        tail = m.group(2)
        addrs = {a.group(1).lower() for a in ADDR_RE.finditer(tail)}
        if not addrs:
            continue
        if kind == "PORT":
            port_tags |= addrs
        elif kind == "REF":
            ref_tags |= addrs
    return port_tags, ref_tags


def find_citations(text: str) -> list[tuple[int, str, str]]:
    """Return `[(line_no, addr, line_content)]` for every `FUN_<addr>` mention.

    Includes ALL mentions, including those on `// PORT:` / `// REF:` lines.
    The caller filters those out via the tag maps from `parse_tags`.
    """
    out: list[tuple[int, str, str]] = []
    for lineno, line in enumerate(text.splitlines(), 1):
        for m in ADDR_RE.finditer(line):
            out.append((lineno, m.group(1).lower(), line))
    return out


def check_file(
    path: Path,
    dumped: set[str],
    scope_lines: set[int] | None,
    only_addr: str | None,
) -> list[tuple[int, str, str]]:
    """Return drift warnings for `path`.

    A warning is `(line_no, addr, line_content)` for a citation that:
      - sits in `scope_lines` if scoping is requested (`--staged`);
      - matches `only_addr` if a drill-down is requested;
      - cites an address that has a Ghidra dump (porter-able);
      - is not tagged at the file level (`// PORT:` or `// REF:`);
      - is not itself the body of a PORT/REF tag line.

    Files with no `// PORT:` tags anywhere are treated as reference-only and
    skipped - citations inside them are descriptive doc-comment matter, and
    requiring REF tags in pure-docs files would be churn for no signal.
    """
    try:
        text = path.read_text()
    except (OSError, PermissionError):
        return []
    port_tags, ref_tags = parse_tags(text)
    if not port_tags:
        return []  # reference-only file; not in scope of the drift check
    silenced = port_tags | ref_tags
    warnings: list[tuple[int, str, str]] = []
    for lineno, addr, line in find_citations(text):
        if scope_lines is not None and lineno not in scope_lines:
            continue
        if only_addr and addr != only_addr:
            continue
        if addr not in dumped:
            continue  # can't be a port site; nothing to tag against
        if addr in silenced:
            continue  # tagged at file level (PORT or REF)
        # Skip lines that are themselves a PORT or REF tag (the address there
        # is part of the tag header, not a freeform citation).
        if TAG_RE.search(line):
            continue
        warnings.append((lineno, addr, line.rstrip()))
    return warnings


MODULE_DOC_RE = re.compile(r"^\s*//!")


def insert_ref_block(text: str, addrs: list[str]) -> str:
    """Insert `//! REF: FUN_X, FUN_Y, ...` after the leading `//!` block.

    Falls back to prepending the block if the file has no leading module-doc
    comment. Existing `//! REF:` lines are preserved; the new block is added
    alongside (we never edit an existing one because re-running backfill
    should be idempotent in the sense that it only adds new addresses).
    """
    # Existing PORT tags in the codebase use Ghidra's uppercase hex form
    # (`FUN_801DD35C`); match that style in the backfill output for
    # readability, even though the catalog accepts either case. Wrap long
    # REF lists at 6 addresses per line so 40+ address files don't produce
    # a single un-grokkable line.
    chunked = [
        ", ".join(f"FUN_{a.upper()}" for a in addrs[i : i + 6])
        for i in range(0, len(addrs), 6)
    ]
    new_line = "\n".join(f"//! REF: {chunk}" for chunk in chunked)
    lines = text.splitlines(keepends=True)
    # Locate the index past the last consecutive `//!` line at the top
    # (skipping a possible leading blank or `//` line). We allow `//` and
    # blank lines to break the run only if they aren't `//!` -- a non-`//!`
    # line marks the boundary of the module-doc block.
    insert_at = 0
    last_module_doc = -1
    for i, raw in enumerate(lines):
        stripped = raw.rstrip("\n")
        if MODULE_DOC_RE.match(stripped):
            last_module_doc = i
            continue
        if stripped.strip() == "" and last_module_doc >= 0:
            # blank line WITHIN the doc block; allow it
            continue
        # First non-doc, non-blank line. Stop.
        break
    if last_module_doc >= 0:
        insert_at = last_module_doc + 1
        injected = new_line + "\n"
    else:
        # No leading //! block. Prepend a small one.
        insert_at = 0
        injected = new_line + "\n//!\n"
    lines.insert(insert_at, injected)
    return "".join(lines)


def run_backfill(dumped: set[str]) -> int:
    """`--backfill-refs` driver. For each engine-* Rust file that has at
    least one `// PORT:` tag and ALSO has untagged citations, append a
    `//! REF: ...` block listing every untagged address. Writes in place.

    The pass is conservative: it only touches files that are already in
    scope of the drift check (i.e. port-bearing) and only adds tags - no
    text is removed. Pure-docs files (no PORT tag) stay untouched.
    """
    n_files = 0
    n_addrs = 0
    for path in list_engine_rust_files():
        try:
            text = path.read_text()
        except (OSError, PermissionError):
            continue
        port_tags, ref_tags = parse_tags(text)
        if not port_tags:
            continue
        silenced = port_tags | ref_tags
        untagged: set[str] = set()
        for _lineno, addr, line in find_citations(text):
            if addr in dumped and addr not in silenced and not TAG_RE.search(line):
                untagged.add(addr)
        if not untagged:
            continue
        addrs = sorted(untagged)
        new_text = insert_ref_block(text, addrs)
        path.write_text(new_text)
        try:
            rel = path.relative_to(REPO).as_posix()
        except ValueError:
            rel = str(path)
        print(f"{rel}: added //! REF block with {len(addrs)} address(es)")
        n_files += 1
        n_addrs += len(addrs)
    print(
        f"\n[check-port-tags --backfill-refs] wrote {n_files} file(s), "
        f"{n_addrs} address(es) total"
    )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument(
        "--staged",
        action="store_true",
        help="check only lines added in the staging area (default)",
    )
    mode.add_argument(
        "--scan-all",
        action="store_true",
        help="check every line of every engine-crate Rust file",
    )
    ap.add_argument(
        "--strict",
        action="store_true",
        help="exit 1 if any warning is reported (default: exit 0, warn-only)",
    )
    ap.add_argument(
        "--addr",
        type=str,
        default="",
        help="drill down on a single address (lowercase hex)",
    )
    ap.add_argument(
        "--quiet",
        action="store_true",
        help="suppress the trailing summary line",
    )
    ap.add_argument(
        "--backfill-refs",
        action="store_true",
        help="auto-edit each file to add a `//! REF: ...` block listing every "
        "currently-untagged citation. One-shot grandfather pass; files are "
        "rewritten in place. Implies --scan-all.",
    )
    args = ap.parse_args()

    dumped = collect_dumped()
    only_addr = args.addr.lower().removeprefix("0x") if args.addr else None

    if args.backfill_refs:
        return run_backfill(dumped)

    if args.scan_all:
        files = list_engine_rust_files()
        scope: dict[Path, set[int] | None] = {f: None for f in files}
        mode_label = "scan-all"
    else:
        # Default to --staged. If git can't be reached or nothing is staged,
        # fall through with an empty file list rather than erroring.
        files = list_staged_rust_files()
        scope = {f: collect_added_lines_for(f) for f in files}
        mode_label = "staged"

    total_warnings = 0
    total_files = 0
    for path in files:
        scope_lines = scope[path]
        warnings = check_file(path, dumped, scope_lines, only_addr)
        if not warnings:
            continue
        total_files += 1
        try:
            rel = path.relative_to(REPO).as_posix()
        except ValueError:
            rel = str(path)
        for lineno, addr, line in warnings:
            total_warnings += 1
            preview = line.strip()
            if len(preview) > 100:
                preview = preview[:97] + "..."
            print(
                f"{rel}:{lineno}: cites FUN_{addr} but no `// PORT:` or "
                f"`// REF:` tag for that address in the file"
            )
            print(f"    | {preview}")

    if not args.quiet:
        print(
            f"\n[check-port-tags --{mode_label}] "
            f"{total_warnings} drift warning(s) across {total_files} file(s)"
        )

    return 1 if (args.strict and total_warnings > 0) else 0


if __name__ == "__main__":
    sys.exit(main())
