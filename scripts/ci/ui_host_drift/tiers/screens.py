"""Tier 1: the engine-ui draw-builder surface and which hosts reach it.

No row table: the surface is discovered. Waivers live in
`scripts/ci/ui-host-drift-waivers.toml`.
"""

import re

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    import tomli as tomllib  # type: ignore[no-redef]

from ui_host_drift.roots import HOSTS, REPO, SHARED_COMPOSERS, UI_SRCS, WAIVERS
from ui_host_drift.rust_text import (
    fn_body,
    is_test_source,
    signature_end,
    strip_comments,
    word_set,
)

# A draw builder is a public fn whose return type mentions one of engine-ui's
# own draw-record types - that is exactly "projects a view into
# renderer-agnostic geometry", i.e. one screen's (or one screen fragment's)
# layout.
#
# `TextDraw` / `SpriteDraw` are the terminal records a renderer consumes.
# The rest are engine-ui's *intermediate* records - a resolved screen that
# still needs a host-owned atlas or font to become quads:
#
#   HudDraw           the fishing HUD's item list (fishing::hud)
#   HudQuad           a PROT 0977 textured-Gouraud quad (muscle_dome_hud)
#   DigitCell         one placed digit of a numeric readout (fishing::hud)
#   BarFrame          a resolved gauge bar (fishing::hud)
#   ComparePanelField one field of an equip stat-compare panel
#
# They belong on the surface for the same reason the terminal records do: a
# host "has" the screen exactly when it reaches the projection, and which
# record type the projection stops at is an engine-ui implementation detail.
# Leaving them off is not a smaller claim, it is a silent one - the whole
# window-25 compare-panel chain and the whole fishing HUD were invisible to
# this gate while every other pause screen was covered.
#
# Signatures here are routinely multi-line, so the return type is read from
# the span between the fn keyword and the opening brace of the body rather
# than from a single-line pattern.
DRAW_RECORDS = "TextDraw|SpriteDraw|HudDraw|HudQuad|DigitCell|BarFrame|ComparePanelField"

# One more return shape, and it is a shape rather than a named record: a
# builder whose projection stops at a **screen rect**. Two live in engine-ui -
# `painter_rect` (descriptor -> `PainterRect`) and `guarded_box_rect`
# (`FUN_801E4140`'s bottom-clip guard, `-> Option<(i32, i32, i32, i32)>`) -
# and neither was watched: the record-name rule never matches a bare tuple,
# and `PainterRect` was not on the list, so the gate would not count them as
# screens and would not accept a waiver for them either. That is the worst of
# the three outcomes - not "wired", not "waived", but *silent*: a rect builder
# could lose its only host caller with no line of output changing, which is
# the exact failure the orphan naming above was added to stop.
#
# Kept out of [`DRAW_RECORDS`] on purpose. That constant also feeds
# [`TRANSFORM_PARAM_RE`], whose job is "this fn consumes what screens
# produce", and a rect is the commonest *model* a real screen takes - the
# module docstring says so ("every real screen takes a model (a session, a row
# list, a rect, a font layout)"). Folding rects into the transform test would
# reclassify most of the pause-menu painter chain as plumbing.
RECT_RETURNS = r"PainterRect|\(\s*i32\s*,\s*i32\s*,\s*i32\s*,\s*i32\s*\)"

BUILDER_RE = re.compile(r"^pub fn (?P<name>[a-z0-9_]+)\s*[<(]", re.MULTILINE)
DRAW_RET_RE = re.compile(rf"->[^;{{]*(?:{DRAW_RECORDS}|{RECT_RETURNS})")

# Every `fn` engine-ui defines, at any indentation: free functions, `impl`
# methods and private helpers alike. These are the nodes of the internal call
# graph. Only builders are ever classified; the rest exist so a composition
# that runs through a method is not mistaken for an unused screen.
ANY_FN_RE = re.compile(r"\bfn\s+(?P<name>[a-z0-9_]+)\s*[<(]")

# ...but returning quads is not sufficient. A function that *takes* draw
# records and hands them back is a batching transform inside the draw
# pipeline, not a projection of a model into a screen. Counting those as
# screens is a defect of the instrument: nothing about a host "having" or
# "not having" one describes a gap between the two hosts, and the surface
# they inflate is exactly the surface a waiver file then has to explain.
#
# The shape the return-type rule alone could not see: `sprite_draws_for(
# requests: &[SpriteRequest], anchor)` - draw records in, draw records out,
# no model anywhere in the signature. It read as an unwired screen for as
# long as the gate has existed, and the waiver written for it says so in
# prose ("a generic anchor-translate helper ... not a screen").
#
# Deliberately keyed on the crate's own draw/request record types appearing
# in the PARAMETER list, which is the narrowest statement of "this consumes
# the thing screens produce". Over the current surface it reclassifies
# exactly one function; every real screen takes a model (a session, a row
# list, a rect, a font layout) and is untouched. `--selftest` pins both
# directions.
#
# The intermediate records count here too, and the case that proves it is
# `fishing_hud_draws_for(font, items: &[HudDraw], captions, atlas, origin)
# -> Vec<TextDraw>`. HudDraw records in, TextDraw records out: it is the
# fishing HUD's *renderer*, and the screens are `persistent_hud_draws` /
# `catch_hud_draws`, which take a model and return the item list. Counting the
# renderer as a screen would have reported DRIFT, since the browser host walks
# the same `HudDraw` list through its own projection - a true observation
# about the rendering half, which this gate has already said it cannot decide,
# reported in the one bucket that means "a screen is missing".
TRANSFORM_PARAM_RE = re.compile(rf"\b(?:SpriteRequest|{DRAW_RECORDS})\b")


def is_screen_signature(signature: str) -> bool:
    """Is this `fn` signature one screen's geometry builder?

    `signature` is the source span from the `fn` keyword up to (not
    including) the body's opening brace. Two conditions, and the second is
    the one a return-type-only rule was missing:

    1. it returns quads (`TextDraw` / `SpriteDraw`), and
    2. it does **not** take quads - a function fed the crate's own draw or
       request records is a transform over a draw list, not a projection of
       a model into one.
    """
    if not DRAW_RET_RE.search(signature):
        return False
    arrow = signature.rfind("->")
    params = signature[:arrow] if arrow >= 0 else signature
    return not TRANSFORM_PARAM_RE.search(params)


def collect_builders() -> dict[str, str]:
    """Map builder name -> `path:line` where it is defined."""
    out: dict[str, str] = {}
    for path in sorted(p for r in UI_SRCS for p in r.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for m in BUILDER_RE.finditer(text):
            # The signature runs from the fn keyword to the body's opening
            # brace; anything past that is the body and must not be sniffed
            # for a return type.
            brace = signature_end(text, m.start())
            if brace < 0:
                continue
            if not is_screen_signature(text[m.start() : brace]):
                continue
            line = text[: m.start()].count("\n") + 1
            rel = path.relative_to(REPO)
            out[m.group("name")] = f"{rel}:{line}"
    return out


def collect_fn_names() -> set[str]:
    """Every `fn` name engine-ui defines, at any indentation."""
    out: set[str] = set()
    for path in sorted(p for r in UI_SRCS for p in r.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for m in ANY_FN_RE.finditer(text):
            out.add(m.group("name"))
    return out


def collect_call_graph(names: set[str]) -> dict[str, set[str]]:
    """Map engine-ui fn name -> the engine-ui fn names its body references.

    This is the engine-ui-internal half of the call graph. It spans free
    functions, `impl` methods and private helpers, because a composition edge
    is an edge wherever it is written - see the module docstring for the six
    builders a builder-only graph reported as unused while both hosts drew
    them.

    Nodes are keyed by bare name, so two `impl`s that both define `service`
    merge into one node. That over-counts "used", which is the safe direction
    this file's `is_test_source` docstring argues for at length: the failure
    it can produce is a nag about a screen that is in fact wired, never a
    waiver asserting a gap that does not exist.
    """
    refs: dict[str, set[str]] = {n: set() for n in names}
    for path in sorted(p for r in UI_SRCS for p in r.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for m in ANY_FN_RE.finditer(text):
            name = m.group("name")
            if name not in refs:
                continue
            brace = signature_end(text, m.start())
            if brace < 0:
                continue
            body = strip_comments(fn_body(text, brace))
            refs[name] |= word_set(body) & names
            refs[name].discard(name)
    return refs


def seed_transitively(uses: dict[str, set[str]], refs: dict[str, set[str]]) -> None:
    """Propagate each host label along the builder call graph, to a fixpoint.

    If a host draws builder A and A composes builder B, the host draws B. Cycles
    are harmless - the loop only ever adds labels, so it terminates.
    """
    changed = True
    while changed:
        changed = False
        for name, callees in refs.items():
            hosts = uses.get(name)
            if not hosts:
                continue
            for callee in callees:
                if not hosts <= uses[callee]:
                    uses[callee] |= hosts
                    changed = True


def collect_uses(names: set[str]) -> dict[str, set[str]]:
    """Map builder name -> set of host labels that call it."""
    uses: dict[str, set[str]] = {n: set() for n in names}
    for host, roots in HOSTS.items():
        for root in roots:
            if not root.is_dir():
                continue
            for path in root.rglob("*.rs"):
                if is_test_source(path):
                    continue
                body = strip_comments(path.read_text(encoding="utf-8"))
                for name in word_set(body) & names:
                    uses[name].add(host)
    for root, entries in SHARED_COMPOSERS.items():
        callers = composer_callers(entries)
        if not callers or not root.is_dir():
            continue
        for path in root.rglob("*.rs"):
            if is_test_source(path):
                continue
            body = strip_comments(path.read_text(encoding="utf-8"))
            for name in word_set(body) & names:
                uses[name] |= callers
    return uses


def composer_callers(entries: tuple[str, ...]) -> set[str]:
    """Host labels whose shipped sources call one of a composer's entries.

    A CALL (`entry(` after `.` / `::`), not a mention: a host defining a
    same-named wrapper method does not count unless it also calls through."""
    pattern = re.compile(r"[.:]\s*(?:" + "|".join(map(re.escape, entries)) + r")\s*\(")
    out: set[str] = set()
    for host, roots in HOSTS.items():
        for root in roots:
            if not root.is_dir():
                continue
            for path in root.rglob("*.rs"):
                if is_test_source(path):
                    continue
                if pattern.search(strip_comments(path.read_text(encoding="utf-8"))):
                    out.add(host)
    return out


def load_waivers() -> dict[str, dict]:
    if not WAIVERS.is_file():
        return {}
    data = tomllib.loads(WAIVERS.read_text(encoding="utf-8"))
    out: dict[str, dict] = {}
    for entry in data.get("waiver", []):
        name = entry.get("builder")
        if name:
            out[name] = entry
    return out
