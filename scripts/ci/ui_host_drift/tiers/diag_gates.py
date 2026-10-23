"""Tier 6 analysis: is every additive debug draw off by default on both hosts?

Rows: `ui_host_drift.rows.diag_gates`.
"""

import re

from ui_host_drift.roots import REPO
from ui_host_drift.rows.diag_gates import DIAG_GATES, DIAG_ROOTS

# Where this tier's rows are declared, as its own messages cite it.
ROWS_FILE = "scripts/ci/ui_host_drift/rows/diag_gates.py"

DIAG_ENV_RE = re.compile(r'"(LEGAIA_DIAG_[A-Z0-9_]+)"')


def discover_diag_gates() -> set[str]:
    """Every `LEGAIA_DIAG_*` name appearing as a string literal in the crates."""
    found: set[str] = set()
    for root in DIAG_ROOTS:
        if not root.exists():
            continue
        for path in root.rglob("*.rs"):
            found.update(DIAG_ENV_RE.findall(path.read_text(encoding="utf-8", errors="replace")))
    return found


def initialiser_is_off(init: str) -> bool:
    """Does a toggle's initialiser text mean "off"?

    Split out of [`web_toggle_defaults_off`] so it can be run against synthetic
    inputs - a checker that only ever sees the one real file cannot show it
    would notice the file changing.
    """
    return bool(re.search(r"\bnew\s*\(\s*false\s*\)|^false$", init))


def web_toggle_defaults_off(symbol: str) -> tuple[bool, str]:
    """Does `symbol` exist in web-play / web-viewer and initialise to false?

    Returns `(ok, detail)`. The initialiser test is the whole point: a toggle
    that exists but defaults on is the defect this tier was written for,
    wearing the shape of the fix.
    """
    pattern = re.compile(
        r"\b" + re.escape(symbol) + r"\b[^=;]*=\s*([^;]+);",
        re.S,
    )
    web_roots = (
        REPO / "crates" / "web-play" / "src",
        REPO / "crates" / "web-minigames" / "src",
        REPO / "crates" / "web-viewer" / "src",
        REPO / "crates" / "web-patcher" / "src",
    )
    for path in (p for root in web_roots for p in sorted(root.rglob("*.rs"))):
        text = path.read_text(encoding="utf-8", errors="replace")
        m = pattern.search(text)
        if not m:
            continue
        init = " ".join(m.group(1).split())
        if initialiser_is_off(init):
            return True, f"{path.relative_to(REPO)}: {init}"
        return False, f"{path.relative_to(REPO)}: initialiser is `{init}`, expected false"
    return False, f"no `{symbol}` found in the browser crates' sources"


def check_diag_gates() -> tuple[list[str], int]:
    """Tier 6: an additive diagnostic must be default-off on BOTH hosts.

    Returns `(problems, additive_count)`.
    """
    problems: list[str] = []
    declared = {str(g["env"]): g for g in DIAG_GATES}
    found = discover_diag_gates()

    for env in sorted(found - set(declared)):
        problems.append(
            f"UNDECLARED DIAG GATE {env}: add it to DIAG_GATES in "
            f"{ROWS_FILE} and say whether it is `additive` (draws "
            f"something retail does not). An additive gate needs a default-off "
            f"twin in crates/web-play, or the browser draws it always."
        )
    for env in sorted(set(declared) - found):
        problems.append(
            f"STALE DIAG GATE {env}: declared in DIAG_GATES but no longer "
            f"appears in any engine crate. Drop the row."
        )

    additive = 0
    for env, gate in sorted(declared.items()):
        if not gate.get("additive"):
            continue
        additive += 1
        toggle = gate.get("web_toggle")
        if toggle is None:
            if not str(gate.get("waiver", "")).strip():
                problems.append(
                    f"{env}: additive with no `web_toggle` needs a `waiver` "
                    f"saying why the browser cannot draw it."
                )
            continue
        ok, detail = web_toggle_defaults_off(str(toggle))
        if not ok:
            problems.append(
                f"DIAG DRIFT {env}: its browser twin `{toggle}` is not "
                f"default-off - {detail}. The native gate suppresses a draw "
                f"retail never makes; a browser that cannot suppress it "
                f"stamps that draw on every user's frame."
            )
    return problems, additive
