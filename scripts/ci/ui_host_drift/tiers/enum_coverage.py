"""Tier 9 analysis: does each host answer every variant of a shared enum?

Rows: `ui_host_drift.rows.enum_coverage`.
"""

import re

from ui_host_drift.roots import REPO
from ui_host_drift.rows.enum_coverage import ENUM_COVERAGE
from ui_host_drift.rust_text import BLOCK_COMMENT_RE, strip_comments


def enum_variants(rel: str, name: str) -> list[str] | None:
    """Top-level variant names of `pub enum <name>` in `rel`, or None."""
    path = REPO / rel
    if not path.is_file():
        return None
    text = path.read_text(encoding="utf-8")
    m = re.search(rf"pub enum {name}\s*\{{", text)
    if not m:
        return None
    depth, i, body = 1, m.end(), []
    while i < len(text) and depth:
        c = text[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                break
        body.append(c)
        i += 1
    src = strip_comments(BLOCK_COMMENT_RE.sub("", "".join(body)))
    out: list[str] = []
    nest = 0
    for line in src.splitlines():
        stripped = line.strip()
        if nest == 0:
            hit = re.match(r"^([A-Z][A-Za-z0-9]*)\s*(?:\{|\(|,|=|$)", stripped)
            if hit:
                out.append(hit.group(1))
        nest += line.count("{") + line.count("(") - line.count("}") - line.count(")")
    return out


def check_enum_coverage(
    shipped: dict[str, list[tuple[str, str]]],
) -> tuple[list[str], list[str], int]:
    """Every [`ENUM_COVERAGE`] row against both hosts. `(problems, pending, n)`."""
    problems: list[str] = []
    pending: list[str] = []
    checked = 0
    for row in ENUM_COVERAGE:
        name = str(row["enum"])
        variants = enum_variants(str(row["source"]), name)
        if not variants:
            problems.append(
                f"STALE ENUM ROW {name}: {row['source']} defines no such enum "
                f"(renamed or moved?). Re-point or drop the row."
            )
            continue
        waivers: dict = row.get("waivers", {})  # type: ignore[assignment]
        seen: set[tuple[str, str]] = set()
        for host, rows in shipped.items():
            blob = "\n".join(text for _rel, text in rows)
            for variant in variants:
                checked += 1
                if re.search(rf"\b{name}::{variant}\b", blob):
                    if (variant, host) in waivers:
                        problems.append(
                            f"STALE ENUM WAIVER {name}::{variant} ({host}): the host "
                            f"names the variant now. Drop the waiver."
                        )
                        seen.add((variant, host))
                    continue
                entry = waivers.get((variant, host))
                if entry is None:
                    problems.append(
                        f"UNANSWERED VARIANT {name}::{variant} ({host}): no shipped "
                        f"source on this host names it ({row['why']}). Answer it, or "
                        f"waive the pair with a reason."
                    )
                    continue
                seen.add((variant, host))
                pending.append(
                    f"{name}::{variant} ({host}) - {entry.get('blocked_on') or entry.get('reason')}"
                )
        for key in waivers:
            if key not in seen:
                problems.append(
                    f"STALE ENUM WAIVER {name}::{key[0]} ({key[1]}): no such variant "
                    f"or host. Drop the waiver."
                )
    return problems, pending, checked
