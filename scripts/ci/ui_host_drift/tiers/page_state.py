"""Tier 17 analysis: does the play page keep the GL state and frame order its
rows name?

Rows: `ui_host_drift.rows.page_state`.
"""

from ui_host_drift.roots import REPO
from ui_host_drift.rows.page_state import PAGE_STATE_RULES


def page_state_violation(src: str, rule: dict) -> bool:
    """Whether `src` breaks `rule`."""
    at = src.find(rule["source"])
    if at < 0:
        return True
    if rule["kind"] == "before":
        then = src.find(rule["then"])
        return then < 0 or then < at
    return False


def check_page_state() -> tuple[list[str], int]:
    """Tier 17. Returns `(problems, rules_checked)`."""
    problems: list[str] = []
    for rule in PAGE_STATE_RULES:
        src = rule["path"].read_text(encoding="utf-8")
        if page_state_violation(src, rule):
            problems.append(
                f"PAGE STATE {rule['path'].relative_to(REPO)}: {rule['what']}: "
                f"`{rule['source']}`. {rule['why']}."
            )
    return problems, len(PAGE_STATE_RULES)
