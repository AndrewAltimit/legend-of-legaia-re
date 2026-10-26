"""The catalog rows: one dict per address, the ignore list that classifies
them, and the command-line row filters.
"""

import argparse
import tomllib
from collections import Counter

from port_catalog.paths import IGNORE_TOML


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


# What an ignore row claims, by section. The four kinds carry different
# evidence and fail differently, so the summary reports them apart rather than
# under one "PsyQ" label (most rows are not library code):
#
# - library: a statically-linked PsyQ / BIOS / SDK routine.
# - address: no portable routine begins at the VA (interior PC, shared tail,
#   phantom, label, data, mis-based print, VA alias). Audited against the
#   extracted images by `classify-worklist.py --audit-ignored`.
# - unreferenced: a real routine that nothing on the disc reaches.
# - scope: a real game routine the port covers another way (or a foreign-build
#   image this disc never loads). The kind a new section falls into by default,
#   because it is the one whose reason a reader must check.
LIBRARY_IGNORE_SECTIONS = frozenset(
    {
        "bios",
        "libc",
        "libgte",
        "libcd",
        "libgs",
        "libgpu",
        "libetc",
        "libapi",
        "libsnd",
        "libsnd_seq_volume",
        "libspu",
        "joy_serial",
        "kernel_callback",
    }
)
ADDRESS_IGNORE_SECTIONS = frozenset(
    {
        "field_vm_labels",
        "overlay_epilogue_labels",
        "ghidra_phantoms",
        "va_aliased_overlay_local",
    }
)
IGNORE_KINDS = ("library", "address", "unreferenced", "scope")


def ignore_kind(category: str) -> str:
    """Classify an ignore-list section into one of `IGNORE_KINDS`."""
    if category in LIBRARY_IGNORE_SECTIONS:
        return "library"
    if category.startswith("worklist_") or category in ADDRESS_IGNORE_SECTIONS:
        return "address"
    if category == "unreferenced" or category.startswith("unreferenced_"):
        return "unreferenced"
    return "scope"


def build_rows(
    dumped: dict[str, str],
    refs: Counter,
    sources: dict[str, list[str]],
    docs: dict[str, set[str]],
    ports: dict[str, set[str]],
    ignore: dict[str, tuple[str, str]] | None = None,
    live: dict[str, dict] | None = None,
) -> list[dict]:
    """Union the four signals into a per-address row list, sorted by address."""
    addrs = set(dumped) | set(refs) | set(docs) | set(ports)
    ignore = ignore or {}
    live = live if live is not None else {}
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
        lv = live.get(addr)
        rows.append(
            {
                "addr": addr,
                "live": bool(lv and lv["live"]),
                "live_known": lv is not None,
                "not_wired_tag": bool(lv and lv["not_wired_tag"]),
                "replaced_tag": bool(lv and lv["replaced_tag"]),
                "anchors": lv["anchors"] if lv else [],
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
        if not args.include_ignored:
            out = [r for r in out if not r["ignored"]]
        out.sort(key=lambda r: (-r["refs"], r["addr"]))
    if args.ported_only:
        out = [r for r in out if r["ported"]]
    if args.ignored_only:
        out = [r for r in out if r["ignored"]]
        out.sort(key=lambda r: (r["ignore_category"], r["addr"]))
    if args.not_live:
        out = [r for r in out if r["ported"] and r["live_known"] and not r["live"]]
        out.sort(key=lambda r: (r["not_wired_tag"], r["addr"]))
    if args.live_only:
        out = [r for r in out if r["ported"] and r["live"]]
    if args.bucket:
        out = [r for r in out if r["bucket"] == args.bucket]
    if args.top:
        out = out[: args.top]
    return out
