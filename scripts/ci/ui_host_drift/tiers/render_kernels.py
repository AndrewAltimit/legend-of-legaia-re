"""Tier 7 analysis: does every surface that assembles a draw list run its kernel?

Rows: `ui_host_drift.rows.render_kernels`.
"""

import re
from pathlib import Path

from ui_host_drift.roots import REPO
from ui_host_drift.rows.render_kernels import (
    RENDER_KERNEL_RULES,
    RENDER_ROOTS,
    RENDER_SURFACE_GROUPS,
)
from ui_host_drift.rust_text import is_test_source, strip_all_comments


def render_sources() -> list[Path]:
    """Every non-test render-surface source, `.rs` and `.js` alike."""
    out: list[Path] = []
    for root in RENDER_ROOTS:
        if not root.exists():
            continue
        for ext in ("*.rs", "*.js"):
            out.extend(p for p in root.rglob(ext) if not is_test_source(p))
    return sorted(out)


def rule_findings(rule: dict, text: str) -> list[str]:
    """Findings for one rule against one file's comment-stripped source.

    Empty when the file does not trigger, or triggers and complies. The
    `forbids` kind reports one entry per offending statement window so the
    output names the line, not just the file.
    """
    trigger = str(rule["trigger"])
    if not re.search(trigger, text):
        return []
    out: list[str] = []
    for pat in rule.get("requires", []):  # type: ignore[union-attr]
        if not re.search(str(pat), text):
            out.append(f"does not reach `{pat}`")
    forbids = rule.get("forbids")
    if forbids:
        lines = text.splitlines()
        for i in range(len(lines)):
            window = "\n".join(lines[max(0, i - 2) : i + 1])
            if re.search(str(forbids), window):
                out.append(f"line {i + 1}: {lines[i].strip()[:80]}")
    return out


def check_render_kernels() -> tuple[list[str], list[str], list[tuple[str, int, int, int]]]:
    """Tier 7. Returns (problems, disclosed-blocked notes, per-rule counts).

    A count row is `(kernel, running, blocked, exempt)`: how many surfaces
    assembling that draw list run the kernel, how many are disclosed as a
    divergence still being closed, and how many the rule provably does not
    apply to. The three are printed separately because collapsing them is how
    a matrix reads clean while a surface is missing.
    """
    problems: list[str] = []
    pending: list[str] = []
    counts: list[tuple[str, int, int, int]] = []
    sources = render_sources()
    texts = {p: strip_all_comments(p.read_text(encoding="utf-8", errors="ignore")) for p in sources}
    for group in RENDER_SURFACE_GROUPS:
        head, *rest = [REPO / rel for rel in group]
        if head not in texts:
            continue
        texts[head] = "\n".join([texts[head]] + [texts.pop(q) for q in rest if q in texts])
    for rule in RENDER_KERNEL_RULES:
        kernel = str(rule["kernel"])
        blocked: dict = rule.get("blocked_on", {})  # type: ignore[assignment]
        exempt: dict = rule.get("exempt", {})  # type: ignore[assignment]
        clean = 0
        seen_blocked: set[str] = set()
        seen_exempt: set[str] = set()
        for path, text in texts.items():
            rel = path.relative_to(REPO).as_posix()
            if not re.search(str(rule["trigger"]), text):
                continue
            findings = rule_findings(rule, text)
            if rel in exempt:
                seen_exempt.add(rel)
                if not findings:
                    problems.append(
                        f"STALE EXEMPT {kernel} / {rel}: the file now satisfies "
                        f"the rule, so the exemption claims nothing. Drop it."
                    )
                continue
            if not findings:
                clean += 1
                if rel in blocked:
                    seen_blocked.add(rel)
                    problems.append(
                        f"STALE BLOCKED {kernel} / {rel}: the divergence is "
                        f"closed. Drop the `blocked_on` entry."
                    )
                continue
            detail = "; ".join(findings)
            if rel in blocked:
                seen_blocked.add(rel)
                pending.append(f"{kernel} / {rel}: {blocked[rel]}")
                continue
            problems.append(
                f"RENDER KERNEL {kernel}: {rel} assembles this draw list but "
                f"{detail}. {rule['why']}."
            )
        for rel in blocked:
            if rel not in seen_blocked:
                problems.append(
                    f"STALE BLOCKED {kernel} / {rel}: the file no longer "
                    f"assembles this draw list (renamed or deleted?). Drop the "
                    f"`blocked_on` entry."
                )
        for rel in exempt:
            if rel not in seen_exempt:
                problems.append(
                    f"STALE EXEMPT {kernel} / {rel}: the file no longer "
                    f"assembles this draw list. Drop the exemption."
                )
        counts.append((kernel, clean, len(seen_blocked), len(seen_exempt)))
    return problems, pending, counts
