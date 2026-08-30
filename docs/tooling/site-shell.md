# Static-site app shell + its silent-shadowing gates

The pages under `site/` share one chrome, injected at runtime by
`site/js/layout.js` and styled by `site/css/styles.css`. This page covers what
that chrome is at each viewport width, the navigation invariant the narrow
layout has to preserve, and two gates against code that keeps reading like a
working feature after it has stopped being one: a breakpoint that loses to a
later rule, and a case fold that covers one operand instead of the string.

## The four chrome pieces

| Piece | Class | What it navigates |
|---|---|---|
| Top bar | `.topbar` | Brand, zone tabs (Explore / Docs), search, disc chip, GitHub. |
| Icon rail | `.rail` | The five top-level areas: Home, Play, Mods, Browse, Docs. |
| Sidebar | `.sidebar` | The page list *within* the current zone. |
| TOC rail | `.toc-rail` | Headings of the current docs page. |

The rail and the sidebar are **different destination sets**. The rail crosses
zones; the sidebar lists pages inside one. Neither is a subset of the other, so
hiding one and keeping the other is not a safe simplification on its own.

## Breakpoints

Three widths, each dropping one piece:

- **≤1100px** - the TOC rail goes. Docs pages become sidebar + article.
- **≤880px** - the icon rail goes and the sidebar becomes an off-canvas
  drawer. Content takes the full width. A hamburger appears in the top bar,
  `.sidebar-overlay` scrims the page, and the drawer opens on `.sidebar.open`.
- **≤620px / ≤420px** - the top bar sheds the zone tabs and the GitHub link
  (both live in the drawer), then collapses the disc chip to its status dot.

### The rail's destinations do not disappear with the rail

`buildZoneShortcuts()` renders the rail's five entries a second time as
`.sidebar-zones` at the top of the drawer. The strip is built on every page and
hidden by CSS wherever the rail itself is visible, so exactly one of the two is
on screen at any width. Without it, a phone on a docs page could reach Home
only through the brand mark and could not reach Play, Mods or Browse at all -
the docs sidebar deliberately omits them (`DOCS_SECTIONS` excludes the explore
section, and the `home` key is skipped from the tree).

The hamburger lives *inside* the top bar rather than floating over the page.
A floating button needs the content column to reserve a strip for it; an in-bar
button needs nothing, and can never sit on top of a paragraph.

## What makes a page wider than the phone

Three shapes, all of which size the *document* rather than only their own box,
because the page is laid out to its widest min-content:

- **A bare `<table>`.** It is `display: table`, and `overflow` does not make a
  scroll container out of a table box. `.table-wrap` is the scroller;
  `wrapWideTables()` in `layout.js` puts one around every prose table at layout
  time, before any page script builds tables of its own.
- **An unbreakable token** - `overlay_0897_801ef2b0`, `PTR_DAT_8007436C[id*3]`,
  a bare `ghidra/scripts/funcs/...` path in running text. `.content` takes
  `overflow-wrap: anywhere` below the breakpoint; it is inherited, so it
  reaches inline code, links, list items and table cells alike. `anywhere`
  rather than `break-word` on purpose - only `anywhere` lowers min-content
  width, which is the quantity being sized to. Scoped to the breakpoint so
  desktop column widths, computed from the same min-content, do not move.
  `pre` is unaffected: `white-space: pre` never wraps.
- **A canvas sized by its HTML attributes.** `<canvas width="600">` with no CSS
  cap lays out at 600px. Cap it with `max-width: 100%` - hit-testing that
  already scales by `canvas.width / rect.width` costs nothing.

## The cascade-order trap

A `@media` block contributes **nothing** to specificity. These two rules are
both specificity `(0,1,0)`, and the shipped stylesheet had them 2500 lines
apart in this order:

```css
@media (max-width: 880px) { .rail { display: none; } }
.rail { display: flex; }
```

The later one wins, at every viewport width. The breakpoint is dead code that
still reads like a working feature - and the failure is worse than "no mobile
layout", because the *other* half of the same block does apply: `.app`'s
`margin-left: 0` is the last `margin-left` for `.app`, so the content column
loses its offset while the fixed 76px rail keeps painting over it. Content laid
out full-width, rendered underneath the rail.

The second instance in that same block had the shorthand form: a
`.content { padding-top }` swallowed by a later `padding`. Both came from one
cause - the shell is declared in three places in a single long stylesheet, and
the responsive block sat above two of them.

**The convention:** `site/css/styles.css` ends with a `Responsive shell`
section holding every shell breakpoint, after all three shell declarations.
Adding a shell breakpoint anywhere else is how this comes back.

## Gate: `check-css-cascade-order.py`

```bash
python3 scripts/ci/check-css-cascade-order.py            # scan site/
python3 scripts/ci/check-css-cascade-order.py --selftest # controls only
python3 scripts/ci/check-css-cascade-order.py a.css      # explicit paths
```

It parses every stylesheet under `site/` plus the `<style>` blocks in
`site/_content/**/*.html`, and reports a media declaration only when a later
**unconditional** rule with the same normalised selector, a colliding property
(shorthands expanded), and specificity no lower would win. It runs on the
linked stylesheet and each page's own blocks in cascade order, so a page rule
shadowing a stylesheet breakpoint is caught too.

Shapes it stays silent on, because the later rule legitimately wins: a more
specific later selector, a later media block overriding an earlier one, a
generalisation of the media selector (lower specificity, so it loses anyway),
and an `!important` media declaration.

Ten positive/negative controls ship inside the file, and the positive ones run
on **every** invocation - a "clean" verdict from a detector that never matches
is the exact failure the gate exists to prevent. Wired into CI and into the
pre-commit hook when staged changes touch `site/`.

## One disc, one delivery

`site/js/rom-cache.js` holds the user's picked disc in IndexedDB so one pick
serves the whole site. `RomCache.attach(input, { onLoad })` therefore has **two
mouths** - the cached-disc auto-load on page init, and the input's own `change`
event - and they are not mutually exclusive. A returning visitor whose disc is
already cached, who then picks that same file anyway, gets both.

Every page rebuilds its whole engine per delivery (`new LegaiaRuntime()` +
`load_disc`), so the second delivery lands on top of whatever the first one
started. On the play page that is fatal, and it does not look like a double
load: the title card comes up normally, and a few seconds later the status
snaps back to `Disc loaded (…). Pick a boot mode.` over a dead canvas, because
the runtime the live title session was stepping has been replaced under it. The
same shape can strand a scene entered from the picker.

`attach` therefore keys deliveries by `(name, size)` and sequences them: the
same disc is never handed to `onLoad` twice, and a genuinely different disc
waits for the delivery in flight, so *last picked* wins rather than *last to
finish*. A failed delivery clears the key so a retry still gets through.

The reproduction is a control pair, not a screenshot: with a warm cache, pick
the file again, click **New game** as soon as the boot modes appear, and read
the HUD ten seconds later. Re-picking gives `hud-frame=0`; skipping the re-pick,
same profile and same timing, boots to `opdeene`.

## The 3D pages are WebGL2-only, and they now say so

Every 3D surface on the site takes a `webgl2` context and there has never been
a WebGL1 path. Three entry points ask for one: `webgl-tmd.js` (the shared
`TmdRenderer`, twelve construction sites - viewer, all five minigames, the play
page, world overview, characters, NPCs, world, mesh/field views),
`webgl-prim-replay.js`, and `monsters.html`, which carries its own renderer.

A null context used to raise a bare `Error('WebGL2 not available')`, and what a
user saw depended entirely on whether their page caught it:

| Page | Before | What it looked like |
|---|---|---|
| Asset viewer | caught; drops to its 2D-canvas flat-shaded loop | every model **untextured** |
| Minigames | uncaught | stage gone, floating HUD boxes and 2D faces on black - **scrambled art** |
| Play page | caught by `enter()`, status set to `<scene>: WebGL2 not available` | black canvas, no scene, no new game |

All three read as a broken site rather than a browser that cannot draw, which
is the most expensive way to be wrong: it sends people into the renderer after
a bug that is not there. It has already cost that once - a graphics-driver
update left a browser without hardware acceleration until it was restarted, and
these pages answered with untextured geometry instead of naming the cause.

So `window.legaiaWebgl2Failure()` (in `site/js/main.js`, loaded on every page)
raises a sticky banner naming the real condition and returns the `Error` for
the caller to throw. Control flow is unchanged - the viewer's fallback still
runs - the failure just explains itself first. The banner is `position: sticky`
under the top bar on purpose: the canvas that failed is usually well below the
fold, so a banner pinned to the top of the document scrolls out of sight
exactly when the user reaches the blank area it explains. Its text leads with
**restart the browser after a graphics-driver update**, because that is the
case that actually happened and the one nobody guesses.

**A WebGL1 fallback is not worth building.** Every shader is `#version 300 es`,
and the renderer leans on WebGL2-only constructs throughout - `usampler2D` and
`texelFetch` against the `R16UI` VRAM texture *are* the PSX CLUT decode, plus
vertex-array objects and instanced draws. That is a renderer rewrite, not a
fallback. The viewer's flat-shaded path is not a reusable one either: it is a
2D-canvas software rasteriser wired to that page's own wasm API, and it is what
made a missing context look like a texturing bug in the first place.

Reproduce it without touching a driver - override the context request, which is
exactly the observable condition:

```js
await ctx.addInitScript(() => {
  const orig = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (type, attrs) {
    return type === 'webgl2' ? null : orig.call(this, type, attrs);
  };
});
```

## Script cache-busting is content-addressed

`site/_gen.py` rewrites every `js/*.js` reference in every generated page to
carry `?v=<content hash>`, discarding whatever marker the `_content` source
had. Do not hand-write a version marker in `_content` - it is overwritten, and
maintaining one is maintaining a lie.

Hand-written markers were the previous scheme and they fail silently: they only
bust when someone remembers to bump them, and a forgotten bump is
indistinguishable from a correct deploy in a diff, in `git log`, and in a fresh
browser. `webgl-tmd.js` and `webgl-math.js` both changed content while still
shipping `?v=zfight-1`; `layout.js` carried no marker at all. A content hash
cannot be forgotten - it changes exactly when the bytes change, and only then.

This does **not** cover `site/wasm/`. The engine bundle is loaded by a bare
`import('./wasm/legaia_web_viewer.js')` with no query, and the glue then fetches
its own binary through `new URL('legaia_web_viewer_bg.wasm', import.meta.url)` -
a URL query on the glue does **not** propagate to that fetch, so busting only
the glue is a half-fix. Both would have to be handled together, via a loader
that passes an explicit versioned URL into the wasm-bindgen `init`. What the
skew actually costs today is measured in
[`shipped-bundle-freshness.md`](shipped-bundle-freshness.md#what-a-stale-engine-against-fresh-pages-costs).

## A case fold binds to one operand, not to the concatenation

A filter box has two halves that must agree on case: the query, and the
haystack it is matched against. JavaScript makes it easy to write a fold that
covers only part of the second one, because `.toLowerCase()` binds to the
operand it is written on:

```js
// Wrong: only the SECOND literal is folded.
return `${t.tier} ${t.label} ` +
  `${t.block} ${t.replaceable}`.toLowerCase();

// Right: fold the finished string.
return (`${t.tier} ${t.label} ` +
  `${t.block} ${t.replaceable}`).toLowerCase();
```

The ROM patcher's texture grid shipped the first form. Its query was
lowercased, its haystack's label half kept the disc's own capitals, and so
every capitalised word in a label was unsearchable: typing `ra-seru` returned
nothing while `Gala - Ra-Seru Ozma $1` sat in the grid, and the label preset
chips - whose text is a label's lead segment - filtered to zero when clicked.

Nothing else catches this. The page loads, the scan runs, the grid fills and
the filter filters; the only symptom is that a search misses rows that are
present, which is indistinguishable from "the disc does not have that
texture". The row data was correct the whole time - the defect lived entirely
in the join between query and haystack.

## Gate: `check-js-case-fold-chains.py`

```bash
python3 scripts/ci/check-js-case-fold-chains.py            # scan site/
python3 scripts/ci/check-js-case-fold-chains.py --selftest # controls only
python3 scripts/ci/check-js-case-fold-chains.py a.js       # explicit paths
```

It reports a `.toLowerCase()` / `.toUpperCase()` whose receiver is a string or
template **literal** that is an operand of a `+` - `FOLD_TAIL_ONLY` when the
`+` precedes it, `FOLD_HEAD_ONLY` when it follows. A fold on an identifier, on
a call result, or on a parenthesised expression is not reported: those fold
what they say they fold. Waive in place with `// case-fold-ok: <reason>`; a
reason-less waiver is itself a finding.

Ten positive/negative controls ship inside the file and the positive ones run
on every invocation. Wired into CI and into the pre-commit hook when staged
changes touch `site/`.

## Module-only syntax kills a classic script whole

`import.meta`, static `import ... from` and `export` declarations are legal
only inside `<script type="module">`. In a classic `<script src=...>` any one
of them is a **parse error for the entire file**: no line of it runs, and the
page silently degrades to whatever its static HTML shows - controls present,
nothing wired. The world-overview viewer shipped dead exactly this way: its
wasm loader gained one `import.meta.url` (pasted from the module-loaded ROM
patcher app, where the same line is correct), and the one syntax error took
out the whole 1400-line app with every other gate green. Only a browser
parses these files, so nothing but a browser - or a checker - sees it.

The legal classic-script forms, used by `arts-viewer.js` / `summon-view.js`:
dynamic `import()` (an expression, allowed anywhere), and URL resolution
against the *page* via `new URL('wasm/...', document.baseURI)` instead of
against the script via `import.meta.url`.

## Gate: `check-js-classic-module-syntax.py`

```bash
python3 scripts/ci/check-js-classic-module-syntax.py            # audit site/
python3 scripts/ci/check-js-classic-module-syntax.py --selftest # controls only
```

It joins two facts no single file carries: which `.js` files the committed
page sources (`site/_content/**/*.html` + `_gen.py`'s shell template) load
without `type="module"`, and which files use module-only syntax outside
comments and strings (`IMPORT_META_IN_CLASSIC` / `STATIC_IMPORT_IN_CLASSIC` /
`EXPORT_IN_CLASSIC`). A file referenced by no scanned source is skipped - a
dynamically-`import()`ed module is entitled to module syntax. Waive in place
with `// classic-script-ok: <reason>`; a reason-less waiver is itself a
finding. Controls ship inside the file and the positive ones run on every
invocation. Wired into CI and into the pre-commit hook when staged changes
touch `site/`.

## A `_content` mirror of a `docs/` page rots silently

Several `site/_content/` fragments are *mirrors*: a page under `docs/` is the
source of record and the fragment restates it for the public site. Nothing
relates the two. The generator's page table maps `_content/<x>.html` to
`<x>.html` and never opens `docs/` at all, so the only thing keeping a mirror
current is somebody remembering it exists.

The failure mode is not a broken page. A stale mirror builds, renders, passes
`check-site-links.py`, and reads as finished - it has simply stopped mentioning
whole areas of its source, and a reader has no way to tell. The open-RE-threads
mirror shipped with **no battle section at all** while its source had one, and
went on describing a thread as open after the doc closed it by capture. Both
halves matter: an absent area is invisible, and a present-but-stale area is
worse, because it is confidently wrong.

`scripts/ci/check-site-doc-mirrors.py` makes the relation explicit and
mechanical. Every `##` heading of the source doc must be claimed by a section
of the mirror through a `data-doc` attribute carrying that heading's GitHub
anchor slug:

```html
<section class="doc-section" id="battle-rendering" data-doc="battle--rendering">
```

One section may claim several headings (comma-separated) where the mirror
deliberately merges them; a section that exists only on the site claims
`data-doc="-"`. A slug naming no heading in the source is an error too - that
is what a *renamed* heading looks like, and it is the half a "did you cover
everything" check on its own would miss.

What the gate deliberately does not check: prose, tables, row counts, or
whether the mirror says the same thing as its source. A section can be present
and wrong. It answers one question - is a whole area of the source missing from
the public page - because that is the failure that survived review.

## Verifying a shell change

Static reading is not enough - the trap above is invisible in a diff and
invisible in a single-width screenshot. Render it:

1. `python3 site/_gen.py`, then serve `site/` over HTTP.
2. Drive headless Chromium (see [`.claude/skills/verify`](../../.claude/skills/verify/SKILL.md))
   at 360 / 390 / 768 / 880 / 881 / 1280 px.
3. Assert, per page: `document.documentElement.scrollWidth` never exceeds
   `clientWidth`; the content column's box does not intersect a visible
   `.rail`; the hamburger is displayed below the breakpoint and hidden above
   it; and the set of `.rail` hrefs is a subset of the hrefs reachable from
   the top bar plus the drawer.

The last assertion is the one that catches a "tidy" mobile layout that has
quietly dropped a destination.

### Reaching the play page's battle screen headlessly

The battle screen is the play page's least-reachable surface, and for a long
time it was not reachable at all from a driver: a battle needs a step roll, the
retail towns are all rate-0 by design so they never produce one, and the
native-only `debug_start_test_battle` is `#[cfg(not(target_arch = "wasm32"))]`
and never crossed into the bundle. Anything shipped to the battle screen was
therefore verified on the native window alone.

`LegaiaRuntime::debug_force_battle(row)` is the wasm-side twin of the native
`--battle <ROW|first>`: it resolves a formation row against the scene's own MAN
formation table, turns the live loop on (the transition is drained by the field
tick, so an armed fight cannot open without it) and hands the row to
`World::force_encounter` - the same entry an organic encounter takes, so the
intro, the BGM swap and the battle load are the ordinary ones.
`debug_formation_rows()` returns the rows the current scene registered, so a
driver picks one that exists instead of guessing.

```js
await page.evaluate(() => window.__playRuntime.debug_formation_rows());  // "[0,1,...]"
await page.evaluate(() => window.__playRuntime.debug_force_battle(4));   // -1 = "first"
```

Both are on the ordinary `#[wasm_bindgen]` surface rather than behind a
feature, because a debug entry point that is not in the shipped bundle is
exactly the thing that left this screen unverified.

Two driver notes. Give the fight ~150 sim ticks before expecting
`SceneMode::Battle` - the encounter transition runs first. And a popup-lifetime
surface (the floating damage numeral) needs the sim paused on the frame you
found it, or the screenshot lands after it expires: poll
`play_overlay_draws_json`, then `window.__playView.setPaused(true)`, then
shoot.
