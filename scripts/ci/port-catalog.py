#!/usr/bin/env python3
"""Port-catalog tracker: per-address status across decompilation and engine port.

Generates a unified per-function table with three independent status columns:

  - dumped     : a Ghidra decompiler dump exists under `ghidra/scripts/funcs/`
  - documented : the address is cited from at least one file under `docs/`
  - ported     : the address appears in a `// PORT: FUN_<addr>` tag in a Rust
                 source file under `crates/`. The tag is the source of truth -
                 grep-by-mention is too noisy because addresses get cited as
                 "not yet ported" or "inspired by" in port-progress comments.

Reuses helpers from `function-coverage.py` where useful: same dump-file naming
convention, same address regex shape (SCUS 0x80010000-0x8006FFFF and overlays
0x801C0000-0x8020FFFF).

PORT tag format (Rust source files under crates/). The scanner accepts the
tag inside any of three comment forms:

    // PORT: FUN_801dd35c                  -- plain line comment
    /// PORT: FUN_801dd35c                  -- outer doc-comment (attaches to next item)
    //! PORT: FUN_801dd35c, FUN_801cf244    -- inner doc-comment (module-level)

The dominant convention in this codebase is the `//!` module-level form: each
crate file leads with a doc block that lists every dispatcher / top-level
function ported into that module on one (or several) `//! PORT:` line(s). The
catalog counts each distinct `FUN_<addr>` only once across all tag forms, so a
quick sanity check is:

    grep -rEi '//[/!]?\\s*PORT\\s*:' crates/ | grep -oEi 'FUN_8[0-9a-f]{7}' | sort -u | wc -l

A plain `grep -r "// PORT:"` under-counts dramatically — most tags live in
`//!` doc blocks that don't contain the literal substring `"// "`.

Trailing context after the address list is allowed:

    //! PORT: FUN_801dd35c (sub-mode jump table)

Addresses are matched strictly to the SCUS/overlay range (0x80010000-0x8006FFFF
and 0x801C0000-0x8020FFFF) so unrelated text in the tail of a tag won't
false-positive.

Usage:
    python3 scripts/ci/port-catalog.py                       # build global catalog
    python3 scripts/ci/port-catalog.py --missing-ports       # dumped+documented, not ported
    python3 scripts/ci/port-catalog.py --missing-dumps       # cited but not dumped
    python3 scripts/ci/port-catalog.py --md                  # markdown table to stdout
    python3 scripts/ci/port-catalog.py --top 30              # cap rows
    python3 scripts/ci/port-catalog.py --addr 801dd35c       # single-row drill-down
    python3 scripts/ci/port-catalog.py --dashboard           # open-work rollup (single page)

Output (default): target/port-catalog/catalog.csv + catalog.md
"""

import argparse
import csv
import re
import sys
import tomllib
from collections import Counter, defaultdict, deque
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
FUNCS_DIR = REPO / "ghidra" / "scripts" / "funcs"
DOCS_DIR = REPO / "docs"
CRATES_DIR = REPO / "crates"
OUT_DIR = REPO / "target" / "port-catalog"
FEATURES_TOML = REPO / "scripts" / "ci" / "features.toml"
IGNORE_TOML = REPO / "scripts" / "ci" / "port-catalog-ignore.toml"

# Address ranges that correspond to executable code:
#   SCUS_942.54   : 0x80010000 - 0x8006FFFF
#   Overlays      : 0x801C0000 - 0x8020FFFF
# Match the same shape function-coverage.py uses so the two tools share a worldview.
# IGNORECASE so PORT tags written `FUN_801DD35C` (uppercase) match as cleanly
# as the lowercase form Ghidra emits.
CODE_ADDR_RE = re.compile(r"80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4}", re.IGNORECASE)

# Addresses scraped from a `// PORT:` tag's tail. Same code-range shape as
# CODE_ADDR_RE, but with the citation regex's negative lookbehinds on `_DAT_` /
# `DAT_` / `PTR_`: a PORT tag's prose often names the *data global* a ported
# function writes (e.g. ``PORT: the `_DAT_801F0204 = N` writes in FUN_801DD35C``),
# and an overlay-range data global (0x801c..0x8020) would otherwise be miscounted
# as a ported function address - a "ported but not dumped" false positive, since
# data globals have no function dump. The function on the same line (`FUN_...`)
# is still picked up, so the real port credit is unaffected.
PORT_ADDR_RE = re.compile(
    r"(?<!PTR_)(?<!_DAT_)(?<!DAT_)(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})",
    re.IGNORECASE,
)

# Citations of an address by some other piece of text. Covers Ghidra's auto-named
# call forms plus raw disassembly forms. Matches `function-coverage.py` with one
# addition: a negative lookbehind on `PTR_` so we don't false-positive on
# `PTR_FUN_<addr>` symbols, which Ghidra emits for data-pointer tables that
# happen to contain function pointers - the address there is a *table base*,
# not a function entry. Same negative lookbehind on `_DAT_` for completeness.
CITATION_RE = re.compile(
    r"(?<!PTR_)(?<!_DAT_)(?:FUN_|func_0x|jal\s+0x|jalr\s+\w+,0x|->\s*func_0x)"
    r"(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})"
)

# Doc-side citation. More permissive: doc text uses `FUN_8003EBE4` (caps),
# bare `0x8003e4e8` forms, and backtick-wrapped bare addresses like
# `` `80056678` `` (the shape `docs/reference/functions.md` uses in table
# rows). The backtick form requires an OPENING backtick so we don't false-
# positive on every hex literal in the SCUS range that happens to occur in
# tables or paragraphs.
DOC_CITATION_RE = re.compile(
    r"(?:FUN_|0x|`)(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})",
    re.IGNORECASE,
)

# // PORT: FUN_801dd35c [, FUN_xxx]*  --  the only signal we trust for "ported".
# Accepts plain `//`, doc `//!`, and outer-doc `///` so the tag can live inside
# a rustdoc block (where the provenance is co-located with the human-readable
# description) or as a standalone comment.
PORT_TAG_RE = re.compile(
    r"//[/!]?\s*PORT\s*:\s*(.*)",
    re.IGNORECASE,
)


def collect_dumped() -> dict[str, str]:
    """Return {addr: dump_filename_stem} for every dump under ghidra/scripts/funcs/."""
    out: dict[str, str] = {}
    if not FUNCS_DIR.exists():
        return out
    for p in FUNCS_DIR.glob("*.txt"):
        m = re.search(r"([0-9a-fA-F]{8})\.txt$", p.name)
        if m:
            addr = m.group(1).lower()
            # Prefer overlay dumps over SCUS-named bare dumps if both exist.
            stem = p.stem
            if addr not in out or "overlay_" in stem:
                out[addr] = stem
    return out


def collect_citations() -> tuple[Counter, dict[str, list[str]]]:
    """Walk every dump and count citations of every code-range address.

    Returns (refs counter, sources map). `sources[addr]` is the list of dump
    filename stems that mention `addr` - useful for traceability columns.
    """
    refs: Counter = Counter()
    sources: dict[str, list[str]] = defaultdict(list)
    if not FUNCS_DIR.exists():
        return refs, sources
    for p in sorted(FUNCS_DIR.glob("*.txt")):
        if (
            p.name.endswith("_unique_index.txt")
            or p.name.endswith("_index.txt")
            or p.name.endswith("_survey.txt")
        ):
            continue
        try:
            text = p.read_text(errors="ignore")
        except (PermissionError, OSError):
            continue
        seen: set[str] = set()
        for m in CITATION_RE.finditer(text):
            a = m.group(1).lower()
            if a in seen:
                continue
            seen.add(a)
            refs[a] += 1
            sources[a].append(p.stem)
    return refs, sources


def collect_doc_citations() -> dict[str, set[str]]:
    """Return {addr: set(doc_paths)} for every code-range address cited in docs/."""
    out: dict[str, set[str]] = defaultdict(set)
    if not DOCS_DIR.exists():
        return out
    for p in DOCS_DIR.rglob("*.md"):
        try:
            text = p.read_text(errors="ignore")
        except (PermissionError, OSError):
            continue
        seen: set[str] = set()
        for m in DOC_CITATION_RE.finditer(text):
            a = m.group(1).lower()
            if a in seen:
                continue
            seen.add(a)
            rel = str(p.relative_to(REPO))
            out[a].add(rel)
    return out


def collect_ports() -> dict[str, set[str]]:
    """Return {addr: set(crate_names)} for every Rust `// PORT: FUN_<addr>` tag."""
    out: dict[str, set[str]] = defaultdict(set)
    if not CRATES_DIR.exists():
        return out
    for p in CRATES_DIR.rglob("*.rs"):
        try:
            text = p.read_text(errors="ignore")
        except (PermissionError, OSError):
            continue
        # Crate name is the first path segment under crates/.
        try:
            rel = p.relative_to(CRATES_DIR)
        except ValueError:
            continue
        crate = rel.parts[0] if rel.parts else "?"
        for line in text.splitlines():
            tag = PORT_TAG_RE.search(line)
            if not tag:
                continue
            # Inside the tail of the tag, pick up every code-range *function*
            # address - excluding `_DAT_` / `PTR_` data globals the prose names.
            for m in PORT_ADDR_RE.finditer(tag.group(1)):
                addr = m.group(1).lower()
                out[addr].add(crate)
    return out


def build_call_graph(sources: dict[str, list[str]]) -> dict[str, set[str]]:
    """Build forward call-graph edges: `{src_addr: set(cited_addr)}`.

    Each dump filename ends with an 8-hex-digit address that identifies the
    function the dump documents. For every (cited_addr, [src_files]) entry in
    `sources`, we derive an edge `src_file_addr -> cited_addr`.

    Note: edges only exist between addresses that are *dumped*. Undumped
    helpers have no outgoing edges - the BFS frontier widens only as more
    dumps land.
    """
    graph: dict[str, set[str]] = defaultdict(set)
    addr_re = re.compile(r"([0-9a-fA-F]{8})$", re.IGNORECASE)
    for cited_addr, src_files in sources.items():
        for src in src_files:
            m = addr_re.search(src)
            if not m:
                continue
            src_addr = m.group(1).lower()
            if src_addr == cited_addr:
                continue
            graph[src_addr].add(cited_addr)
    return graph


def bfs_reachable(
    graph: dict[str, set[str]],
    roots: list[str],
    stop_at: set[str] | None = None,
    max_depth: int | None = None,
) -> set[str]:
    """Return the set of addresses reachable from `roots` via citation edges.

    `stop_at` addresses are kept in the result but their callees aren't followed
    - useful for stopping at shared-infrastructure boundaries (e.g. the generic
    CD loader) so a feature filter doesn't pull in everything reachable.
    """
    visited: set[str] = set()
    stop_at = stop_at or set()
    queue: deque[tuple[str, int]] = deque()
    for r in roots:
        rl = r.lower()
        if rl not in visited:
            visited.add(rl)
            queue.append((rl, 0))
    while queue:
        addr, depth = queue.popleft()
        if addr in stop_at:
            continue
        if max_depth is not None and depth >= max_depth:
            continue
        for nxt in graph.get(addr, ()):
            if nxt not in visited:
                visited.add(nxt)
                queue.append((nxt, depth + 1))
    return visited


def load_features() -> dict[str, dict]:
    """Load feature definitions from scripts/ci/features.toml. Returns {} if missing."""
    if not FEATURES_TOML.exists():
        return {}
    with FEATURES_TOML.open("rb") as f:
        data = tomllib.load(f)
    out = {}
    for name, body in data.items():
        if not isinstance(body, dict):
            continue
        out[name] = {
            "description": body.get("description", ""),
            "roots": [r.lower() for r in body.get("roots", [])],
            "stop_at": {a.lower() for a in body.get("stop_at", [])},
            "max_depth": body.get("max_depth"),
        }
    return out


def load_ignore() -> dict[str, tuple[str, str]]:
    """Load `scripts/ci/port-catalog-ignore.toml`. Returns `{addr: (category, reason)}`.

    The TOML is organised as top-level tables (one per category) of
    `addr = "reason"` rows. Addresses across different categories merge into
    a single map; the category travels along for drill-down.
    """
    if not IGNORE_TOML.exists():
        return {}
    with IGNORE_TOML.open("rb") as f:
        data = tomllib.load(f)
    out: dict[str, tuple[str, str]] = {}
    for category, body in data.items():
        if not isinstance(body, dict):
            continue
        for addr, reason in body.items():
            out[addr.lower()] = (category, str(reason))
    return out


def build_rows(
    dumped: dict[str, str],
    refs: Counter,
    sources: dict[str, list[str]],
    docs: dict[str, set[str]],
    ports: dict[str, set[str]],
    ignore: dict[str, tuple[str, str]] | None = None,
) -> list[dict]:
    """Union the four signals into a per-address row list, sorted by address."""
    addrs = set(dumped) | set(refs) | set(docs) | set(ports)
    ignore = ignore or {}
    rows: list[dict] = []
    for addr in sorted(addrs):
        dump_stem = dumped.get(addr, "")
        is_dumped = bool(dump_stem)
        doc_hits = sorted(docs.get(addr, set()))
        port_crates = sorted(ports.get(addr, set()))
        is_documented = bool(doc_hits)
        is_ported = bool(port_crates)
        ig = ignore.get(addr)
        bucket = "scus" if int(addr, 16) < 0x801C0000 else "overlay"
        rows.append(
            {
                "addr": addr,
                "bucket": bucket,
                "dumped": is_dumped,
                "dump_source": dump_stem,
                "documented": is_documented,
                "doc_sources": doc_hits,
                "ported": is_ported,
                "port_crates": port_crates,
                "refs": refs.get(addr, 0),
                "first_sources": sources.get(addr, [])[:3],
                "ignored": ig is not None,
                "ignore_category": ig[0] if ig else "",
                "ignore_reason": ig[1] if ig else "",
            }
        )
    return rows


def yesno(b: bool) -> str:
    return "yes" if b else "—"


def render_csv(rows: list[dict], out_path: Path) -> None:
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", newline="") as f:
        w = csv.writer(f)
        w.writerow(
            [
                "addr",
                "bucket",
                "dumped",
                "documented",
                "ported",
                "ignored",
                "ignore_category",
                "ignore_reason",
                "port_crates",
                "doc_sources",
                "refs",
                "first_dump_sources",
            ]
        )
        for r in rows:
            w.writerow(
                [
                    r["addr"],
                    r["bucket"],
                    int(r["dumped"]),
                    int(r["documented"]),
                    int(r["ported"]),
                    int(r["ignored"]),
                    r["ignore_category"],
                    r["ignore_reason"],
                    "|".join(r["port_crates"]),
                    "|".join(r["doc_sources"]),
                    r["refs"],
                    "|".join(r["first_sources"]),
                ]
            )


def render_md(rows: list[dict], out_path: Path | None, title: str) -> str:
    lines = [
        f"# {title}",
        "",
        "Generated by `scripts/ci/port-catalog.py`. Three independent status columns:",
        "",
        "- **dumped** — Ghidra decompiler output exists under `ghidra/scripts/funcs/`.",
        "- **documented** — the address is cited from at least one file under `docs/`.",
        "- **ported** — the address appears in a `// PORT: FUN_<addr>` tag in a Rust source under `crates/`.",
        "- **ignore** — address is listed in `scripts/ci/port-catalog-ignore.toml` as non-port-site (PsyQ / BIOS / libgte / ...).",
        "",
        "| addr | bucket | dumped | documented | ported (crates) | ignore | refs | first dump source |",
        "|---|---|---|---|---|---|---|---|",
    ]
    for r in rows:
        crates = ", ".join(r["port_crates"]) if r["port_crates"] else "—"
        first_src = r["first_sources"][0] if r["first_sources"] else "—"
        ignore_cell = r["ignore_category"] if r["ignored"] else "—"
        lines.append(
            f"| `{r['addr']}` | {r['bucket']} | {yesno(r['dumped'])} | "
            f"{yesno(r['documented'])} | {crates} | {ignore_cell} | "
            f"{r['refs']} | `{first_src}` |"
        )
    md = "\n".join(lines) + "\n"
    if out_path:
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(md)
    return md


def summarize(rows: list[dict]) -> str:
    n = len(rows)
    n_dumped = sum(1 for r in rows if r["dumped"])
    n_documented = sum(1 for r in rows if r["documented"])
    n_ported = sum(1 for r in rows if r["ported"])
    # Cross-cuts that the table is meant to surface
    dd = sum(1 for r in rows if r["dumped"] and r["documented"])
    ddp = sum(1 for r in rows if r["dumped"] and r["documented"] and r["ported"])
    dd_not_p = sum(
        1 for r in rows if r["dumped"] and r["documented"] and not r["ported"]
    )
    dd_not_p_ignored = sum(
        1
        for r in rows
        if r["dumped"] and r["documented"] and not r["ported"] and r["ignored"]
    )
    cited_not_dumped = sum(1 for r in rows if r["refs"] > 0 and not r["dumped"])
    ported_not_documented = sum(
        1 for r in rows if r["ported"] and not r["documented"]
    )
    ported_not_dumped = sum(1 for r in rows if r["ported"] and not r["dumped"])
    return "\n".join(
        [
            f"total addresses tracked       : {n}",
            f"dumped                        : {n_dumped}",
            f"documented (in docs/)         : {n_documented}",
            f"ported (// PORT: tag)         : {n_ported}",
            "",
            f"dumped + documented           : {dd}",
            f"dumped + documented + ported  : {ddp}",
            f"dumped + documented, NOT ported                 : {dd_not_p}",
            f"  of which ignored (PsyQ / BIOS / libgte / ...) : {dd_not_p_ignored}",
            f"  remaining port worklist                       : {dd_not_p - dd_not_p_ignored}",
            "",
            f"cited but NOT dumped  (dump worklist)           : {cited_not_dumped}",
            f"ported but NOT documented (provenance gap)      : {ported_not_documented}",
            f"ported but NOT dumped     (provenance gap)      : {ported_not_dumped}",
        ]
    )


def render_dashboard(
    all_rows: list[dict],
    sources: dict[str, list[str]],
    features: dict[str, dict],
    top_n: int,
    out_path: Path | None,
) -> str:
    """Generate the open-work dashboard: one regenerable page that combines
    per-feature port stats, per-feature top-N missing-ports, the ignore-list
    summary, and a pointer to the open-RE-threads index.

    The dashboard is the answer to "what's next" at a glance. It is generated
    output (target/port-catalog/open-work.md, gitignored) and re-runs cheaply.
    """
    rows_by_addr = {r["addr"]: r for r in all_rows}
    graph = build_call_graph(sources)

    lines: list[str] = [
        "# Open work — port-catalog dashboard",
        "",
        "Regenerated by `scripts/ci/port-catalog.py --dashboard`. Cross-references:",
        "",
        "- [`docs/reference/open-rev-eng-threads.md`](../../docs/reference/open-rev-eng-threads.md) — question-level open hunts (what is *unknown*; complements this page's per-function worklists).",
        "- [`docs/tooling/port-catalog.md`](../../docs/tooling/port-catalog.md) — tool usage + column semantics.",
        "- [`scripts/ci/features.toml`](../../scripts/ci/features.toml) — feature definitions (roots + stop_at boundaries).",
        "- [`scripts/ci/port-catalog-ignore.toml`](../../scripts/ci/port-catalog-ignore.toml) — addresses excluded from the port worklist.",
        "",
        "## Global",
        "",
    ]
    # Global summary block — render as a markdown-friendly variant.
    n_dumped = sum(1 for r in all_rows if r["dumped"])
    n_documented = sum(1 for r in all_rows if r["documented"])
    n_ported = sum(1 for r in all_rows if r["ported"])
    n_ignored = sum(1 for r in all_rows if r["ignored"])
    dd_not_p = sum(
        1 for r in all_rows if r["dumped"] and r["documented"] and not r["ported"]
    )
    dd_not_p_ignored = sum(
        1
        for r in all_rows
        if r["dumped"] and r["documented"] and not r["ported"] and r["ignored"]
    )
    remaining = dd_not_p - dd_not_p_ignored
    lines.extend(
        [
            f"- **dumped**: {n_dumped}",
            f"- **documented**: {n_documented}",
            f"- **ported** (`// PORT:` tag): {n_ported}",
            f"- **ignored** (PsyQ / BIOS / libgte / ...): {n_ignored}",
            f"- **port worklist** (dumped + documented, not ported, not ignored): {remaining}",
            "",
        ]
    )

    if features:
        lines.extend(
            [
                "## Per-feature status",
                "",
                "Each feature is a BFS over the citation graph starting from `roots`, bounded by `stop_at`.",
                "Reachable counts widen as more dumps land — feature views start tight and grow.",
                "",
                "| Feature | Reachable | Ported | Port % | Missing | Ignored | Description |",
                "|---|---:|---:|---:|---:|---:|---|",
            ]
        )
        # Compute per-feature numbers and stash for the per-feature top-N
        # sections that follow the summary table.
        feature_stats: list[tuple[str, dict, list[dict]]] = []
        for name in sorted(features):
            body = features[name]
            reachable = bfs_reachable(
                graph,
                body["roots"],
                stop_at=body["stop_at"],
                max_depth=body["max_depth"],
            )
            f_rows = [rows_by_addr[a] for a in reachable if a in rows_by_addr]
            n_reach = len(f_rows)
            n_ported_f = sum(1 for r in f_rows if r["ported"])
            n_missing_f = sum(
                1
                for r in f_rows
                if r["dumped"]
                and r["documented"]
                and not r["ported"]
                and not r["ignored"]
            )
            n_ignored_f = sum(1 for r in f_rows if r["ignored"])
            pct = (100.0 * n_ported_f / n_reach) if n_reach else 0.0
            stats = {
                "reachable": n_reach,
                "ported": n_ported_f,
                "pct": pct,
                "missing": n_missing_f,
                "ignored": n_ignored_f,
            }
            feature_stats.append((name, stats, f_rows))
            desc = body.get("description", "") or ""
            lines.append(
                f"| `{name}` | {n_reach} | {n_ported_f} | {pct:.1f}% | "
                f"{n_missing_f} | {n_ignored_f} | {desc} |"
            )
        lines.append("")

        # Per-feature top-N missing-ports — the highest-leverage helpers first.
        # Highest-citation-count helpers tend to be shared infrastructure the
        # feature can't progress without.
        lines.extend(
            [
                f"## Per-feature missing-ports (top {top_n} by citation count)",
                "",
                "These are the dumped + documented helpers reachable from each feature's roots that don't yet carry a `// PORT:` tag. Ignore-list entries are excluded.",
                "",
            ]
        )
        for name, stats, f_rows in feature_stats:
            missing = sorted(
                (
                    r
                    for r in f_rows
                    if r["dumped"]
                    and r["documented"]
                    and not r["ported"]
                    and not r["ignored"]
                ),
                key=lambda r: (-r["refs"], r["addr"]),
            )[:top_n]
            lines.append(f"### `{name}` — {stats['missing']} missing")
            lines.append("")
            if not missing:
                lines.append("All reachable + documented helpers ported.")
                lines.append("")
                continue
            lines.extend(
                [
                    "| addr | refs | first dump source | doc sources |",
                    "|---|---:|---|---|",
                ]
            )
            for r in missing:
                first_src = r["first_sources"][0] if r["first_sources"] else "—"
                docs_cell = (
                    ", ".join(r["doc_sources"][:3]) if r["doc_sources"] else "—"
                )
                lines.append(
                    f"| `{r['addr']}` | {r['refs']} | `{first_src}` | {docs_cell} |"
                )
            lines.append("")
    else:
        lines.extend(
            [
                "## Per-feature status",
                "",
                "_No features defined in `scripts/ci/features.toml`._",
                "",
            ]
        )

    # Ignore-list breakdown — per category.
    ignored_rows = [r for r in all_rows if r["ignored"]]
    if ignored_rows:
        by_cat: dict[str, int] = defaultdict(int)
        for r in ignored_rows:
            by_cat[r["ignore_category"]] += 1
        lines.extend(
            [
                "## Ignore-list summary",
                "",
                "Addresses explicitly out of scope for engine porting — statically-linked PsyQ / BIOS / SDK code mapped to native equivalents (Rust stdlib, wgpu, cpal). Source: [`scripts/ci/port-catalog-ignore.toml`](../../scripts/ci/port-catalog-ignore.toml).",
                "",
                "| Category | Count |",
                "|---|---:|",
            ]
        )
        for cat in sorted(by_cat):
            lines.append(f"| `{cat}` | {by_cat[cat]} |")
        lines.append(f"| **total** | **{len(ignored_rows)}** |")
        lines.append("")

    # Provenance gaps — small but worth surfacing on the same page.
    gaps_port_no_doc = [r for r in all_rows if r["ported"] and not r["documented"]]
    gaps_port_no_dump = [r for r in all_rows if r["ported"] and not r["dumped"]]
    if gaps_port_no_doc or gaps_port_no_dump:
        lines.extend(
            [
                "## Provenance gaps",
                "",
                "Addresses with a `// PORT:` tag but missing the supporting trail. Either backfill the missing axis or remove the tag if attribution was wrong.",
                "",
            ]
        )
        if gaps_port_no_doc:
            lines.append(f"- **ported but not documented**: {len(gaps_port_no_doc)}")
        if gaps_port_no_dump:
            lines.append(f"- **ported but not dumped**: {len(gaps_port_no_dump)}")
        lines.append("")
        if gaps_port_no_doc:
            lines.append("### Ported but not documented")
            lines.append("")
            lines.append("| addr | crates |")
            lines.append("|---|---|")
            for r in sorted(gaps_port_no_doc, key=lambda r: r["addr"]):
                lines.append(
                    f"| `{r['addr']}` | {', '.join(r['port_crates']) or '—'} |"
                )
            lines.append("")
        if gaps_port_no_dump:
            lines.append("### Ported but not dumped")
            lines.append("")
            lines.append("| addr | crates |")
            lines.append("|---|---|")
            for r in sorted(gaps_port_no_dump, key=lambda r: r["addr"]):
                lines.append(
                    f"| `{r['addr']}` | {', '.join(r['port_crates']) or '—'} |"
                )
            lines.append("")

    md = "\n".join(lines) + "\n"
    if out_path:
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(md)
    return md


def filter_rows(rows: list[dict], args: argparse.Namespace) -> list[dict]:
    out = rows
    if args.addr:
        addr = args.addr.lower().removeprefix("0x")
        out = [r for r in out if r["addr"] == addr]
    if args.missing_ports:
        out = [
            r for r in out if r["dumped"] and r["documented"] and not r["ported"]
        ]
        if not args.include_ignored:
            out = [r for r in out if not r["ignored"]]
        out.sort(key=lambda r: (-r["refs"], r["addr"]))
    if args.missing_dumps:
        out = [r for r in out if r["refs"] > 0 and not r["dumped"]]
        out.sort(key=lambda r: (-r["refs"], r["addr"]))
    if args.ported_only:
        out = [r for r in out if r["ported"]]
    if args.ignored_only:
        out = [r for r in out if r["ignored"]]
        out.sort(key=lambda r: (r["ignore_category"], r["addr"]))
    if args.bucket:
        out = [r for r in out if r["bucket"] == args.bucket]
    if args.top:
        out = out[: args.top]
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--md",
        action="store_true",
        help="emit markdown to stdout instead of CSV-to-file (CSV/MD still written to target/port-catalog/)",
    )
    ap.add_argument("--top", type=int, default=0, help="cap output rows (0 = no cap)")
    ap.add_argument("--addr", type=str, default="", help="single-address drill-down")
    ap.add_argument(
        "--missing-ports",
        action="store_true",
        help="filter: dumped + documented but not ported (port worklist)",
    )
    ap.add_argument(
        "--missing-dumps",
        action="store_true",
        help="filter: cited but not yet dumped (dump worklist)",
    )
    ap.add_argument(
        "--ported-only",
        action="store_true",
        help="filter: only addresses with a // PORT: tag",
    )
    ap.add_argument(
        "--ignored-only",
        action="store_true",
        help="filter: only addresses in scripts/ci/port-catalog-ignore.toml",
    )
    ap.add_argument(
        "--include-ignored",
        action="store_true",
        help="don't exclude ignore-list entries from --missing-ports (default: exclude)",
    )
    ap.add_argument(
        "--bucket",
        choices=["scus", "overlay"],
        help="filter: only SCUS-resident or only overlay-resident addresses",
    )
    ap.add_argument(
        "--no-write",
        action="store_true",
        help="don't write CSV/MD artifacts to target/port-catalog/",
    )
    ap.add_argument(
        "--feature",
        type=str,
        default="",
        help="filter to addresses reachable from a feature's roots (see scripts/ci/features.toml)",
    )
    ap.add_argument(
        "--list-features",
        action="store_true",
        help="list available features in scripts/ci/features.toml and exit",
    )
    ap.add_argument(
        "--dashboard",
        action="store_true",
        help="emit open-work rollup (per-feature stats + top-N missing-ports + "
        "ignore summary) to target/port-catalog/open-work.md and exit",
    )
    ap.add_argument(
        "--dashboard-top",
        type=int,
        default=10,
        help="per-feature top-N missing-ports cap for --dashboard (default: 10)",
    )
    args = ap.parse_args()

    if args.list_features:
        features = load_features()
        if not features:
            print(f"no features found in {FEATURES_TOML}")
            return 0
        print(f"features in {FEATURES_TOML}:")
        for name, body in features.items():
            roots = ", ".join(body["roots"]) or "(none)"
            stop_at = (
                f"  stop_at: {', '.join(sorted(body['stop_at']))}\n"
                if body["stop_at"]
                else ""
            )
            depth = (
                f"  max_depth: {body['max_depth']}\n"
                if body["max_depth"] is not None
                else ""
            )
            print(f"\n[{name}]")
            print(f"  description: {body['description']}")
            print(f"  roots: {roots}")
            if stop_at:
                print(stop_at.rstrip())
            if depth:
                print(depth.rstrip())
        return 0

    dumped = collect_dumped()
    refs, sources = collect_citations()
    docs = collect_doc_citations()
    ports = collect_ports()
    ignore = load_ignore()

    rows = build_rows(dumped, refs, sources, docs, ports, ignore=ignore)

    # Always write the global catalog so the latest state is on disk even when
    # the user is also drilling into a feature filter.
    if not args.no_write:
        render_csv(rows, OUT_DIR / "catalog.csv")
        render_md(rows, OUT_DIR / "catalog.md", "Port catalog (global)")

    if args.dashboard:
        features = load_features()
        out_path = None if args.no_write else OUT_DIR / "open-work.md"
        md = render_dashboard(rows, sources, features, args.dashboard_top, out_path)
        if args.md:
            print(md)
        else:
            print(summarize(rows))
            if out_path:
                print()
                print(f"wrote {out_path}")
        return 0

    # Feature filter trims rows to BFS-reachable addresses + writes a
    # per-feature artifact under target/port-catalog/<feature>.{csv,md}.
    feature_meta: dict | None = None
    if args.feature:
        features = load_features()
        if args.feature not in features:
            print(
                f"unknown feature: {args.feature!r}. "
                f"Available: {', '.join(features) or '(none — populate scripts/ci/features.toml)'}"
            )
            return 1
        feature_meta = features[args.feature]
        graph = build_call_graph(sources)
        reachable = bfs_reachable(
            graph,
            feature_meta["roots"],
            stop_at=feature_meta["stop_at"],
            max_depth=feature_meta["max_depth"],
        )
        rows = [r for r in rows if r["addr"] in reachable]
        if not args.no_write:
            title = f"Port catalog — feature: {args.feature}"
            render_csv(rows, OUT_DIR / f"{args.feature}.csv")
            render_md(rows, OUT_DIR / f"{args.feature}.md", title)

    filtered = filter_rows(rows, args)

    if args.md:
        title = "Port catalog"
        if args.feature:
            title += f" — feature: {args.feature}"
        if args.missing_ports:
            title += " — port worklist (dumped + documented, not ported)"
        elif args.missing_dumps:
            title += " — dump worklist (cited, not dumped)"
        elif args.ported_only:
            title += " — ported only"
        elif args.addr:
            title += f" — {args.addr}"
        print(render_md(filtered, None, title))
        return 0

    if feature_meta is not None:
        print(f"[feature: {args.feature}]")
        print(f"  description: {feature_meta['description']}")
        print(f"  roots:       {', '.join(feature_meta['roots']) or '(none)'}")
        if feature_meta["stop_at"]:
            print(f"  stop_at:     {', '.join(sorted(feature_meta['stop_at']))}")
        print(f"  reachable:   {len(rows)} addresses")
        print()
    print(summarize(rows))
    if filtered != rows:
        print()
        print(f"filtered rows ({len(filtered)}):")
        print(f"{'addr':<10} {'bucket':<8} {'D/d/P/I':<8} {'refs':>4}  port crates / ignore / first src")
        print(f"{'-' * 10} {'-' * 8} {'-' * 8} {'-' * 4}  {'-' * 60}")
        for r in filtered:
            flags = (
                ("D" if r["dumped"] else ".")
                + ("d" if r["documented"] else ".")
                + ("P" if r["ported"] else ".")
                + ("I" if r["ignored"] else ".")
            )
            crates = ",".join(r["port_crates"]) if r["port_crates"] else "—"
            if r["ported"]:
                tail = crates
            elif r["ignored"]:
                tail = f"[{r['ignore_category']}] {r['ignore_reason']}"
            else:
                tail = r["first_sources"][0] if r["first_sources"] else "—"
            print(f"{r['addr']:<10} {r['bucket']:<8} {flags:<8} {r['refs']:>4}  {tail}")
    if not args.no_write:
        print()
        print(f"wrote {OUT_DIR / 'catalog.csv'}")
        print(f"wrote {OUT_DIR / 'catalog.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
