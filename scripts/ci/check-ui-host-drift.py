#!/usr/bin/env python3
"""UI host-drift checker: does every shared screen reach BOTH hosts?

The engine ships two hosts for the same game UI:

* **native** - `legaia-engine play-window` (`crates/engine-shell`, wgpu via
  `crates/engine-render`),
* **web** - the browser play page (`crates/web-viewer`, WebGL + canvas via
  `site/play.html`).

`crates/engine-ui` is the wgpu-free leaf both hosts share: every screen's
geometry is a `pub fn ..._draws_for(...) -> Vec<TextDraw>` (or `SpriteDraw`)
builder there, and a host "has" a screen exactly when it calls that builder.
That makes the set of engine-ui draw builders a **machine-checkable feature
surface** - no hand-maintained list of screens to fall out of date.

The failure this catches: an engine wave adds a screen to engine-ui, wires it
into the native window, and the browser play page silently drifts a release
behind. Nothing about that is visible in a diff. Here it is a red CI run.

A host reaches a screen transitively too. Builders compose - the party info
panel folds in the AP gauge, the pen-only tab alias delegates to the tab
painter - so "is this screen on screen" is a question about the call graph, not
about which name the host happens to type. Host references seed the used-set and
then propagate along engine-ui's own builder-to-builder edges. Counting only the
shallowest wrapper made every composed widget read as unused, which is a defect
of the instrument rather than a gap in the port, and it rewarded a host for
naming a wrapper over calling the thing that draws.

Classification per builder:

* used by both hosts              -> ok
* used by native, not by web      -> DRIFT (fail, unless waived)
* used by web, not by native      -> web-ahead (info only)
* used by neither                 -> ORPHAN (fail, unless waived)

Waivers live in `scripts/ci/ui-host-drift-waivers.toml`; each needs a reason.
They are validated in both directions, which is what keeps the file honest:

* a waiver naming a builder that no longer exists   -> fail (stale)
* a waiver for a builder now wired on both hosts    -> fail (close it out)
* a `web_missing` waiver whose builder is not
  actually native-only any more                     -> fail (wrong bucket)

So the waiver file cannot rot into a lie: it only compiles as long as it
describes the real state of the two hosts.

## The second question: do both hosts feed the builder the same model?

Reachability is only half of "shared". Two hosts can call one builder with
divergently-constructed arguments and everything above stays green forever -
same screen, different geometry, no diff to look at. That blind spot is
structural: the checks above ask whether a host's source *names* a builder,
never what it passes.

The general form of the second question is not decidable from source text -
"does `assets.pen(id)` equal `self.menu_window_pen(id)` at runtime" is a
question about two programs, not two token streams. So this file does not
attempt it. What it does instead is pin the one part of the divergence that
*is* exactly decidable, and which is where the duplication actually sits:
**geometry constants that exist twice, once per host.**

`crates/web-viewer/src/play_menu.rs` carries a 23-row pinned window-rect
table whose doc comment says it is "byte-identical to the native window's
`MENU_WINDOW_FALLBACK`". That sentence is the entire guarantee - a prose
assertion of the kind this repo has already watched go false in the waiver
file, where a bucket is re-derived from source every run but a *reason* is
not. [`CONSTANT_PAIRS`] turns those sentences into a check: each pair names a
constant on each host, and the two initialisers must normalise to the same
token stream.

Be precise about what that does and does not establish:

* it DOES prove two named constants carry equal values, and that neither was
  renamed or deleted out from under the pairing;
* it does NOT prove the two hosts *use* the constants the same way, that they
  build the same model, or that any un-paired literal agrees.

A narrow check that says so is worth more than a broad one that implies more
than it measured. Adding a pair is how the covered set grows.

Usage:

    python3 scripts/ci/check-ui-host-drift.py            # check, exit 1 on drift
    python3 scripts/ci/check-ui-host-drift.py --quiet    # findings only
    python3 scripts/ci/check-ui-host-drift.py --list     # full surface table
    python3 scripts/ci/check-ui-host-drift.py --selftest # detector control suite

Exit status: 0 = clean, 1 = drift / stale waiver / constant mismatch,
2 = self-test failed.
"""

import argparse
import re
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    import tomli as tomllib  # type: ignore[no-redef]

REPO = Path(__file__).resolve().parent.parent.parent
UI_SRC = REPO / "crates" / "engine-ui" / "src"
WAIVERS = Path(__file__).resolve().parent / "ui-host-drift-waivers.toml"

# Source roots per host. `engine-render` counts as native: it re-exports
# engine-ui wholesale and wraps some builders in GPU-resident batches, so a
# call there is still the native window reaching the screen.
HOSTS = {
    "native": [
        REPO / "crates" / "engine-shell" / "src",
        REPO / "crates" / "engine-render" / "src",
    ],
    "web": [REPO / "crates" / "web-viewer" / "src"],
}

# A draw builder is a public fn whose return type mentions TextDraw or
# SpriteDraw - that is exactly "projects a view into renderer-agnostic
# quads", i.e. one screen's (or one screen fragment's) geometry.
#
# Signatures here are routinely multi-line, so the return type is read from
# the span between the fn keyword and the opening brace of the body rather
# than from a single-line pattern.
BUILDER_RE = re.compile(r"^pub fn (?P<name>[a-z0-9_]+)\s*[<(]", re.MULTILINE)
DRAW_RET_RE = re.compile(r"->[^;{]*(?:TextDraw|SpriteDraw)")

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
TRANSFORM_PARAM_RE = re.compile(r"\b(?:SpriteRequest|TextDraw|SpriteDraw)\b")

LINE_COMMENT_RE = re.compile(r"//.*$", re.MULTILINE)

# Host source files the paired-constant check reads. Named here rather than
# discovered, because a pair is a claim about two specific declarations.
NATIVE_WINDOW = "crates/engine-shell/src/bin/legaia-engine/window.rs"
NATIVE_HUD = "crates/engine-shell/src/bin/legaia-engine/window/hud.rs"
WEB_PLAY_MENU = "crates/web-viewer/src/play_menu.rs"
WEB_PLAY_SHOP = "crates/web-viewer/src/play_shop.rs"

# Geometry constants that exist once per host and must agree. See the module
# docstring for the scope of the claim: equal values, nothing about use.
#
# A pair earns its place by being a number the two hosts each hand to the
# SAME engine-ui builder. That is what makes a divergence a screen that
# renders differently on the two hosts rather than an unrelated coincidence
# of two equal integers - `hud.rs`'s `BATTLE_HUD_PEN` is also `(8, 60)` and
# is deliberately NOT paired with the level-up pen, because nothing says the
# battle HUD and the level-up banner must move together.
CONSTANT_PAIRS: list[dict[str, object]] = [
    {
        "what": "pinned menu window-descriptor rects (fallback when the disc "
        "table is absent) - fed to menu_window_chrome_draws_for / tab_banner_draws "
        "and to every *_draws_for pen on the pause screens",
        "native": (NATIVE_WINDOW, "MENU_WINDOW_FALLBACK"),
        "web": (WEB_PLAY_MENU, "WINDOW_FALLBACK"),
    },
    {
        "what": "near-fullscreen content rect for the sub-screens whose retail "
        "window set is not capture-pinned (Items / Magic / Arts generic frame)",
        "native": (NATIVE_WINDOW, "MENU_SUBWINDOW_CONTENT"),
        "web": (WEB_PLAY_MENU, "SUBWINDOW_CONTENT"),
    },
    {
        "what": "field shop / inn overlay pen - shop_draws_for's `pen` argument",
        "native": (NATIVE_HUD, "SHOP_OVERLAY_PEN"),
        "web": (WEB_PLAY_SHOP, "SHOP_PEN"),
    },
    {
        "what": "level-up banner pen - level_up_draws_for's `pen` argument",
        "native": (NATIVE_HUD, "LEVEL_UP_BANNER_PEN"),
        "web": (WEB_PLAY_SHOP, "LEVEL_UP_PEN"),
    },
    {
        "what": "capture banner pen - capture_banner_draws_for's `pen` argument",
        "native": (NATIVE_HUD, "CAPTURE_BANNER_PEN"),
        "web": (WEB_PLAY_SHOP, "CAPTURE_PEN"),
    },
]

BLOCK_COMMENT_RE = re.compile(r"/\*.*?\*/", re.DOTALL)
USE_STMT_RE = re.compile(r"\buse\s+[^;]*;")
CONST_DECL_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+{}\b", re.MULTILINE)


def const_initialiser(text: str, name: str) -> str | None:
    """The source of `const NAME ... = <here>;`, or None if not declared.

    Scans for the terminating `;` at nesting depth zero rather than taking the
    line, because these initialisers are multi-line array and block
    expressions. String and char literals are skipped so a `;` inside one
    cannot end the scan early.
    """
    m = CONST_DECL_RE.pattern.format(re.escape(name))
    decl = re.search(m, text, re.MULTILINE)
    if not decl:
        return None
    eq = text.find("=", decl.end())
    if eq < 0:
        return None
    i, depth, n = eq + 1, 0, len(text)
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            nl = text.find("\n", i)
            i = n if nl < 0 else nl
            continue
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            end = text.find("*/", i + 2)
            i = n if end < 0 else end + 2
            continue
        if c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            i = j + 1
            continue
        if c == "'":
            lit = re.match(r"'(?:\\.|[^\\'])'", text[i:])
            if lit:
                i += lit.end()
                continue
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == ";" and depth == 0:
            return text[eq + 1 : i]
        i += 1
    return None


def normalise_value(src: str) -> str:
    """Reduce an initialiser to its value tokens.

    Drops comments, drops `use ...;` (both tables open with an alias import
    whose spelling is a host's own business), drops trailing commas before a
    closer, and collapses whitespace - so formatting, rustfmt directives and
    import aliasing cannot read as a value change, while a single changed
    digit always does.
    """
    src = BLOCK_COMMENT_RE.sub(" ", src)
    src = LINE_COMMENT_RE.sub(" ", src)
    src = USE_STMT_RE.sub(" ", src)
    src = re.sub(r",\s*(?=[)\]}])", "", src)
    src = re.sub(r"\s+", "", src)
    return src


def check_constant_pairs() -> list[str]:
    """Compare every [`CONSTANT_PAIRS`] entry; return one message per problem."""
    problems: list[str] = []
    for pair in CONSTANT_PAIRS:
        values: dict[str, str] = {}
        missing = False
        for host in ("native", "web"):
            rel, name = pair[host]  # type: ignore[index]
            path = REPO / rel
            if not path.is_file():
                problems.append(f"CONSTANT {name}: host source {rel} is missing.")
                missing = True
                continue
            raw = const_initialiser(path.read_text(encoding="utf-8"), name)
            if raw is None:
                problems.append(
                    f"CONSTANT {name}: no `const {name}` in {rel}. Renamed or "
                    f"deleted? Update CONSTANT_PAIRS in "
                    f"{Path(__file__).name} - an unresolvable pair checks nothing."
                )
                missing = True
                continue
            values[host] = normalise_value(raw)
        if missing or len(values) != 2:
            continue
        if values["native"] != values["web"]:
            nrel, nname = pair["native"]  # type: ignore[index]
            wrel, wname = pair["web"]  # type: ignore[index]
            nat, web = values["native"], values["web"]
            problems.append(
                f"CONSTANT DRIFT {nname} != {wname} ({pair['what']}):\n"
                f"      native {nrel}:\n        {first_difference(nat, web)}\n"
                f"      web    {wrel}:\n        {first_difference(web, nat)}\n"
                f"      Both hosts feed these to the same engine-ui builder, so "
                f"the screen now renders differently on the two hosts. Make them "
                f"agree, or move the value into a crate both hosts depend on."
            )
    return problems


def first_difference(this: str, other: str, window: int = 60) -> str:
    """`this` windowed around where it first departs from `other`.

    A 23-row table printed from its start is unreadable and, worse, useless:
    the two renderings agree for hundreds of characters, so a head-truncated
    dump shows two identical-looking lines and leaves the reader to diff by
    eye. The divergence is the only part worth printing.
    """
    i = 0
    while i < min(len(this), len(other)) and this[i] == other[i]:
        i += 1
    lo = max(0, i - window // 2)
    hi = min(len(this), i + window)
    return ("..." if lo else "") + this[lo:hi] + ("..." if hi < len(this) else "")

# A host "has" a screen when its *shipped* code draws it. Both native roots
# carry `#[cfg(test)]` modules inside `src/` - `engine-render/src/tests/` is a
# whole directory of them - and a builder exercised only by a unit test is
# precisely the not-wired case this gate exists to surface. Counting those
# references made the gate report test coverage as wiring, which let four
# `web_missing` waivers assert "native calls it" about builders no host called.
# The two-directional waiver validation could not catch that: it re-derives the
# bucket from the same over-counted signal.


def is_test_source(path: Path) -> bool:
    """Is this file test-only code rather than shipped host code?

    Deliberately **path-only**: a `tests/` directory component or a `tests.rs`
    file name, which is the split-out `mod tests;` convention every test module
    under these roots follows (`engine-render/src/tests/` is a whole directory
    of them, and it is where all six mis-bucketed references lived).

    Sniffing file *contents* for `#[cfg(test)]` was tried and rejected: plenty
    of shipped files carry an inline test module, so a content rule drops real
    host code from the scan - it excluded `engine-render/src/lib.rs`,
    `engine-render/src/renderer.rs` and `engine-shell/.../window.rs` among
    others. That direction is the dangerous one. Over-counting "used" only
    makes the gate nag about a screen that is in fact wired; under-counting it
    invents ORPHANs and, worse, lets a waiver be written asserting a gap that
    does not exist. The path rule cannot do that, because a file under
    `src/tests/` is never a host draw site.
    """
    return "tests" in path.parts or path.name == "tests.rs"


def strip_comments(text: str) -> str:
    """Drop `//`-style comments.

    Doc comments name sibling builders constantly (`[`shop_draws_for`]`), and
    a mention in prose is not a wiring. Stripping them keeps the checker
    conservative in the safe direction: it under-reports "used", so it can
    nag about a screen that is in fact wired, but it never stays silent about
    one that is not.
    """
    return LINE_COMMENT_RE.sub("", text)


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
    for path in sorted(UI_SRC.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for m in BUILDER_RE.finditer(text):
            # The signature runs from the fn keyword to the body's opening
            # brace; anything past that is the body and must not be sniffed
            # for a return type.
            brace = text.find("{", m.start())
            if brace < 0:
                continue
            if not is_screen_signature(text[m.start() : brace]):
                continue
            line = text[: m.start()].count("\n") + 1
            rel = path.relative_to(REPO)
            out[m.group("name")] = f"{rel}:{line}"
    return out


def fn_body(text: str, brace: int) -> str:
    """The source between `text[brace]` and its matching close brace.

    Rust-aware enough to brace-match: comments, string / raw-string / char
    literals are skipped so a `format!("{}", ..)` or a lone brace in a string
    cannot unbalance the scan. Everything else is counted, which is all the
    caller needs - it only greps the result for identifiers.
    """
    n = len(text)
    i, depth = brace, 0
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            nl = text.find("\n", i)
            i = n if nl < 0 else nl
            continue
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            end = text.find("*/", i + 2)
            i = n if end < 0 else end + 2
            continue
        if c == "r" and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            j = i + 1
            while j < n and text[j] == "#":
                j += 1
            if j < n and text[j] == '"':
                term = '"' + "#" * (j - i - 1)
                end = text.find(term, j + 1)
                i = n if end < 0 else end + len(term)
                continue
        if c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            i = j + 1
            continue
        if c == "'":
            m = re.match(r"'(?:\\.|[^\\'])'", text[i:])
            if m:
                i += m.end()
                continue
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return text[brace + 1 : i]
        i += 1
    return text[brace + 1 :]


def collect_builder_refs(names: set[str]) -> dict[str, set[str]]:
    """Map builder name -> the builders its own body references.

    This is the engine-ui-internal half of the call graph. It is deliberately
    limited to builder-to-builder edges: a host that draws one screen draws
    whatever that screen composes, and nothing further is claimed.
    """
    refs: dict[str, set[str]] = {n: set() for n in names}
    for path in sorted(UI_SRC.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for m in BUILDER_RE.finditer(text):
            name = m.group("name")
            if name not in names:
                continue
            brace = text.find("{", m.start())
            if brace < 0:
                continue
            body = strip_comments(fn_body(text, brace))
            for other in names:
                if other != name and re.search(rf"\b{re.escape(other)}\b", body):
                    refs[name].add(other)
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
                for name in names:
                    if name in uses and re.search(rf"\b{re.escape(name)}\b", body):
                        uses[name].add(host)
    return uses


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


# Detector control suite for `is_screen_signature`. Real signatures, copied
# from `crates/engine-ui/src` (multi-line ones flattened - the caller feeds it
# a raw source span either way).
SELFTEST_SCREENS: list[tuple[str, str]] = [
    # A model in, quads out: the ordinary screen shape.
    ("shop_draws_for",
     "pub fn shop_draws_for(font: &legaia_font::Font, title: &str, rows: &[ShopRow<'_>], "
     "cursor: usize, gold: Option<i32>, pen: (i32, i32)) -> Vec<TextDraw>"),
    # A glyph layout is a model of text, not a draw record - the sibling of
    # `sprite_draws_for` that must stay counted, or the surface loses a
    # screen fragment both hosts really do share.
    ("text_draws_for",
     "pub fn text_draws_for(layout: &legaia_font::Layout, pen: (i32, i32), "
     "color: [f32; 4]) -> Vec<TextDraw>"),
    # Sprite-returning screens are screens: the parameters are a model.
    ("equip_screen_sprites_for",
     "pub fn equip_screen_sprites_for(view: &EquipView<'_>, rects: &SaveMenuAtlasRects, "
     "origin: (i32, i32), scale: u32) -> Vec<SpriteDraw>"),
    # Tuple returns count too - the painter family returns quads beside
    # pictogram / cursor requests.
    ("sell_quantity_draws_for",
     "pub fn sell_quantity_draws_for(font: &legaia_font::Font, rect: PainterRect, "
     "selected: bool, heading: &str, quantity: u32, held: u32, unit_price: u32) "
     "-> (Vec<TextDraw>, Option<PainterPictogram>, Option<PainterSprite>)"),
]

SELFTEST_TRANSFORMS: list[tuple[str, str]] = [
    # The shape the return-type-only rule could not see.
    ("sprite_draws_for",
     "pub fn sprite_draws_for(requests: &[SpriteRequest], anchor: (i32, i32)) "
     "-> Vec<SpriteDraw>"),
    # Same shape with the other two record types, so the rule is not pinned
    # to one name.
    ("rebatch_text",
     "pub fn rebatch_text(draws: &[TextDraw], origin: (i32, i32)) -> Vec<TextDraw>"),
    ("merge_sprites",
     "pub fn merge_sprites(a: &[SpriteDraw], b: &[SpriteDraw]) -> Vec<SpriteDraw>"),
    # Not a draw builder at all - no quads out.
    ("scale_stage_text_draws",
     "pub fn scale_stage_text_draws(draws: &mut [TextDraw], stage_origin: (i32, i32), "
     "stage_scale: u32)"),
]


# Control suite for the constant-pair normaliser. A normaliser that collapsed
# everything to "" would report every pair equal and the check would be
# theatre, so both directions are pinned: noise must vanish, values must not.
#
# Each case is `(label, source_a, source_b, should_match)`.
SELFTEST_CONSTANTS: list[tuple[str, str, str, bool]] = [
    (
        "formatting and import aliasing are noise",
        "{ use legaia_asset::menu_windows::window_ids as w;\n"
        "  [ (w::TAB_ITEMS, (16, 12, 60, 12)),\n"
        "    (w::TAB_MAGIC, (16, 12, 60, 12)) ] }",
        "{use foo::bar as w; [(w::TAB_ITEMS,(16,12,60,12)),(w::TAB_MAGIC,(16,12,60,12)),]}",
        True,
    ),
    (
        "comments are noise",
        "(18, 18, 284, 200) // the near-fullscreen stage rect",
        "(18, 18, 284, 200) /* same rect, different note */",
        True,
    ),
    (
        "one changed digit is a value change",
        "(8, 140)",
        "(8, 141)",
        False,
    ),
    (
        "a dropped table row is a value change",
        "[(w::A, (1, 2, 3, 4)), (w::B, (5, 6, 7, 8))]",
        "[(w::A, (1, 2, 3, 4))]",
        False,
    ),
    (
        "a reordered table is a value change (id order is the table)",
        "[(w::A, (1, 2, 3, 4)), (w::B, (5, 6, 7, 8))]",
        "[(w::B, (5, 6, 7, 8)), (w::A, (1, 2, 3, 4))]",
        False,
    ),
]


def run_selftest() -> int:
    failures = 0
    for name, sig in SELFTEST_SCREENS:
        if is_screen_signature(sig):
            print(f"  ok    {name}: counted as a screen")
        else:
            print(f"  FAIL  {name}: dropped from the surface (expected a screen)")
            failures += 1
    for name, sig in SELFTEST_TRANSFORMS:
        if is_screen_signature(sig):
            print(f"  FAIL  {name}: counted as a screen (expected a transform)")
            failures += 1
        else:
            print(f"  ok    {name}: excluded as a draw-list transform")
    for label, a, b, want in SELFTEST_CONSTANTS:
        got = normalise_value(a) == normalise_value(b)
        if got == want:
            print(f"  ok    constants: {label}")
        else:
            verdict = "matched" if got else "differed"
            print(f"  FAIL  constants: {label} - normaliser {verdict}")
            failures += 1
    total = len(SELFTEST_SCREENS) + len(SELFTEST_TRANSFORMS) + len(SELFTEST_CONSTANTS)
    if failures:
        print(
            f"\nself-test: {failures} of {total} case(s) failed - the surface this "
            f"gate measures is not the set of screens, so its verdict means nothing"
        )
        return 2
    print(f"\nself-test: all {total} cases pass")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--quiet", action="store_true", help="findings only")
    ap.add_argument("--list", action="store_true", help="print the full surface table")
    ap.add_argument(
        "--selftest",
        action="store_true",
        help="run the screen-vs-transform control suite and exit",
    )
    args = ap.parse_args()

    if args.selftest:
        print("check-ui-host-drift self-test")
        return run_selftest()

    # The surface is only meaningful if the classifier demonstrably separates
    # the two shapes. Run the control every time: a "0 orphans" verdict from a
    # classifier that counts everything, or nothing, is not a measurement.
    for _name, sig in SELFTEST_SCREENS:
        if not is_screen_signature(sig):
            print(
                "ERROR: built-in screen control failed; the builder surface is not "
                "trustworthy. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _name, sig in SELFTEST_TRANSFORMS:
        if is_screen_signature(sig):
            print(
                "ERROR: built-in transform control failed; the builder surface is not "
                "trustworthy. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, a, b, want in SELFTEST_CONSTANTS:
        if (normalise_value(a) == normalise_value(b)) != want:
            print(
                "ERROR: built-in constant-pair control failed; a normaliser that "
                "cannot tell a value change from formatting proves nothing about "
                "the pairs below. Run --selftest.",
                file=sys.stderr,
            )
            return 2

    builders = collect_builders()
    if not builders:
        print("[ui-drift] no draw builders found - is crates/engine-ui/src present?", file=sys.stderr)
        return 1
    uses = collect_uses(set(builders))
    seed_transitively(uses, collect_builder_refs(set(builders)))
    waivers = load_waivers()

    drift: list[str] = []
    orphan: list[str] = []
    web_ahead: list[str] = []
    both: list[str] = []
    for name in sorted(builders):
        hosts = uses[name]
        if hosts == {"native", "web"}:
            both.append(name)
        elif hosts == {"native"}:
            drift.append(name)
        elif hosts == {"web"}:
            web_ahead.append(name)
        else:
            orphan.append(name)

    if args.list:
        for name in sorted(builders):
            hosts = uses[name] or {"-"}
            mark = "W" if name in waivers else " "
            print(f"{mark} {name:<40} {','.join(sorted(hosts)):<12} {builders[name]}")

    problems: list[str] = []

    # Unwaived drift / orphans.
    for name in drift:
        if name in waivers:
            if waivers[name].get("kind") != "web_missing":
                problems.append(
                    f"{name}: waiver kind is "
                    f"'{waivers[name].get('kind')}' but the builder is native-only "
                    f"(expected kind = \"web_missing\")"
                )
            continue
        problems.append(
            f"DRIFT {name} ({builders[name]}): wired in the native window, "
            f"not in the browser play page. Wire it into crates/web-viewer, or "
            f"add a waiver with a reason to {WAIVERS.relative_to(REPO)}."
        )
    for name in orphan:
        if name in waivers:
            if waivers[name].get("kind") != "orphan":
                problems.append(
                    f"{name}: waiver kind is '{waivers[name].get('kind')}' but "
                    f"no host calls the builder (expected kind = \"orphan\")"
                )
            continue
        problems.append(
            f"ORPHAN {name} ({builders[name]}): no host calls this builder. "
            f"Wire it, delete it, or waive it in {WAIVERS.relative_to(REPO)}."
        )

    # Stale waivers - the half that stops this file decaying into fiction.
    for name, entry in sorted(waivers.items()):
        if name not in builders:
            problems.append(
                f"STALE WAIVER {name}: no such engine-ui draw builder "
                f"(renamed or deleted?). Drop the waiver."
            )
            continue
        if name in both:
            problems.append(
                f"STALE WAIVER {name}: now wired on BOTH hosts - the gap is "
                f"closed. Drop the waiver."
            )
        if name in web_ahead:
            problems.append(
                f"STALE WAIVER {name}: web calls it and native does not, so "
                f"this is not a web gap. Drop the waiver."
            )
        if not str(entry.get("reason", "")).strip():
            problems.append(f"WAIVER {name}: needs a non-empty `reason`.")

    # The model half: paired host constants must carry equal values.
    problems.extend(check_constant_pairs())

    if not args.quiet:
        print(
            f"[ui-drift] engine-ui draw builders: {len(builders)} "
            f"({len(both)} on both hosts, {len(drift)} native-only, "
            f"{len(web_ahead)} web-only, {len(orphan)} unused)"
        )
        print(
            f"[ui-drift] paired host geometry constants: {len(CONSTANT_PAIRS)} "
            f"(value equality only - see the module docstring for what this "
            f"does not prove)"
        )
        if web_ahead:
            print(f"[ui-drift] web-ahead (informational): {', '.join(web_ahead)}")

    if problems:
        print(f"\n[ui-drift] {len(problems)} problem(s):", file=sys.stderr)
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        return 1

    if not args.quiet:
        print("[ui-drift] ok - every shared screen reaches both hosts or is waived")
    return 0


if __name__ == "__main__":
    sys.exit(main())
