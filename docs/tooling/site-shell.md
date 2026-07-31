# Static-site app shell + the CSS cascade-order gate

The pages under `site/` share one chrome, injected at runtime by
`site/js/layout.js` and styled by `site/css/styles.css`. This page covers what
that chrome is at each viewport width, the navigation invariant the narrow
layout has to preserve, and the gate that keeps a breakpoint from silently
losing to a later rule.

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
