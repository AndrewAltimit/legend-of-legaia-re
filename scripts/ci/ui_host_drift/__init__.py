"""UI host-drift checker: does every shared screen reach BOTH hosts?

The engine ships the same game UI through more than one framebuffer:

* **native** - `legaia-engine play-window` (`crates/engine-shell`, wgpu via
  `crates/engine-render`),
* **web** - the browser pages built from `crates/web-play` (shipped in
  `crates/web-viewer`'s bundle). That is two
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

The browser play page used to carry a 23-row pinned window-rect table whose
doc comment said it was "byte-identical to the native window's
`MENU_WINDOW_FALLBACK`". That sentence was the entire guarantee - a prose
assertion of the kind this repo has already watched go false in the waiver
file, where a bucket is re-derived from source every run but a *reason* is
not. [`CONSTANT_PAIRS`] turns those sentences into a check: each pair names a
constant on each host, and the two initialisers must normalise to the same
token stream.

A pair is the second-best outcome. The best one is that the constant exists
**once**, in a crate both hosts already depend on, at which point there is
nothing to pair and the row comes out of the table - which is what happened to
that window-rect table and to the near-fullscreen sub-screen rect: both now
live in `legaia_engine_ui::pause_menu` beside the composition that reads them.
Deleting a pair is therefore not always a loss of coverage; check which way it
went before restoring one.

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

## The fifth question: is a debug draw off on BOTH hosts?

Every question above asks whether a host *reaches* a surface. None of them can
see the shape where both hosts reach it and only one of them turns it off.

That shipped. The effect billboards carry a tinted wireframe outline so a spawn
stays readable when its texels are not resident; the native window gates it
behind `LEGAIA_DIAG_FX=1`, and the browser twin had no gate at all. Retail
draws no such rectangle, so every play-page fight stamped an opaque red-ish box
around every effect sprite - up to 25 at once - and a user reported it as a
rendering bug. Both hosts called the builder, the constants matched, the sim
pairs matched, no page carried a key table: **all four tiers above passed.**

[`DIAG_GATES`] declares every `LEGAIA_DIAG_*` env gate in the engine crates and
whether it is `additive` - whether it draws something retail does not. The
asymmetry is the whole point:

* a **subtractive** gate (suppress a layer, blend off, draw slots [a,b) only)
  can only remove pixels, so a host without it still renders retail-correctly
  and merely cannot bisect;
* an **additive** gate paints what retail never paints, so a host without it
  paints that thing in normal play, for every user.

So only additive gates require a twin. A WASM module has no process
environment, which is why this cannot be checked by looking for the env name on
both sides - the browser twin is a module static a page or console flips, and
the check is that its *initialiser* is false. Validated both ways, like the
waivers: an undeclared `LEGAIA_DIAG_*` fails (declare what it draws), and a
declared gate that no longer exists fails (drop the row).

Scope, stated as narrowly as the tiers above:

* it DOES prove every diagnostic env gate is declared, and that each additive
  one has a browser twin whose initialiser reads false;
* it does NOT prove the two gates suppress the *same* draw, that the twin is
  wired to anything, or that no un-gated debug draw exists under another name.

## Three more questions the tiers above were blind to

Each of the three closes a shape that a side-by-side reading of the two hosts
found with every tier green, and each is written up in
`docs/tooling/host-drift.md` with the bug that motivated it.

**Ownership** ([`OWNED_TYPES`]). A host can reach an engine type's outputs
without ever holding the type. The play page framed the field with a spherical
orbit of its own and held no `engine_core::camera::Camera`, so nothing there
routed the op-`0x45` beats into a controller, advanced the mover or wrote the
retail camera globals back - one absence producing a projection difference, a
simulation difference and two missing screens. No tier above can see a type
that is *absent*: there is no builder to miss, no constant to pair and no
injection site to diverge from. Ownership is a field declaration or a
construction in that host's own shipped source; a `use` line, a match arm and
a borrowed parameter are not.

**Variant coverage** ([`ENUM_COVERAGE`]). A shared enum's variant can be
entered on both hosts and answered on one - four minigame `SceneMode`s shipped
that way, reached through the shared scene host's door-warp drain and drawn
only natively, because their presentation is text lines and a hand-rolled 3D
scene rather than an `engine-ui` builder tier 1 enumerates. The variant list is
derived from the enum's own source, so a variant added tomorrow is measured;
what is declared is the enum and the per-(variant, host) waivers. A host
"answers" a variant by naming it *qualified* - an unqualified match would count
a same-named session type and report everything covered.

**Entry symmetry** ([`HOTKEY_SOURCES`]). The dance count-in and the how-to
actor were complete shared kernels that only the native window drove, from a
driver of its own reached by the `K` / `U` hotkeys - so *neither* host counted
in when a player walked into the hall. A gap that reads as "one host has it"
can be "one host's debug path has it", and the tiers cannot tell those apart
because both end at a live call site. For every `world.<method>()` the native
key arms call, this asks whether any call site exists outside the native
`bin/` tree. A debug probe is a legitimate answer and takes a waiver; the
waiver names the ARM (or the mechanism that superseded it), never a schedule.

Usage:

    python3 scripts/ci/check-ui-host-drift.py            # check, exit 1 on drift
    python3 scripts/ci/check-ui-host-drift.py --quiet    # findings only
    python3 scripts/ci/check-ui-host-drift.py --list     # full surface table
    python3 scripts/ci/check-ui-host-drift.py --selftest # detector control suite

Exit status: 0 = clean, 1 = drift / stale waiver / constant mismatch /
sim-pair mismatch, 2 = self-test failed.
"""
