# Site redesign brief — shared chassis for all 5 variants

## What we're fixing (diagnosis of the current site)

The current site is a three-column dark wiki. Confirmed problems:

1. **One flat sidebar with ~70 links** mixes "Play the port" with "Scene V12 table".
   A player looking for the browser port scrolls past byte-level format specs.
2. **Every page gets the same chrome** — the interactive Enemy table page looks
   identical to the TMD byte-spec page. There is no visual signal for "this is a
   toy you can touch" vs "this is a reference document".
3. **Text-heavy** — the homepage opens with a 5-line paragraph, the Progress
   section has a 6-line methodology paragraph before the bars, every card has a
   3-4 line body. Interactive pages bury the tool below 2-3 sections of prose.
4. **Dev aesthetic throughout** — `$ legend-of-legaia-re` terminal branding,
   monospace headings everywhere, no imagery anywhere (cards are text-only).

## The shared UX chassis (EVERY variant implements ALL of these)

These are the structural fixes. The five variants differ in visual language only.

### C1. Two-zone information architecture
Top-level split in the header nav: **Explore** (interactive: play, viewers,
tables, patcher, minigames) vs **Docs** (guides, write-ups, subsystems, formats,
tooling, reference). Plus a GitHub link.
- Explore pages: **no global sidebar**. Full-width app-like layout.
- Docs pages: sidebar contains ONLY docs sections (grouped, collapsible) + a
  right TOC rail. Never lists the interactive pages.
- The two zones must be visually distinguishable at a glance (how much they
  differ is a per-variant decision — variant E maximizes it).

### C2. Hero that shows, not tells
One-line H1 + one short sub-line + two CTAs: primary **"Play in your browser"**,
secondary **"Browse the disc"** or **"Read the docs"**. The legal note becomes a
compact badge/pill ("Zero Sony bytes shipped — bring your own disc"), not a bold
paragraph. Media (the demo video, shown as a poster/still placeholder) is
visually dominant.

### C3. Visual cards, one-line bodies
Explore cards get a **thumbnail area** (in mockups: inline SVG / CSS-gradient
placeholder art per card — NO game imagery, see constraints) + title + ONE line
(≤ ~75 chars). Cards are grouped: Play / Browse the disc / Game data / Modding.

### C4. Disc-status chip
The disc image is picked once and cached for every page (IndexedDB). Surface
that as a persistent header chip: `● No disc loaded — pick once, works
everywhere` → `● Legend of Legaia (USA).bin ✓`. Kills the per-page "Open a disc
image" wall of text (it collapses to a small drop-zone strip only when no disc
is loaded).

### C5. Tool-first explore pages
On explore pages the interactive content is the FIRST thing under the page
title. All explanatory prose ("Reading the columns", caveat boxes, stat-boost
footnotes) demotes to collapsible "About this data" / info-icon popovers /
footnotes BELOW the tool.

### C6. Docs pages keep depth, gain readability
Byte-level content stays. Add: a **metadata strip** under the H1 (confidence
badge e.g. CONFIRMED, provenance link "traced from FUN_80026B4C +3 more", the
implementing crate e.g. `crates/tmd`), readable measure (~70ch), styled byte-
layout tables, sticky right TOC.

### C7. Progress as a stat strip
Four compact stat tiles (big number + one-line label), one accent hue for the
meter fills, labels/values in text ink (never colored text), one "how these are
measured" link. No methodology paragraph on the homepage.
Data: Decompilation **99.8%** of exe code bytes · Asset formats **99.5%** of
disc bytes recognized · Engine port **825** functions ported (99.9%) · Port
wiring **80.4%** reachable from an entry point.

### C8. Write-ups surfaced as stories
Editorial cards (title + hook line + reading time feel), not a single generic
card. They are the narrative on-ramp for non-experts.

## Mockup deliverable — one self-contained HTML file per variant

File: `design-proposals/<variant-key>.html`. Self-contained: ALL CSS inline in
a `<style>` block. Google Fonts `<link>` is allowed (the real site uses Google
Fonts). NO JavaScript beyond trivial (e.g. a details/summary works without JS;
a tiny script for a tab switch is OK). No external images — placeholder
thumbnails are inline SVG or CSS gradients.

The file is ONE scrolling page with THREE full-bleed demo sections, separated
by an obvious divider bar (`SECTION 1 / 3 — HOME`, etc.):

1. **HOME** — full homepage: header/nav, hero, stat strip, explore card grid
   (all 12 cards from content.md), stories row, docs entry section, footer.
2. **EXPLORE PAGE** — the Enemy table page in this variant's app chrome:
   zone-appropriate header, tool-first layout using the sample table rows from
   content.md, a fake 3D-model side panel (placeholder), collapsed "About this
   data" details, disc chip states.
3. **DOCS PAGE** — the "Legaia TMD (3D mesh)" spec page in this variant's docs
   chrome: docs sidebar (grouped nav from content.md), metadata strip, prose +
   byte-table sample from content.md, right TOC rail.

Every variant renders THE SAME content (copy in `content.md` verbatim) so the
comparison isolates design.

Include at the very top of `<body>` a fixed, small variant banner:
`Variant <letter> — <name> · <one-line thesis>` so screenshots are identifiable.

## Hard constraints

- **No Sony imagery, no game screenshots, no character art** in mockups. All
  thumbnail art = abstract placeholders (wireframe meshes, waveforms, dice,
  map contours as inline SVG). Real thumbnails come later via engine captures.
- Dark variants: test that text contrast is comfortable. Light variant: same.
- Meters/stat tiles: single accent hue for fills; numbers and labels in text
  ink, not accent-colored; no multi-hue rainbows.
- Responsive enough to not break at 900px width (simple stacking is fine).
- Keep the project's credibility: this is a serious RE project. Playful ≠ toy.
  No emoji in headings or nav. No lorem ipsum — use content.md.
- Desktop-first at 1440px; that's how it will be screenshotted and compared.

## The five variants (visual language ONLY differs)

### A — `refined-terminal` · "Keep the soul, fix the ergonomics"
Evolution, not revolution. Stays dark (#0d1117 family) with the cyan accent,
but: Inter (or similar humanist sans) for ALL headings and UI — monospace
retreats to code, data tables, addresses, and the disc chip. Softer larger
type scale, more whitespace, wireframe-style SVG thumbnails (single accent
stroke on dark). The safest option; reads as "the same site, grown up".

### B — `ra-seru` · "A game site first, a lab notebook second"
Game-forward dark. Deep indigo/charcoal night palette; ember/flame gold-orange
primary accent (Ra-Seru fire), with restrained per-zone accents (Explore =
ember, Docs = cool steel, Stories = violet). A display serif with presence
(e.g. Cormorant Garamond / Marcellus) for the hero + section titles, humanist
sans body. Big cinematic hero with a gradient-overlaid video poster. Chunky
rounded cards, glow used sparingly. Should feel like a modern fan-site for the
game that happens to contain a world-class RE archive.

### C — `archive-light` · "The museum catalog"
Light editorial. Warm paper background (#faf8f3-ish), near-black ink, a fine
serif display (Newsreader / Source Serif 4) with sans support, hairline rules,
numbered sections (01, 02…), exhibit-style framed thumbnails, generous
whitespace, small-caps labels. Docs pages read like a beautifully typeset spec/
field guide. The contrarian option: everything else in this space is dark.

### D — `console-dashboard` · "A launcher, not a wiki"
App-shell. Slim persistent top bar + a narrow left icon rail (zone icons with
labels on hover — in the mockup just render icons + tiny labels). Home is a
console-style tile grid: one large hero tile ("Play"), medium tool tiles with
status badges (READY / NEEDS DISC), a stats row, a stories shelf. Dark neutral
graphite + one electric blue accent (PlayStation nod), squared tiles, subtle
depth. Docs open in a distinct "reader" surface (lighter panel tone, serif-ish
reading typography) inside the same shell.

### E — `split-portal` · "Two sites under one roof"
Maximal separation. The homepage hero is a split portal: left half **Play &
Explore** (rounded, colorful, soft gradients, friendly sans e.g. Nunito Sans),
right half **Under the Hood** (austere terminal mono, near-current aesthetic,
kept deliberately). Picking a zone themes ALL chrome for that zone — the
Explore demo section renders fully in the playful identity, the Docs demo
section fully in the terminal identity. One shared thin meta-header (project
name, GitHub, disc chip) bridges them so it still feels like one project.

## Self-check before finishing (each variant)

Screenshot your file headless at 1440×900 (full-page too), open the PNG, and
fix what looks broken: overflow, cramped cards, unreadable contrast, dead
whitespace, font pairs that clash. Iterate until it genuinely looks like a
designer made it. The bar: a stranger should instantly tell the three sections
apart AS home / app / document, and should want to click "Play".
