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
| Top bar | `.topbar` | Brand, zone tabs (Explore / Docs), search, disc chip, theme toggle, GitHub. |
| Icon rail | `.rail` | The five top-level areas: Home, Play, Mods, Browse, Docs. |
| Sidebar | `.sidebar` | The page list *within* the current zone. |
| TOC rail | `.toc-rail` | Headings of the current docs page. |

The rail and the sidebar are **different destination sets**. The rail crosses
zones; the sidebar lists pages inside one. Neither is a subset of the other, so
hiding one and keeping the other is not a safe simplification on its own.

## The sidebar's page list

`NAV` in `layout.js` is the single source of truth for page order, the docs
sidebar, prev / next and the explore sidebar's groups. A page exists for the
shell only when it has a `NAV` entry, and exists at all only when `_gen.py`
writes it - a new page needs both. `check-site-links.py` holds the two tables
to each other: a `PAGES` row with no `NAV` key is an orphan (the sidebar,
prev / next and search never list it), and a `NAV` href the generator never
writes is a dead sidebar row. Neither appears as an href in the generated
HTML, so the page-link scan alone cannot see them.

Three fields shape what a reader sees:

- **`label` is a key, `title` is the text.** `label` keys `DOCS_SECTIONS`, the
  persisted collapse state and the search index, so it never changes; `title`
  is the reader-facing name (`subsystems` reads "How the game works").
- **`{ heading: '...' }` rows are group subheads.** They render as
  `.sidebar-subhead` inside a long section and are not pages: every walker of
  `NAV` items skips a row with no `key`, which is what keeps them out of
  prev / next and the explore groups. The sidebar wraps each subhead and the
  items up to the next one in a `.sidebar-group`: the subhead is drawn as an
  underlined accent label and the items hang off a guide rail
  (`.sidebar-group-items`), so a grouped section reads as a tree.
- **Long sections open collapsed.** `COLLAPSED_BY_DEFAULT` names the reference
  sections; one opens when it holds the current page or the reader opened it
  before. Five of them expanded at once is a 150-row list that buries the two
  short sections a newcomer wants.

Every long section is grouped, and the groups are not the sidebar's alone: a
section's index page (`site/_content/<section>/index.html`) presents its cards
under the same headings in the same order, and for the formats section so do
`docs/formats/overview.md` and the formats table in `CLAUDE.md`. Moving a page
between groups means moving it in each. Items inside a group run from overview
to detail; a companion page (internals, a runbook, a sub-reference) is
indented under the page it extends.

The explore sidebar is built from `EXPLORE_GROUPS` rather than from `NAV`
sections. A `'#Name'` entry in a group's `keys` is a subhead row there, the
explore counterpart of a `{ heading }` row.

Two things the shell derives from `NAV` instead of trusting the page:

- **The breadcrumb.** Each page's hand-written `.breadcrumb` is the no-JS
  fallback; `syncBreadcrumb()` rewrites it on docs pages as Home, the section
  (linked to its index when it has one), any series index or indented parent
  above the page, then the page's own sidebar label. Hand-written trails had
  drifted - some linked their section, some did not, and leaves kept the
  wording of a page's first draft.
- **The drawer's scroll position.** `revealActiveRow()` scrolls the current
  page's row into view after the saved scroll position is restored and every
  time the phone drawer opens. Without it the drawer opened on the two
  always-expanded sections, with the reader's own page below its fold.

## Themes

Dark is the default; light is opt-in through the top-bar toggle
(`buildThemeToggle`), which stores `site-theme` and sets `data-theme="light"`
on the root element. The page template's head carries a one-line script that
applies a stored choice before the stylesheet loads, so a light-theme reader
never sees a dark first paint.

The whole light palette is one block, `:root[data-theme="light"]`, restating
the custom properties declared on `:root`. That only works while components
take every colour from a property: a literal colour in a shared rule, a page
`<style>` block or a doc SVG is a spot the light theme cannot reach. The game
palette chips (element colours, art cards) are the deliberate exception - they
keep a dark ground in both themes rather than being re-picked.

## Shared components

Content pages are built from a fixed vocabulary, indexed in the comment block
at the top of `site/css/styles.css` with a longer note beside each rule.

| Component | Classes |
|---|---|
| Callout | `.callout` + `.callout-tip` / `-warn` / `-trap` / `-provenance`, optional `.callout-title` |
| Status badge | `.status-badge` + `.sb-confirmed` / `-inferred` / `-unknown` / `-works` / `-mostly` / `-partial` / `-rough` / `-planned` |
| Status list | `ul.status-list` (+ `.cols-2`) |
| Card index | `.card-grid` (+ `.cols-2..4`, `.compact`) of `a.card` |
| Spec list | `dl.kv` |
| Tables | `table.byte-layout`, `table.compact`, `table.narrow`, `td.num`, `td.hex` |
| Byte map | `.byte-map` of `.bm-cell` |
| Diagram | `figure.diagram` / `figure.pipeline` with the `dg-*` SVG classes; `ol.steps` |

`.notice`, `.hint` and `.tag-*` are older names for the callout and the badge
and render through the same rules. A card is itself the link, so it never
contains one: a link nested in a link is closed early by the HTML parser and
the card's remaining content lands outside it.

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
  time, before any page script builds tables of its own. A wrapped table keeps
  its words whole below the breakpoint (`overflow-wrap: break-word`, headers
  `nowrap`) and scrolls inside its frame - left to the `anywhere` rule below, a
  five-column table shrinks to five columns of single letters instead.
- **An unbreakable token** - `overlay_0897_801ef2b0`, `PTR_DAT_8007436C[id*3]`,
  a bare `ghidra/scripts/funcs/...` path in running text. `.content` takes
  `overflow-wrap: anywhere` below the breakpoint; it is inherited, so it
  reaches inline code, links, list items and table cells alike. `anywhere`
  rather than `break-word` on purpose - only `anywhere` lowers min-content
  width, which is the quantity being sized to. Scoped to the breakpoint so
  desktop column widths, computed from the same min-content, do not move.
  `pre` is unaffected: `white-space: pre` never wraps. A table inside a
  `.table-wrap` opts back out, as above; a table a page script builds is not
  wrapped and keeps `anywhere`.
- **A canvas sized by its HTML attributes.** `<canvas width="600">` with no CSS
  cap lays out at 600px. Cap it with `max-width: 100%` - hit-testing that
  already scales by `canvas.width / rect.width` costs nothing.

### A sticky header needs a wrapper that is not a scroller

A table header can stick to the page only if no ancestor of the table is a
scroll container, and `.table-wrap` is one by design. `fitTables()` resolves
it per table: where the table fits its column the wrapper takes `.fits`, which
drops the scroller and turns on `position: sticky` under the top bar; where it
is too wide the wrapper keeps the horizontal scroller and a static header. It
runs in the docs zone only, on load, on resize and once fonts settle.

## One right edge per column

Prose is capped at `--measure` (46rem) so a line stays readable. That is right
on the 880px docs column, where the measure is most of the column, and wrong
on a wider one: a 46rem paragraph or callout sitting among 1400px grids, tiles
and tables stops at half the page and reads as a section that failed to load.
Nothing errors, and it is invisible below about 1300px of viewport, where the
column is not yet wider than the measure.

Three column shapes, each with one rule for where text ends:

| Column | Pages | Text ends at |
|---|---|---|
| Docs (880px) | every docs page not listed below | `--measure` |
| App (1400 / 1480px) | home, `.wide-page` (`WIDE_PAGES` in `_gen.py`) | the column: `--measure` is `none` |
| Prose with a breakout | `.wide-page.prose-page` (`PROSE_WIDE_PAGES`), `.dp-page` | the docs column; the named exhibit alone is wide |

A `.prose-page` is a long-form page that is wide for one exhibit, such as a
many-column table. Its header and sections keep the docs column, and a
`section.doc-section.breakout` takes the full width and lifts the measure
inside itself, so the exhibit's own caption follows the exhibit. The
disc-patching write-ups (`.dp-page`) are the same shape with their own named
breakouts.

A breakout spans its grid cell, and in the docs zone that cell sits between
the sidebar and the TOC rail: at a 1440px viewport it is no wider than the
docs column, so widening the page's `max-width` alone gives the exhibit
nothing. A `.prose-page` therefore drops the TOC rail's column until the
viewport is wide enough to hold both (the `Responsive shell` block, 1101px to
1679px), which is what lets a ten-column table sit unscrolled on a 1440px
desktop.

The policy is one custom property, so it holds only while every prose cap is
spelled `max-width: var(--measure)`. A literal `72ch` on a new component is
exactly how the home page's port-status caption stayed at half width after the
status list beside it had been exempted by hand. Width caps that are not prose
measures - the hero copy beside its video, a card in a grid track - are sized
by their own layout and keep literal values.

Checking it needs a browser. Load each page at 1440 and 1920 wide and, for
every block container in `.content`, report a child narrower than 70% of the
container while a sibling spans it; then look at the flagged pages, since a
`.prose-page` section beside its breakout is flagged by design.

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
the HUD ten seconds later. Re-picking leaves the HUD's Scene at `-` (the engine
never ticked); skipping the re-pick, same profile and same timing, boots to `opdeene`.

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

`site/_gen.py` rewrites every `js/*.js` reference and every linked `css/*.css`
stylesheet in every generated page to carry `?v=<content hash>`, discarding
whatever marker the `_content` source had. Do not hand-write a version marker
in `_content` - it is overwritten, and maintaining one is maintaining a lie.

The stylesheet was the last unstamped file, and it shows what an unstamped
asset beside stamped ones does. `python3 -m http.server` sends `Last-Modified`
and no `Cache-Control`, so Firefox treats a cached `styles.css` as
heuristically fresh for a tenth of its age and reuses it after it has changed,
while the `layout.js` beside it is current. New chrome then renders against old
rules: a page served over the LAN came up partly styled. GitHub Pages sends
`max-age=600` and never showed it.

The web fonts are linked from the page head (`FONT_LINKS` in `_gen.py`), not
`@import`ed from `styles.css`. An `@import` holds the stylesheet's load until a
second, third-party fetch completes, and a parser-blocking stylesheet holds the
page's scripts with it - so a client with a slow route to the font origin got a
blank or half-built page for as long as that request took. The font sheet loads
as `media="print"` and switches itself to `all` on load, which keeps it off
the render-blocking path; text paints on the fallback stack until it arrives,
as `display=swap` already does.

Hand-written markers were the previous scheme and they fail silently: they only
bust when someone remembers to bump them, and a forgotten bump is
indistinguishable from a correct deploy in a diff, in `git log`, and in a fresh
browser. `webgl-tmd.js` and `webgl-math.js` both changed content while still
shipping `?v=zfight-1`; `layout.js` carried no marker at all. A content hash
cannot be forgotten - it changes exactly when the bytes change, and only then.

`site/wasm/` is versioned the same way, through one loader.
[`site/js/wasm-loader.js`](../../site/js/wasm-loader.js) is a classic script the
page template puts in every `<head>`; `LegaiaWasm.load()` imports the glue and
passes the binary URL into the wasm-bindgen `init` explicitly, both carrying
`?v=` from `window.LEGAIA_WASM_V` (a hash of the glue and the binary, written by
`_gen.py`). Both halves matter: the glue fetches its binary through
`import.meta.url`, which drops any query on the glue, so busting the glue alone
can pair new glue with a cached binary. The loader is also the only import site,
because a dynamic import is keyed by its URL - two call sites on one page that
spell it differently instantiate the bundle twice. Never `import()` the glue
directly from a page or script.

Helpers every page shares sit in one classic head script the same way:
[`site/js/site-util.js`](../../site/js/site-util.js) defines `window.escapeHtml`
(the five HTML entities; `null` renders empty) and `window.LegaiaUtil`:
`rgbaCanvas` (an RGBA8 export wrapped in an offscreen canvas, `null` on a
missing or mis-sized buffer), `downloadFile` (a Blob or byte array handed to
the viewer as a file) and `poseClipInto` (the minigame posers' per-object
`Rz.Ry.Rx . v + T` plus world yaw and floor offset). Inline page scripts,
classic scripts and modules call these instead of carrying a copy each, and
`scripts/ci/check-site-shared-helpers.py` (pre-commit and CI) fails on a new
definition under one of those names - or the local names the old copies wore
(`triggerDownload`, `escHtml`) - anywhere else in the site sources; a
`// shared-helper-ok: <reason>` comment on the line waives one.

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
`<x>.html` and, for an ordinary page, never opens `docs/` at all, so the only
thing keeping a mirror current is somebody remembering it exists. (The register
pages are the exception, rendered from `docs/` at build time; see
[below](#the-registers-are-rendered-not-mirrored).)

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

### A section is the wrong unit for a page made of rows

Heading coverage is necessary and not sufficient, and the same mirror proved it
a second time with every `##` area claimed. `open-rev-eng-threads.md` is a page
whose *content* is table rows - one row per live hunt - and the mirror was
showing two of the source's twelve. Three of its area tables were present and
**empty**, its battle table carried a thread that had closed, and its field
table carried one row where the source carries five. Nothing was missing at the
level the gate measured; the live-hunt list a reader saw was a sixth of the
real one, and one entry of it was an invitation to work that was finished.

So the gate runs a row half as well. A **live row** is a row of a
`| Thread | Status | ... |` table - identified by the header's own first cell,
because the three reference pages disagree on every column after it - whose
status cell opens `open`, `partial` or `mostly resolved`. Each is claimed by a
`data-row` attribute carrying the slug of its first cell:

```html
<tr data-row="where-does-prot-0896-link"><td>Where does PROT 0896 link?</td>
```

The rule scopes itself rather than needing an exemption list: those three
statuses are `open-rev-eng-threads.md`'s own vocabulary, so the settled and
falsified registers - several hundred thread rows between them, every one
`resolved` or `falsified` - contribute nothing, while a live row appearing on
one of *them* is a finding. As with `data-doc`, a `data-row` naming no live row
is an error: that is precisely what a closed hunt still advertised as open
looks like.

The mirror is also where a second copy of the list becomes a liability. A
"live threads at a glance" digest restating the area tables is another place a
closed thread can keep looking open, and the gate cannot tell a digest row from
an area row - both are claims. One place per row; the digest is jump links.

What the gate deliberately does not check, on a hand-written mirror: prose,
wording, or whether the mirror says the same thing about an area or a row it
claims. Either can be present and wrong. It answers exactly two questions - is
a whole area, or a live hunt, missing from the public page - because those are
the failures that survived review.

### The registers are rendered, not mirrored

The settled and falsified registers showed that "present and wrong" is not
hypothetical. Both hand mirrors claimed every area and still stated verdicts
their docs had overturned: a routine with six `jal` callers listed as
link-time dead code, a census floor listed as a total. A page whose content is
hundreds of rows that change on every correction cannot be kept in step by
hand, so the three register pages are no longer copied.

Each register fragment keeps its framing - header, lede, the page's own intro
sections, the filter box and its script - and marks where the register goes:

```html
<!--REGISTER-NAV:settled-->   the "Jump to" area list
<!--REGISTER:settled-->       every area, row and write-up
```

`site/_gen.py` expands the markers through `site/_registers.py` on every build,
from the current `docs/` (the settled register reads its index page and the
nine area pages under `re-settled-threads/`). A settled row renders its
verdict from the status parenthetical, the answer under *Why*, and the evidence
cell as grade tags; a falsified row renders the same way from `Thread | Verdict
| Why`; each `###` write-up becomes a collapsed entry under *Long write-ups in
this area*, and a `[details ↓]` link points at it on the same page. The
renderer counts the thread rows of every table it was handed and fails the
build when the page it produced carries a different number, so a row shape it
does not know is an error, not a silent drop. Links into a register doc
resolve to the rendered anchor on the site; other relative links go to the
site page when one exists and to the file on GitHub otherwise.

`check-site-doc-mirrors.py` covers the generated pages three ways: the marker
must be present (a fragment without it is a hand copy coming back), the
section and row checks run over the *expanded* fragment, and each expansion
opens with a `<!--register:<kind> sha=...-->` stamp of the source bytes it was
rendered from, which must match the current docs whenever a local build of the
page exists. CI builds on deploy, so there the stamp check has nothing to read;
locally it is what tells a stale preview from a fresh one.

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

Shoot both themes: set `localStorage['site-theme'] = 'light'` in an init
script before navigation. A component that reads a literal colour looks
correct in the dark screenshot and only shows in the light one.

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

### The play page's minigame status rows sit under the canvas

The engine's minigame status rows (`engine-minigame-scenes::minigame_status`) are the
port's own readout of the rules state plus the key hints - not retail chrome.
They are data, not a draw list: `minigame_status::caption` returns the live
game and its rows, and no builder emits them as glyph quads. The play page
draws a game's own chrome (screen primitives) over its text layer, as the
native window does, so a row in the frame would be covered by whatever the
game draws there - the Baka Fighter title logo sat on the prompt line - and a
long row ran off the frame's right edge.

The page reads the rows through one export, `play_mg_caption_json`:

```json
{ "game": "baka", "rows": [
    { "kind": "readout", "clauses": ["BAKA", "you 3200hp (wins 0)", "round 1"] },
    { "kind": "hint", "clauses": ["choose your attack", "Start = quit"] } ] }
```

| Field | Meaning |
|---|---|
| `game` | `slot` / `baka` / `muscle` / `dance` / `fishing`, or `null` outside a minigame |
| `rows` | reading order; may be empty while a game is live (a phase with nothing to say) |
| `kind` | `readout` (the rules state) or `hint` (what to press) |
| `clauses` | one statement each; the page separates them with a middle dot |

`captionSync` in `play-minigames.js` writes them into `#play-mg-caption`, the
strip under the canvas, where they wrap. It hides the strip when `game` is
`null` and keeps its two-line seat while a game is live, so the controls under
it do not jump between phases. The page holds no pen, ink or glyph constant:
what the strip shows is whatever the export says.

Two rows are decided in Rust, where they are authored. The dance beat track is
not a caption row - it is the game's own beat bar, which the HUD frame draws.
The slot machine's rows are a fallback for a machine that is not on screen:
the caption stays live and empty once the cabinet draws itself.

#### Fullscreen

The Fullscreen button presents `#play-screen`, which holds the canvas wrap and
the strip, so the rows come along. The strip takes its height out of the
picture's letterbox and is never drawn over the picture.

The element that goes fullscreen cannot itself be the 4:3 box. A browser's own
sheet sizes a fullscreen element to the whole screen with `!important`
(`width` / `height: 100%`, `inset: 0`, `margin: 0`), which no author rule
overrides. A wrap asked to be `min(100vw, 100vh * 4/3)` wide therefore comes
out the screen's full width, its 4:3 canvas runs taller than the screen, and
the bottom of the picture is cut off. `.play-screen:fullscreen` is a flex
column that centres the wrap, and the wrap takes its width from the height
left after the strip.

To reach a minigame from a driver, enter any scene and call
`window.__playRuntime.play_mg_debug_warp(n)`: 0 fishing, 3 slot machine,
4 Baka Fighter, 5 Muscle Dome, 6 dance. Use the GPU backend
(`--use-angle=vulkan --enable-gpu --ignore-gpu-blocklist`); software rendering
drops key presses at these frame rates.
