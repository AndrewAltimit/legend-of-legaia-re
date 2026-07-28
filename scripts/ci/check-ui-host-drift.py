#!/usr/bin/env python3
"""UI host-drift checker: does every shared screen reach BOTH hosts?

The engine ships the same game UI through more than one framebuffer:

* **native** - `legaia-engine play-window` (`crates/engine-shell`, wgpu via
  `crates/engine-render`),
* **web** - the browser pages built from `crates/web-viewer`. That is two
  surfaces, not one: the **play page** (`runtime.rs` + `play_*.rs`, driven by
  `site/js/play-app.js`) and the **minigames page** (`minigames*.rs`,
  `LegaiaMinigames`), which calls `engine-core` session types directly and
  never goes through `World`.

The web bucket is deliberately one label here, and saying why matters,
because collapsing it is where this gate's blind spot lives. The
*reachability* question below - "does a host's shipped source reach this
builder" - is answered per binary, and both web surfaces ship in one
`legaia-web-viewer` cdylib. Splitting them would not find a gap; it would
manufacture 80 of them, because the minigames page is a different screen set
rather than a second copy of the play page.

What the collapse really hides is a *model* question - the two web surfaces
can feed one shared kernel two differently-built models, and the pause-menu
reachability test can never see it. That is what [`SIM_PAIRS`] is for; see
"The third question" below.

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
then propagate along engine-ui's own internal call edges. Counting only the
shallowest wrapper made every composed widget read as unused, which is a defect
of the instrument rather than a gap in the port, and it rewarded a host for
naming a wrapper over calling the thing that draws.

The graph spans **every** `fn` engine-ui defines, not only the builders. Limiting
it to builder-to-builder edges invents orphans wherever the composition runs
through a method or a private helper, and engine-ui's fishing HUD is exactly
that shape: `FishingBanners::service_frame` takes the four banner builders as
function pointers and `HudDraw::resolve_bar` is what reaches `bar_frame` /
`power_bar_frame`. Both are `impl` methods, so a builder-only graph reported six
wired builders as unused - six waivers that would each have asserted a gap that
does not exist, which is worse than the silence it replaced. Non-builder `fn`s
are graph nodes only; nothing classifies them.

Classification per builder:

* used by both hosts              -> ok
* used by native, not by web      -> DRIFT (fail, unless waived)
* used by web, not by native      -> web-ahead (info only)
* used by neither                 -> ORPHAN (fail, unless waived)

Every orphan is **named** on stdout, waived or not. A count is not a finding: for
as long as this gate printed `6 unused` and nothing else, deleting a builder's
only caller was invisible - which is how `RecipientWindowRects::active_compare`
was removed and left window 25's painter chain with no consumer at all. A count
went from 5 to 6 and no line of output changed.

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

## The third question: do both hosts feed the same MODEL to a shared kernel?

A geometry constant is the easy half of "same model". The hard half is the
simulation: two hosts can call one `engine-core` kernel having built its inputs
differently, or call different kernels entirely, and every check above stays
green - the screen is reachable, the rects agree, and the numbers on it come
from somewhere else.

[`SIM_PAIRS`] is the simulation twin of [`CONSTANT_PAIRS`]. Each row names a
feature and, per host, the **injection site** where that host hands a model to
the shared kernel, plus what must be true of both sites at once. Three
assertion modes, in increasing strength:

* `symbols_all`  - each named symbol must appear in both bodies,
* `symbols_same` - each named symbol must appear in both or in neither,
* `pattern_same` - the *set* of regex captures must be equal across the two.

`pattern_same` is the one that does not need the answer up front: it says the
two sites must agree without saying what they must agree on, so it keeps
working when the right set changes.

Scope, stated as narrowly as the constant check above:

* it DOES prove the two named sites mention (or omit) the same kernels, and
  that neither site was renamed or deleted out from under the pairing;
* it does NOT prove the arguments are equal, that the calls run in the same
  order, or that either site is reached at runtime.

A row may carry `blocked_on`, which marks a divergence that is known and being
closed elsewhere. That marker is validated in both directions exactly as a
waiver is: a `blocked_on` row that diverges reports and does not fail, and a
`blocked_on` row that has become **clean** FAILS, demanding the marker be
deleted. So a pending row cannot rot into a permanent exemption - the moment
the work lands, the gate says so.

Usage:

    python3 scripts/ci/check-ui-host-drift.py            # check, exit 1 on drift
    python3 scripts/ci/check-ui-host-drift.py --quiet    # findings only
    python3 scripts/ci/check-ui-host-drift.py --list     # full surface table
    python3 scripts/ci/check-ui-host-drift.py --selftest # detector control suite

Exit status: 0 = clean, 1 = drift / stale waiver / constant mismatch /
sim-pair mismatch, 2 = self-test failed.
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

# A draw builder is a public fn whose return type mentions one of engine-ui's
# own draw-record types - that is exactly "projects a view into
# renderer-agnostic geometry", i.e. one screen's (or one screen fragment's)
# layout.
#
# `TextDraw` / `SpriteDraw` are the terminal records a renderer consumes.
# The rest are engine-ui's *intermediate* records - a resolved screen that
# still needs a host-owned atlas or font to become quads:
#
#   HudDraw           the fishing HUD's item list (ui_fishing)
#   HudQuad           a PROT 0977 textured-Gouraud quad (other_game_hud)
#   DigitCell         one placed digit of a numeric readout (ui_fishing)
#   BarFrame          a resolved gauge bar (ui_fishing)
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

BUILDER_RE = re.compile(r"^pub fn (?P<name>[a-z0-9_]+)\s*[<(]", re.MULTILINE)
DRAW_RET_RE = re.compile(rf"->[^;{{]*(?:{DRAW_RECORDS})")

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

# Simulation injection sites that must agree across hosts. See the module
# docstring's "third question" for the scope of the claim, and for what a
# `blocked_on` marker does and does not buy.
#
# A row's `sites` map a host label to `(repo-relative path, fn name or None)`.
# `None` means the whole file is the site, which is right when a host's
# injection is a call made from a place the pairing should not pin.
NATIVE_BOOT = "crates/engine-shell/src/boot.rs"
NATIVE_BOOT_CUTSCENE = "crates/engine-shell/src/bin/legaia-engine/window/boot_cutscene.rs"
NATIVE_FRAME_TICK = "crates/engine-core/src/world/frame_tick.rs"
WEB_MINIGAMES_MUSCLE = "crates/web-viewer/src/minigames_muscle.rs"
WEB_PLAY_BATTLE = "crates/web-viewer/src/play_battle.rs"

SIM_PAIRS: list[dict[str, object]] = [
    {
        "what": "Muscle Dome damage - the arena's per-exchange damage must come "
        "off the same battle-formula kernel on both hosts, or the same command "
        "deals different numbers in the window and in the browser. Both hosts "
        "install a `DomeDamageModel` and resolve through it, so the assertion "
        "is on the shared entry point, not on the formula leaf underneath it",
        "sites": {
            "native": (NATIVE_FRAME_TICK, "tick_muscle_dome"),
            "web": (WEB_MINIGAMES_MUSCLE, "muscle_resolve"),
        },
        "mode": "symbols_all",
        "symbols": ["resolve_turn_retail"],
    },
    {
        "what": "save-select model - whether the slot rack is put in card-slots "
        "mode decides how many rows it shows and what each row addresses, so "
        "the two hosts must make the same call or neither may",
        "sites": {
            "native": (NATIVE_BOOT_CUTSCENE, None),
            "web": (WEB_PLAY_MENU, None),
        },
        "mode": "symbols_same",
        "symbols": ["set_card_slots_mode"],
        "blocked_on": "the browser pause menu arms card-slots mode and the "
        "native boot/cutscene save rack does not. A concurrent lane owns the "
        "native window tree; delete this marker once the two agree.",
    },
    {
        "what": "live-loop arming - the browser twin of `enter_field_live`. "
        "Every `World::set_*` one host installs before running the live "
        "gameplay loop and the other does not is a table the two simulations "
        "disagree about (drops, prices, spells, battle BGM)",
        "sites": {
            "native": (NATIVE_BOOT, "enter_field_live"),
            "web": (WEB_PLAY_BATTLE, "arm_live_battles"),
        },
        "mode": "pattern_same",
        "pattern": r"\.(set_[a-z0-9_]+)\s*\(",
        "blocked_on": "a concurrent lane is closing the arming gap. Delete "
        "this marker once both sites install the same table set.",
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


def signature_end(text: str, start: int) -> int:
    """Index of the body's `{` for the `fn` at `text[start]`, or -1 if none.

    Scans at bracket depth zero so a `;` inside an array return type
    (`-> [f32; 4] {`) does not read as a bodyless trait declaration, and a
    genuinely bodyless `fn f(..);` does.
    """
    i, n, depth = start, len(text), 0
    while i < n:
        c = text[i]
        if c in "([":
            depth += 1
        elif c in ")]":
            depth -= 1
        elif depth == 0 and c == "{":
            return i
        elif depth == 0 and c == ";":
            return -1
        i += 1
    return -1


def site_source(rel: str, fn_name: str | None) -> tuple[str | None, str]:
    """The source a [`SIM_PAIRS`] site names: one fn body, or the whole file.

    Returns `(text, problem)`; `text` is None when the site cannot be
    resolved, in which case `problem` says why. An unresolvable site is
    always an error - a pairing that cannot find its own anchor checks
    nothing, and would otherwise pass forever after a rename.
    """
    path = REPO / rel
    if not path.is_file():
        return None, f"host source {rel} is missing"
    text = path.read_text(encoding="utf-8")
    if fn_name is None:
        return strip_comments(BLOCK_COMMENT_RE.sub(" ", text)), ""
    m = re.search(rf"\bfn\s+{re.escape(fn_name)}\s*[<(]", text)
    if not m:
        return None, f"no `fn {fn_name}` in {rel} (renamed or deleted?)"
    brace = signature_end(text, m.start())
    if brace < 0:
        return None, f"`fn {fn_name}` in {rel} has no body"
    return strip_comments(BLOCK_COMMENT_RE.sub(" ", fn_body(text, brace))), ""


def sim_pair_divergence(pair: dict) -> tuple[list[str], list[str]]:
    """Evaluate one [`SIM_PAIRS`] row.

    Returns `(hard_errors, divergences)`. A hard error (an unresolvable site,
    a malformed row) always fails; a divergence fails unless the row carries
    `blocked_on`.
    """
    sites: dict = pair["sites"]  # type: ignore[assignment]
    errors: list[str] = []
    bodies: dict[str, str] = {}
    for host, (rel, fn_name) in sites.items():
        text, problem = site_source(rel, fn_name)
        if text is None:
            errors.append(f"SIM {pair['what']!r}: {problem}")
        else:
            bodies[host] = text
    if errors or len(bodies) != 2:
        return errors, []

    hosts = sorted(bodies)
    a, b = hosts[0], hosts[1]
    mode = pair.get("mode")
    diffs: list[str] = []

    def where(host: str) -> str:
        rel, fn_name = sites[host]
        return f"{rel}::{fn_name}" if fn_name else rel

    if mode in ("symbols_all", "symbols_same"):
        for sym in pair.get("symbols", []):  # type: ignore[union-attr]
            seen = {h: re.search(rf"\b{re.escape(sym)}\b", bodies[h]) is not None for h in hosts}
            if mode == "symbols_all" and not all(seen.values()):
                missing = [h for h in hosts if not seen[h]]
                diffs.append(
                    f"`{sym}` is not called at {', '.join(where(h) for h in missing)}"
                )
            elif mode == "symbols_same" and seen[a] != seen[b]:
                has = a if seen[a] else b
                lacks = b if seen[a] else a
                diffs.append(
                    f"`{sym}` is called at {where(has)} but not at {where(lacks)} - "
                    f"both must, or neither may"
                )
    elif mode == "pattern_same":
        pat = re.compile(pair["pattern"])  # type: ignore[arg-type]
        found = {h: {m.group(1) for m in pat.finditer(bodies[h])} for h in hosts}
        if found[a] != found[b]:
            only_a = sorted(found[a] - found[b])
            only_b = sorted(found[b] - found[a])
            if only_a:
                diffs.append(f"only {where(a)}: {', '.join(only_a)}")
            if only_b:
                diffs.append(f"only {where(b)}: {', '.join(only_b)}")
    else:
        errors.append(f"SIM {pair['what']!r}: unknown mode {mode!r}")
    return errors, diffs


def check_sim_pairs() -> tuple[list[str], list[str]]:
    """Compare every [`SIM_PAIRS`] row.

    Returns `(problems, pending)`. `pending` lists the rows whose divergence
    is disclosed by `blocked_on`; a `blocked_on` row with NO divergence is a
    problem, not a pass - the marker has outlived the gap and must go.
    """
    problems: list[str] = []
    pending: list[str] = []
    for pair in SIM_PAIRS:
        errors, diffs = sim_pair_divergence(pair)
        problems.extend(errors)
        if errors:
            continue
        blocked = pair.get("blocked_on")
        if diffs and not blocked:
            problems.append(
                f"SIM DRIFT ({pair['what']}):\n"
                + "".join(f"      {d}\n" for d in diffs)
                + f"      The two hosts hand different models to the same kernel. "
                f"Make the sites agree, or record why not with `blocked_on`."
            )
        elif diffs:
            pending.append(f"{pair['what']}\n      " + "\n      ".join(diffs))
        elif blocked:
            problems.append(
                f"STALE blocked_on ({pair['what']}): the two sites now agree, so "
                f"the marker describes a gap that is closed. Drop `blocked_on` "
                f"from the row in {Path(__file__).name} - a pending marker that "
                f"outlives its gap is a permanent exemption wearing a temporary "
                f"name."
            )
    return problems, pending

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
            brace = signature_end(text, m.start())
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


def collect_fn_names() -> set[str]:
    """Every `fn` name engine-ui defines, at any indentation."""
    out: set[str] = set()
    for path in sorted(UI_SRC.rglob("*.rs")):
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
    for path in sorted(UI_SRC.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for m in ANY_FN_RE.finditer(text):
            name = m.group("name")
            if name not in refs:
                continue
            brace = signature_end(text, m.start())
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
    # Intermediate records are screens: a model in, a resolved screen out,
    # still needing the host's atlas or font to become quads. These four are
    # the whole reason the surface widened.
    ("persistent_hud_draws",
     "pub fn persistent_hud_draws(points: i32, best_points: i32, rod_index: u32, "
     "lure_count: i32) -> Vec<HudDraw>"),
    ("number_digit_cells",
     "pub fn number_digit_cells(style: i32, x: i32, y: i32, value: i32) -> Vec<DigitCell>"),
    ("bar_frame",
     "pub fn bar_frame(x: i32, y: i32, value: i32, segments: i32, style: i32) -> BarFrame"),
    ("equip_compare_panel_fields",
     "pub fn equip_compare_panel_fields(view: &EquipComparePanelView<'_>, "
     "pen: (i32, i32)) -> Vec<ComparePanelField>"),
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
    # The intermediate-record renderer: HudDraw list in, TextDraw list out.
    # A projection of a screen it did not build - the screens are
    # `persistent_hud_draws` / `catch_hud_draws` above.
    ("fishing_hud_draws_for",
     "pub fn fishing_hud_draws_for(font: &legaia_font::Font, items: &[HudDraw], "
     "captions: &FishingCaptions<'_>, atlas: &FishingHudAtlas<'_>, origin: (i32, i32)) "
     "-> Vec<TextDraw>"),
    ("compare_panel_draws_for",
     "pub fn compare_panel_draws_for(font: &legaia_font::Font, "
     "fields: &[ComparePanelField]) -> Vec<TextDraw>"),
]

# Control suite for `signature_end`, the scan the whole call graph rests on.
# Each case is `(label, source, should_find_a_body)`.
SELFTEST_SIGNATURES: list[tuple[str, str, bool]] = [
    ("ordinary fn has a body", "fn f(a: i32) -> u8 { 0 }", True),
    ("array return type's `;` is not a terminator",
     "fn tint(x: u8) -> [f32; 4] { [0.0; 4] }", True),
    ("trait declaration has no body", "fn on_scene_enter(&mut self, s: &str);", False),
    ("generic fn has a body", "fn f<T: Into<u8>>(v: T) -> Vec<TextDraw> { vec![] }", True),
]

# Control suite for the sim-pair comparators, over synthetic bodies so the
# modes are pinned independently of whatever the tree happens to look like.
# Each case is `(label, mode, body_a, body_b, extra, should_diverge)`.
SELFTEST_SIM: list[tuple[str, str, str, str, object, bool]] = [
    ("symbols_all: both call it", "symbols_all",
     "let d = damage_finish_lazy(&f);", "damage_finish_lazy(&g)", ["damage_finish_lazy"], False),
    ("symbols_all: one host misses it", "symbols_all",
     "let d = damage_finish_lazy(&f);", "let d = ad_hoc_damage(&g);",
     ["damage_finish_lazy"], True),
    ("symbols_same: neither calls it is agreement", "symbols_same",
     "nothing()", "nothing_else()", ["set_card_slots_mode"], False),
    ("symbols_same: exactly one calls it is drift", "symbols_same",
     "s.set_card_slots_mode(true)", "s.reset()", ["set_card_slots_mode"], True),
    ("pattern_same: same set in any order", "pattern_same",
     "w.set_a(); w.set_b();", "w.set_b(); w.set_a();", r"\.(set_[a-z0-9_]+)\s*\(", False),
    ("pattern_same: a missing installer is drift", "pattern_same",
     "w.set_a(); w.set_b();", "w.set_a();", r"\.(set_[a-z0-9_]+)\s*\(", True),
]


def _selftest_sim_case(mode: str, a: str, b: str, extra: object) -> bool:
    """Run one synthetic sim-pair comparison; True when it diverges."""
    hosts = ["native", "web"]
    bodies = {"native": a, "web": b}
    diffs: list[str] = []
    if mode in ("symbols_all", "symbols_same"):
        for sym in extra:  # type: ignore[union-attr]
            seen = {h: re.search(rf"\b{re.escape(sym)}\b", bodies[h]) is not None for h in hosts}
            if mode == "symbols_all" and not all(seen.values()):
                diffs.append(sym)
            elif mode == "symbols_same" and seen["native"] != seen["web"]:
                diffs.append(sym)
    else:
        pat = re.compile(str(extra))
        found = {h: {m.group(1) for m in pat.finditer(bodies[h])} for h in hosts}
        if found["native"] != found["web"]:
            diffs.append("pattern")
    return bool(diffs)


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
    for label, src, want in SELFTEST_SIGNATURES:
        if (signature_end(src, 0) >= 0) == want:
            print(f"  ok    signature: {label}")
        else:
            print(f"  FAIL  signature: {label}")
            failures += 1
    for label, mode, a, b, extra, want in SELFTEST_SIM:
        if _selftest_sim_case(mode, a, b, extra) == want:
            print(f"  ok    sim pair: {label}")
        else:
            print(f"  FAIL  sim pair: {label}")
            failures += 1
    total = (
        len(SELFTEST_SCREENS)
        + len(SELFTEST_TRANSFORMS)
        + len(SELFTEST_CONSTANTS)
        + len(SELFTEST_SIGNATURES)
        + len(SELFTEST_SIM)
    )
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
    for _label, src, want in SELFTEST_SIGNATURES:
        if (signature_end(src, 0) >= 0) != want:
            print(
                "ERROR: built-in signature control failed; the call graph cannot "
                "find function bodies, so every reachability verdict below is "
                "meaningless. Run --selftest.",
                file=sys.stderr,
            )
            return 2
    for _label, mode, a, b, extra, want in SELFTEST_SIM:
        if _selftest_sim_case(mode, a, b, extra) != want:
            print(
                "ERROR: built-in sim-pair control failed; a comparator that cannot "
                "tell agreement from divergence proves nothing about the pairs "
                "below. Run --selftest.",
                file=sys.stderr,
            )
            return 2

    builders = collect_builders()
    if not builders:
        print("[ui-drift] no draw builders found - is crates/engine-ui/src present?", file=sys.stderr)
        return 1
    # Seed and propagate over the whole engine-ui fn graph, then classify only
    # the builders. Non-builder nodes exist so a composition that runs through
    # a method is not mistaken for an unused screen.
    fn_names = collect_fn_names()
    uses = collect_uses(fn_names)
    seed_transitively(uses, collect_call_graph(fn_names))
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

    # The simulation half: paired injection sites must name the same kernels.
    sim_problems, sim_pending = check_sim_pairs()
    problems.extend(sim_problems)

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
        print(
            f"[ui-drift] paired simulation injection sites: {len(SIM_PAIRS)} "
            f"({len(sim_pending)} disclosed as blocked)"
        )
        if web_ahead:
            print(f"[ui-drift] web-ahead (informational): {', '.join(web_ahead)}")
        # Name every orphan, waived or not. A bare count cannot distinguish
        # "the same six as yesterday" from "a builder's last caller was
        # deleted this morning", which is exactly how window 25's painter
        # chain lost its consumer without a line of output changing.
        for name in orphan:
            mark = "waived" if name in waivers else "UNWAIVED"
            print(f"[ui-drift] orphan ({mark}): {name}  {builders[name]}")
        for note in sim_pending:
            print(f"[ui-drift] sim pair blocked: {note}")

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
