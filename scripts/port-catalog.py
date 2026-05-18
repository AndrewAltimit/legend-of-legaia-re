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

PORT tag format (Rust source files under crates/):

    // PORT: FUN_801dd35c                  -- single
    // PORT: FUN_801dd35c, FUN_801cf244    -- multiple on one line
    // PORT: FUN_801dd35c (sub-mode jump table)   -- trailing context allowed

Whitespace and the `// PORT:` prefix are matched leniently; the addresses are
matched strictly to the SCUS/overlay range so unrelated `// PORT: 80...` text
in unrelated contexts is unlikely to false-positive.

Usage:
    python3 scripts/port-catalog.py                       # build global catalog
    python3 scripts/port-catalog.py --missing-ports       # dumped+documented, not ported
    python3 scripts/port-catalog.py --missing-dumps       # cited but not dumped
    python3 scripts/port-catalog.py --md                  # markdown table to stdout
    python3 scripts/port-catalog.py --top 30              # cap rows
    python3 scripts/port-catalog.py --addr 801dd35c       # single-row drill-down

Output (default): target/port-catalog/catalog.csv + catalog.md
"""

import argparse
import csv
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
FUNCS_DIR = REPO / "ghidra" / "scripts" / "funcs"
DOCS_DIR = REPO / "docs"
CRATES_DIR = REPO / "crates"
OUT_DIR = REPO / "target" / "port-catalog"

# Address ranges that correspond to executable code:
#   SCUS_942.54   : 0x80010000 - 0x8006FFFF
#   Overlays      : 0x801C0000 - 0x8020FFFF
# Match the same shape function-coverage.py uses so the two tools share a worldview.
# IGNORECASE so PORT tags written `FUN_801DD35C` (uppercase) match as cleanly
# as the lowercase form Ghidra emits.
CODE_ADDR_RE = re.compile(r"80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4}", re.IGNORECASE)

# Citations of an address by some other piece of text. Covers Ghidra's auto-named
# call forms plus raw disassembly forms. Matches `function-coverage.py`.
CITATION_RE = re.compile(
    r"(?:FUN_|func_0x|jal\s+0x|jalr\s+\w+,0x|->\s*func_0x)"
    r"(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})"
)

# Doc-side citation. More permissive: doc text uses `FUN_8003EBE4` (caps) and
# also bare backtick-wrapped `0x8003e4e8` forms.
DOC_CITATION_RE = re.compile(
    r"(?:FUN_|0x)(80(?:0[1-6]|1[cdef]|20)[0-9a-fA-F]{4})",
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
            # Inside the tail of the tag, pick up every code-range address.
            for m in CODE_ADDR_RE.finditer(tag.group(1)):
                addr = m.group(0).lower()
                out[addr].add(crate)
    return out


def build_rows(
    dumped: dict[str, str],
    refs: Counter,
    sources: dict[str, list[str]],
    docs: dict[str, set[str]],
    ports: dict[str, set[str]],
) -> list[dict]:
    """Union the four signals into a per-address row list, sorted by address."""
    addrs = set(dumped) | set(refs) | set(docs) | set(ports)
    rows: list[dict] = []
    for addr in sorted(addrs):
        dump_stem = dumped.get(addr, "")
        is_dumped = bool(dump_stem)
        doc_hits = sorted(docs.get(addr, set()))
        port_crates = sorted(ports.get(addr, set()))
        is_documented = bool(doc_hits)
        is_ported = bool(port_crates)
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
        "Generated by `scripts/port-catalog.py`. Three independent status columns:",
        "",
        "- **dumped** — Ghidra decompiler output exists under `ghidra/scripts/funcs/`.",
        "- **documented** — the address is cited from at least one file under `docs/`.",
        "- **ported** — the address appears in a `// PORT: FUN_<addr>` tag in a Rust source under `crates/`.",
        "",
        "| addr | bucket | dumped | documented | ported (crates) | refs | first dump source |",
        "|---|---|---|---|---|---|---|",
    ]
    for r in rows:
        crates = ", ".join(r["port_crates"]) if r["port_crates"] else "—"
        first_src = r["first_sources"][0] if r["first_sources"] else "—"
        lines.append(
            f"| `{r['addr']}` | {r['bucket']} | {yesno(r['dumped'])} | "
            f"{yesno(r['documented'])} | {crates} | {r['refs']} | `{first_src}` |"
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
            f"dumped + documented, NOT ported (port worklist) : {dd_not_p}",
            "",
            f"cited but NOT dumped  (dump worklist)           : {cited_not_dumped}",
            f"ported but NOT documented (provenance gap)      : {ported_not_documented}",
            f"ported but NOT dumped     (provenance gap)      : {ported_not_dumped}",
        ]
    )


def filter_rows(rows: list[dict], args: argparse.Namespace) -> list[dict]:
    out = rows
    if args.addr:
        addr = args.addr.lower().removeprefix("0x")
        out = [r for r in out if r["addr"] == addr]
    if args.missing_ports:
        out = [
            r for r in out if r["dumped"] and r["documented"] and not r["ported"]
        ]
        out.sort(key=lambda r: (-r["refs"], r["addr"]))
    if args.missing_dumps:
        out = [r for r in out if r["refs"] > 0 and not r["dumped"]]
        out.sort(key=lambda r: (-r["refs"], r["addr"]))
    if args.ported_only:
        out = [r for r in out if r["ported"]]
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
        "--bucket",
        choices=["scus", "overlay"],
        help="filter: only SCUS-resident or only overlay-resident addresses",
    )
    ap.add_argument(
        "--no-write",
        action="store_true",
        help="don't write CSV/MD artifacts to target/port-catalog/",
    )
    args = ap.parse_args()

    dumped = collect_dumped()
    refs, sources = collect_citations()
    docs = collect_doc_citations()
    ports = collect_ports()

    rows = build_rows(dumped, refs, sources, docs, ports)

    if not args.no_write:
        render_csv(rows, OUT_DIR / "catalog.csv")
        render_md(rows, OUT_DIR / "catalog.md", "Port catalog (global)")

    filtered = filter_rows(rows, args)

    if args.md:
        title = "Port catalog"
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

    print(summarize(rows))
    if filtered != rows:
        print()
        print(f"filtered rows ({len(filtered)}):")
        print(f"{'addr':<10} {'bucket':<8} {'D/d/P':<6} {'refs':>4}  port crates / first src")
        print(f"{'-' * 10} {'-' * 8} {'-' * 6} {'-' * 4}  {'-' * 60}")
        for r in filtered:
            flags = (
                ("D" if r["dumped"] else ".")
                + ("d" if r["documented"] else ".")
                + ("P" if r["ported"] else ".")
            )
            crates = ",".join(r["port_crates"]) if r["port_crates"] else "—"
            tail = crates if r["ported"] else (r["first_sources"][0] if r["first_sources"] else "—")
            print(f"{r['addr']:<10} {r['bucket']:<8} {flags:<6} {r['refs']:>4}  {tail}")
    if not args.no_write:
        print()
        print(f"wrote {OUT_DIR / 'catalog.csv'}")
        print(f"wrote {OUT_DIR / 'catalog.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
