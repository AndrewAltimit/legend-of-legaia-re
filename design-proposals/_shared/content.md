# Shared content pack — use this copy VERBATIM in every variant

## Header / global

- Project name: **Legend of Legaia RE**
- Header nav zones: **Explore** · **Docs** · GitHub (external icon/link)
- Disc chip states:
  - empty: `No disc loaded — pick once, works on every page`
  - loaded: `Legend of Legaia (USA).bin ✓`
- Footer line: `Clean-room reverse engineering of Legend of Legaia (PSX, 1998).
  No Sony data is distributed — the site runs from your own disc image.
  MIT OR Unlicense.`

## SECTION 1 — HOME

### Hero
- Eyebrow (small, above H1): `PSX · SCUS-94254 · 1998`
- H1: `Legend of Legaia, taken apart and running again.`
- Sub (ONE line-ish): `A 1998 PlayStation RPG reverse-engineered end to end —
  play the clean-room engine port, browse everything on the disc, and read how
  it all works. All in your browser.`
- Legal badge/pill: `Zero Sony bytes shipped — bring your own disc`
- Primary CTA: `Play in your browser`
- Secondary CTA: `Browse the disc`
- Tertiary link: `Read the docs →`
- Hero media: 16:9 video-poster placeholder (abstract SVG/gradient + a play
  button glyph), caption-ish: `2-minute demo — the engine running real scenes`

### Stat strip (4 tiles; link below: "How these are measured →")
| Value | Label |
|---|---|
| 99.8% | of the executable's code decompiled |
| 99.5% | of disc bytes resolve to a documented format |
| 825 | retail functions ported to clean-room Rust |
| 80.4% | of ported code wired into the live engine |

### Explore grid — 12 cards in 4 groups
Group **Play**
1. **Play the port** — `The clean-room engine running the real game — flat or in VR.` (thumb: game controller / play glyph)
2. **Minigames** — `Slots, Noa's dance, Baka Fighter — playable with retail data.` (thumb: dice / reels)

Group **Browse the disc**
3. **Asset viewer** — `Every 3D mesh and texture on the disc, grouped by scene.` (thumb: wireframe cube)
4. **Media browser** — `All the music, sound banks, FMVs, and voice audio.` (thumb: waveform)
5. **Game world** — `Every town and dungeon assembled in 3D, with enemy rosters.` (thumb: map contours)
6. **World overview (3D)** — `Each kingdom's overworld in real-time WebGL.` (thumb: globe / terrain grid)
7. **Characters** — `Vahn, Noa, and Gala — field and battle models, animated.` (thumb: humanoid wireframe)
8. **NPCs** — `Every townsperson in 3D, posed straight from your disc.` (thumb: two small figures)

Group **Game data**
9. **Enemy table** — `Stats, drops, steals — click a row to spin the 3D model.` (thumb: table/grid)
10. **Shops & vendors** — `Every shop's priced inventory, town by town.` (thumb: coins/tag)
11. **Tactical Arts** — `Inputs, AP costs — each art performed by the battle model.` (thumb: d-pad arrows)

Group **Modding**
12. **ROM patcher** — `Randomize drops, shops, doors and more — nothing uploaded.` (thumb: shuffle/patch glyph)

### Stories row (3 editorial cards)
- **Patching a sealed disc** — `Modding a retail CD with no source and no SDK —
  a six-tier ladder from byte edits to hand-assembled MIPS.` · `6-part series`
- **The endless orbit** — `A softlock 27 years old: why one Gaza Valley fight
  never ends, and the two-byte fix.` · `write-up`
- **The Spirit fish gate** — `The fish that couldn't be caught — tracing a
  minigame gate nobody understood.` · `write-up`

### Docs entry section (4 tiles/links)
- **Start here: how the game is put together** — the layer diagram from raw
  sectors to runtime VMs. → `Architecture`
- **Subsystems** — boot, asset loader, five runtime VMs, renderer, audio,
  battle, world map. `24 pages`
- **Formats** — byte-level specs for everything on the disc, each traced to a
  Ghidra dump. `40+ pages`
- **Guides & tooling** — build the workspace, extract assets, mod and
  translate. `Reference` — RAM map, function directory, game-data tables.

## SECTION 2 — EXPLORE PAGE (Enemy table)

- Zone breadcrumb: `Explore / Enemy table`
- H1: `Enemy table`
- One-liner under H1: `Every enemy's stat record and 3D battle model, decoded
  from your own disc in the browser.`
- Disc chip: show the LOADED state here (`Legend of Legaia (USA).bin ✓`)
- Toolbar above table: search input (`Filter enemies…`), region dropdown
  (`All regions`), a `Columns` button, `Export glTF` button (disabled-looking).
- The table IS the page. Sample rows (curated walkthrough data, safe to ship):

| # | Enemy | HP | ATK | UDF | LDF | INT | SPD | EXP | Gold | Drop |
|---|---|---|---|---|---|---|---|---|---|---|
| 001 | Zenoir | 38 | 20 | 10 | 10 | 4 | 6 | 6 | 12 | Healing Leaf |
| 002 | Gimard | 52 | 24 | 12 | 11 | 6 | 7 | 9 | 18 | Antidote |
| 003 | Vera | 60 | 22 | 11 | 13 | 14 | 9 | 11 | 20 | Healing Leaf |
| 004 | Theeder | 74 | 28 | 14 | 12 | 8 | 11 | 14 | 24 | Power Elixir |
| 005 | Caruban | 310 | 46 | 22 | 20 | 12 | 14 | 60 | 120 | Healing Flower |
| 006 | Berserker | 1200 | 74 | 38 | 34 | 20 | 18 | 300 | 500 | Guardian Water |

Row 005 (Caruban) renders as SELECTED, with a side/detail panel: placeholder
rotating-model viewport (wireframe SVG), `Animations: idle · attack · hit ·
death`, small `Download .glb` button, and the steal line: `Steal: Healing
Flower (35%)`.

- Below the tool, a collapsed `<details>`: summary `About this data — columns,
  region differences, provenance`. Inside, two short paragraphs max:
  - `ATK/UDF/LDF/INT show in-battle values: the US/PAL battle loader boosts the
    raw on-disc record as it installs the enemy (ATK ×5/4, UDF ×2, LDF ×2,
    INT ×9/8).`
  - `Steals live in a separate executable table, not the monster record — which
    is why a monster's steal and drop usually differ. Traced in FUN_801DA51C;
    format spec: Monster archive.`

## SECTION 3 — DOCS PAGE (Legaia TMD)

- Docs sidebar groups (abbreviated nav, render these):
  - Guides: Getting started · Extracting assets · Playing + viewing · Modding + translation
  - Write-ups: Patching a sealed disc · The endless orbit · The Spirit fish gate
  - Subsystems: Boot path · Asset loader · Field/event VM · Renderer · Audio · Battle · (+18 more)
  - Formats: Disc geometry · PROT.DAT · LZS · TIM · **TMD ← active** · VAB · SEQ · MES · ANM · (+30 more)
  - Tooling · Reference
- Breadcrumb: `Docs / Formats / Legaia TMD`
- H1: `Legaia TMD (3D mesh)`
- Metadata strip: badge `CONFIRMED` · `traced from FUN_80026B4C · FUN_800268DC
  · FUN_8002735C` · `implemented in crates/tmd` · `last verified against
  SCUS-94254`
- Lede: `TMD is the PlayStation SDK's standard 3D model format. Every 3D mesh
  in Legend of Legaia is a TMD — but it is a custom Legaia variant, not the
  stock Sony format: standard PSX TMD tools reject these files or produce
  garbage. This page documents the variant byte-for-byte.`
- H2: `Not stock PSX TMD` — prose: `The tell is the very first word: Legaia's
  magic is 0x80000002 instead of the standard 0x00000041. Beyond the magic,
  the differences run deeper:` then 3 bullets:
  - `Custom primitive grouping — primitives are batched behind an 8-byte group
    header instead of the stock per-primitive packet stream.`
  - `Offset pointers — the object table's pointer fields are byte-relative
    offsets the runtime patches to absolute RAM addresses on load.`
  - `A fixed scale word — the per-object scale field is always 0x00808080, a
    Legaia-custom value where stock TMD stores a signed log2 scale.`
- H2: `Header (12 bytes)` — byte-layout table:

| off | size | field | notes |
|---|---|---|---|
| 0x00 | u32 | id | always 0x80000002 · bit 31 = FLIST_BIT (pointers relative to header end) · low byte 0x02 = "Legaia TMD v2" |
| 0x04 | u32 | flags | 0 on disc; runtime sets 1 after pointer fixup |
| 0x08 | u32 | nobj | number of objects |

- H2: `Object table (28 bytes per object × nobj)` — 2-line placeholder prose is
  fine (`Each object record points at vertex, normal, and primitive arrays…`)
- Right TOC rail entries: Not stock PSX TMD · Header (12 bytes) · Object table
  · Vertex / normal data · Primitive section · Per-prim colour block · Worked
  example · See also
