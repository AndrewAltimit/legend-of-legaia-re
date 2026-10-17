"""Tier 5 analysis: does a page carry its own keyboard table?

Rows: `ui_host_drift.rows.page_keys`.
"""

import re
from pathlib import Path

from ui_host_drift.roots import REPO
from ui_host_drift.rows.page_keys import (
    GAMEPAD_ADOPTER,
    NON_PAD_CODES,
    PAD_HOST_MARKERS,
    SITE_ROOT,
)
from ui_host_drift.rust_text import BLOCK_COMMENT_RE, strip_comments

# `KeyboardEvent` accesses: an event-shaped identifier, not `g.key` on a
# plain object (the play page's scene-group records have a `key` field, and
# `kbd.dataset.key` is an attribute).
EVENT_KEY_RE = re.compile(r"\b(?:e|ev|evt|event)\.key\b")
EVENT_CODE_LITERAL_RE = re.compile(
    r"\b(?:e|ev|evt|event)\.code\s*[=!]==?\s*['\"]([^'\"]*)['\"]"
)

# A page may also stash key codes in a Set and later ask `held.has('Enter')`.
# The literal never touches `event.code`, so the two detectors above are blind
# to it - and that is not hypothetical: the play page decided whether Start was
# pressed with `p.has('Enter')`, which meant binding Start to Space bound it
# everywhere except the one handler that opens the pause menu. Dispatch must go
# through the pad BUTTON, so a bindable code in a set membership test is the
# same defect wearing a different shape.
KEY_SET_LITERAL_RE = re.compile(r"\.(?:has|includes)\(\s*['\"]([^'\"]+)['\"]\s*\)")


# The controller twin of a keyboard table. A gamepad layout written into a
# page does not look like a key table either: it is an array of PSX button
# names indexed by `Gamepad.buttons[i]`. The play page carried one while the
# native window read no controller at all, so nothing compared it with
# anything. Four or more quoted button names in one array literal is that
# shape; the engine serves the real one (`gamepad_layout_json`).
PAD_BUTTON_NAMES = (
    "Select|L3|R3|Start|Up|Right|Down|Left|L2|R2|L1|R1|Triangle|Circle|Cross|Square"
)
BUTTON_NAME_ARRAY_RE = re.compile(
    rf"\[\s*(?:['\"](?:{PAD_BUTTON_NAMES})['\"]\s*,\s*){{3,}}['\"](?:{PAD_BUTTON_NAMES})['\"]"
)
# Reading the Gamepad API outside the adopter is a second controller reader:
# it would fold the buttons through whatever it wrote down, not the engine's
# layout, and skip the focus-loss reset the hosts share.
GAMEPAD_API_RE = re.compile(r"\bnavigator\.getGamepads\b")


def controller_table_hits(text: str, is_adopter: bool) -> list[tuple[int, str]]:
    """Tier-5 controller detectors over comment-stripped `text`:
    `(offset, kind)` per hit, `kind` one of `"table"` / `"api"`."""
    hits = [(m.start(), "table") for m in BUTTON_NAME_ARRAY_RE.finditer(text)]
    if not is_adopter:
        hits.extend((m.start(), "api") for m in GAMEPAD_API_RE.finditer(text))
    return sorted(hits)


def bindable_dom_codes() -> set[str]:
    """The `KeyboardEvent.code`s the engine's own vocabulary binds.

    Parsed from `KEY_NAME_DOM_CODES` in `crates/engine-system/src/input.rs` rather
    than restated here, so this gate cannot drift from the table it polices.
    An unreadable table yields an empty set, which disables only this detector.
    """
    src = REPO / "crates" / "engine-system" / "src" / "input.rs"
    if not src.is_file():
        return set()
    text = src.read_text(encoding="utf-8", errors="replace")
    m = re.search(r"KEY_NAME_DOM_CODES[^=]*=\s*\[(.*?)\];", text, re.DOTALL)
    if not m:
        return set()
    return {c for _, c in re.findall(r"\(\s*\"([^\"]+)\"\s*,\s*\"([^\"]+)\"\s*\)", m.group(1))}


def pad_host_sources() -> list[Path]:
    """Every `site/` source that reaches the engine's pad surface."""
    if not SITE_ROOT.is_dir():
        return []
    out = []
    for path in sorted(SITE_ROOT.rglob("*")):
        if path.suffix not in (".js", ".html") or not path.is_file():
            continue
        if "wasm" in path.parts:
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        if any(m in text for m in PAD_HOST_MARKERS):
            out.append(path)
    return out


def check_page_key_tables() -> tuple[list[str], int]:
    """Tier 5. Returns `(problems, files_scanned)`."""
    problems: list[str] = []
    sources = pad_host_sources()
    for path in sources:
        rel = path.relative_to(REPO)
        text = strip_comments(BLOCK_COMMENT_RE.sub(" ", path.read_text(encoding="utf-8")))
        for m in EVENT_KEY_RE.finditer(text):
            line = text.count("\n", 0, m.start()) + 1
            problems.append(
                f"PAGE KEY TABLE {rel}:{line}: reads `KeyboardEvent.key`. "
                f"A pad-driving page must resolve `event.code` through the "
                f"engine's table (`pad_bindings_json` / `legaiaPadButtonOf`) "
                f"and dispatch on the pad BUTTON - `.key` is the "
                f"layout-dependent character and cannot be reconciled with a "
                f"binding."
            )
        for m in EVENT_CODE_LITERAL_RE.finditer(text):
            code = m.group(1)
            if code in NON_PAD_CODES:
                continue
            line = text.count("\n", 0, m.start()) + 1
            problems.append(
                f"PAGE KEY TABLE {rel}:{line}: compares `event.code` to "
                f"{code!r}, which the engine may bind. Resolve it through "
                f"`legaiaPadButtonOf` and dispatch on the button, or - if it "
                f"really is not a pad control - use a key the pad has no "
                f"button for (NON_PAD_CODES in this gate)."
            )
        for pos, kind in controller_table_hits(text, path == GAMEPAD_ADOPTER):
            line = text.count("\n", 0, pos) + 1
            if kind == "table":
                problems.append(
                    f"PAGE CONTROLLER TABLE {rel}:{line}: an array of PSX "
                    f"button names - a gamepad layout written into the page. "
                    f"Fold controllers through the engine's layout "
                    f"(`gamepad_layout_json`, adopted by `site/js/pad-bindings.js`), "
                    f"the one the native window folds gilrs through."
                )
            else:
                problems.append(
                    f"PAGE CONTROLLER TABLE {rel}:{line}: reads "
                    f"`navigator.getGamepads` directly. Read controllers through "
                    f"`legaiaPollGamepad` / `legaiaGamepadEdges` "
                    f"(`site/js/pad-bindings.js`), which fold them through the "
                    f"engine's layout and share the focus-loss reset."
                )
        codes = bindable_dom_codes()
        for m in KEY_SET_LITERAL_RE.finditer(text):
            code = m.group(1)
            if code not in codes:
                continue
            line = text.count("\n", 0, m.start()) + 1
            problems.append(
                f"PAGE KEY TABLE {rel}:{line}: tests set membership of "
                f"{code!r}, a key the engine binds. Dispatch on the pad BUTTON "
                f"instead - e.g. `padMaskOf(pulse) & legaiaPadButton('Start')` "
                f"- so a key bound to that button anywhere reaches this handler "
                f"too, and a rebinding follows it."
            )
    return problems, len(sources)
