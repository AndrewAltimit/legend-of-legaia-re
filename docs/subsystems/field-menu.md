# Field Menu - Windows + Status Panel Renderer

Covers the field pause menu's **window system** (the window-descriptor table
that places every menu screen's bordered windows) and `FUN_801D33D8`, the
per-character **status / party panel** renderer. The field pause menu (game
mode `0x17`, the CARD-mode pair) opens the panel for the Status, Magic,
Moves, and Skills tabs; it draws one party member's page into a
caller-supplied window rect. Both live in the **menu overlay** (the same
binary as shop / inn / save; base `0x801CE818`). Source:
`ghidra/scripts/funcs/overlay_menu_801d33d8.txt` plus the shared draw
primitives `ghidra/scripts/funcs/80036888.txt` (string), `8002c488.txt`
(UI-icon sprite), `80034b78.txt` (decimal number); window-table pins from
the catalogued menu-open save states (RAM + VRAM, see below).

The panel draws **content only**. The bordered 9-slice window frame is emitted
by the caller, not here (this function never draws a box). Every position below
is an offset from the window origin, which the caller passes in the rect struct
`a0`: `WX = *(i16*)(a0+0xa)`, `WY = *(i16*)(a0+0xc)`. The rect also carries a
width-ish field at `a0+0xe` (scroll-arrow X and scrollbar length) and a height
field at `a0+0x10` (bottom-anchored scrollbar Y). The rect is caller data -
resolved through the window-descriptor table below.

## Contents

- [Window descriptor table](#window-descriptor-table) · [Live window structs](#live-window-structs)
- [Tab banner](#tab-banner) · [Status satellite windows](#status-satellite-windows)
- [Plumbing](#plumbing) · [Submenu dispatch](#submenu-dispatch)
- [Header row](#header-row-always-drawn) · [Status page](#status-page-submenu-0-or-5)
- [Magic list](#magic-list-submenu-2) · [Moves list](#moves-list-submenu-3) · [Skills page](#skills-page-submenu-1)
- [Equip screen](#equip-screen) · [Options screen](#options-screen)
- [Draw primitives + CLUT staging](#draw-primitives--clut-staging)
- [Record fields consumed](#record-fields-consumed)

## Window descriptor table

Every pause-menu window (rect + content renderer) comes from a 52-entry
table in the menu overlay's data segment at VA `0x801E473C` (PROT 0899 file
offset `0x15F24`; parser `legaia_asset::menu_windows`). Records are 0x10
bytes, indexed by window id:

| off | type | field |
|---|---|---|
| `+0x0..+0x7` | 4 × i16 | `x, y, w, h` - the **content** rect (the `a0+0xa..+0x10` rect the content renderer receives) |
| `+0x8` | u32 | content-renderer VA (menu-overlay function), 0 = frame-only window |
| `+0xc` | u16 | style/param word (low bits are per-renderer params; runtime-mutated on some windows) |
| `+0xe` | u16 | window class: 2 = title tab, 3 = standard, 4 = list page |

The table extent is structural: record 52 fails the rect/renderer validity
envelope. Provenance: byte-matched between the disc entry and the resident
overlay across the six catalogued menu-open mednafen states
(`menu_{status,equipment,options}_{field,town}`); only id 22's style low
bits and id 49's `y` (178 -> 180) differ at runtime. The drawn window frame
extends past the content rect by 8 px on every side (the RAM GPU-prim scan
of the `menu_status_town` capture places each window's 4x4 corner tiles at
`content - 8` - window 26's content `(14, 38)` frames from `(6, 30)` -
cross-checked against the captures' VRAM framebuffer edge pixels).

Screen window sets, read from the live window lists of the captures (each
live window carries its descriptor id):

| screen | windows (draw order) |
|---|---|
| top-level pause menu | 50 command list `(24,24,104,94)` -> `FUN_801CFD68`; 49 money/play-time box `(24,178,104,24)` -> `FUN_801D0148`; 51 right party panel `(144,24,152,180)` -> `FUN_801D030C` |
| Status | tab 3 -> `FUN_801DCAD8`; 26 party list `(14,38,60,38)` -> `FUN_801D2094`; 27 "Condition" pager `(14,92,60,10)` -> `FUN_801D30A4`; 30 summary `(14,134,60,70)` -> `FUN_801D31EC`; 28 **main panel** `(90,16,218,188)` -> `FUN_801D33D8` |
| Equip | tab 2 -> `FUN_801DCA94`; 21 party `(14,42,80,38)` -> `FUN_801D2094`; 23 item list `(174,22,132,182)` (renderer-less container; its lower span is occluded by 22); 22 main `(14,96,292,108)` -> `FUN_801D21C0` |
| Options | tab 4 -> `FUN_801DCB1C`; 48 settings `(24,40,256,148)` -> `FUN_801DCEF0`; 47 value popup `(170, *, 128, *)` -> `FUN_801D2B44` (y/h stamped per open - see [Options screen](#options-screen)) |

The id-28 rect origin `(90, 16)` is the `(WX, WY)` every offset in the
status-page sections below hangs off - cross-checked against the captured
framebuffer (HP row ink at `WY+0x13`, stat grid at `WY+0x42/+0x4f/+0x5c`,
right stat column at `WX+0x74`).

## Live window structs

The engine spawns windows as a doubly-linked list of 0x5C-stride structs
(seen at `0x800AB7BC..` in the captures): `+0x0`/`+0x4` = next/prev,
`+0x8` = descriptor id, `+0xa..+0x11` = the **live** rect. The live rect is
the window's animated position: windows slide to the nearest screen edge on
screen exit and park offscreen (x = 332 right, x = -124 left, y = 240
bottom in the captures - the `menu_options_field` state caught three
status-screen windows mid-slide). The top-level windows 49/50/51 stay
parked in every sub-screen capture, which is how the top-level set was
pinned without a top-level capture.

## Tab banner

The class-2 title-tab windows (descriptor ids 0..=4 - "Status" / "Equip" /
"Options") draw **no gold 9-slice frame or filigree interior**. Their
entire chrome is the carved brown **plaque**, composed of six textured
sprites (RAM prim scan over the `menu_status_town` capture, all CLUT row
12 of the system-UI sheet at `PROT.DAT[0x018E0]`):

| piece | src rect | placement |
|---|---|---|
| left cap | `(208, 64, 8, 20)` | `(WX-8, WY-4)` |
| body tile | `(192, 64, 16, 20)` | tiled from `WX` across the content width `w` (partial remainder) |
| right cap | `(216, 64, 8, 20)` | `(WX+w, WY-4)` |

The tab's content renderer (`FUN_801DCAD8` for Status; siblings
`FUN_801DCA94` / `FUN_801DCB1C`) draws only the label string at the
content origin `(WX, WY)` with staged text CLUT 7. Engine primitive:
`engine-render::tab_banner_draws`.

## Status satellite windows

The three left-column windows of the Status screen, each a content-only
renderer inside the standard gold frame:

**Party list (id 26, `FUN_801D2094`)**: one row per roster slot at pitch
`0x0e`; name string at `(WX+6, Yrow)` from record `+0x2A7`, always CLUT 7
(no selected-row ink change). The highlighted row draws the 16x16
**pointing-hand cursor** at `(WX-0xc, Yrow)` via the animated-cursor
primitive `FUN_8002b994` - sprite-table kind 0 of the 4-record
0x18-stride table at `0x80073d18` (`[frames u8, clut u8, period i16,
last_xy 2×i16, frame UVs 4 bytes each]`; hand = 1 frame, UV `(152,64)`,
CLUT row 7, plus a 0..2-px idle bob from the offset table at
`0x80073d78`).

**"Condition" pager (id 27, `FUN_801D30A4`)**: the folded submenu id
picks the label ("Condition" for the status page; Skills / Magic / Moves
strings for ids 1..3) drawn at `(WX+6, WY)` CLUT 7, flanked by the solid
**triangle sprites**: `FUN_8002b994` kind 2 (left, UV `(168,8)`) at
`(WX-0x10, WY-2)` and kind 3 (right, UV `(168,40)`) at `(WX+0x3A,
WY-2)`, both 16x16 CLUT row 7.

**Summary (id 30, `FUN_801D31EC`)**: name at `(WX, WY)`; "LV" icon (ICO
`0x0a`) at `(WX+0x1c, WY+0xf)` with the 2-digit level field (record
`+0x130`) at `(WX+0x2c, WY+0xd)`; "ATR:" at `(WX, WY+0x1a)` followed by
the **element icon** drawn through the per-character 2-byte string at
menu-overlay VA `0x801E4720 + char*4` (`0xCE 0x1D/0x1F/0x1E`). The
string primitive's `0xCE` token resolves the argument through the
glyph-metadata aux table at `0x80074050` (4-byte records `[i16 ico_code,
u8 x_advance, i8 dy]`): records `0x1D/0x1F/0x1E` → ICO codes
`0x94/0x96/0x95` (Vahn/Noa/Gala), 28x12 sprites at sheet V 208 with the
**alternate CLUT encoding** (record CLUT byte bit `0x40`: CLUT at VRAM
`(896 + (b&3)*16, 500)`). The pixels live in the system-UI **extension
strip** TIM at `PROT.DAT[0x10178]` (256x32 4bpp, VRAM `(896,448)` =
sheet V 192..224); the row-500 palettes are the CLUT block of the
sibling TIM at `PROT.DAT[0x10028]` (rows 498/499/501 come from
`0x10178`/`0x100D0`/`0xFF80`). If the character carries a Seru, a
second block draws the class icon (ICO `0x45`) + Seru name at `WY+0x2f`
and its level at `WY+0x3c`.

## Plumbing

| Item | Value | Instr |
|---|---|---|
| Menu / party base `s2` | `0x80084140` | `801d33dc` |
| Highlighted record index `uVar1` | `*(u8*)(0x80084598 + (DAT_801e46c4 & 0xfff))` | `801d33f0`, `801d3424` |
| Submenu id | `DAT_801e46c0 & 0xfff`, folded `if id>=6 { id-=5 }` -> 0..5 | `801d33f4`, `801d3460` |
| Record stride | `uVar1 * 0x414` | `801d3440` |
| Live record base | `0x80084708 + uVar1*0x414` | `801d3454` |
| Window X `s7` | `*(i16*)(a0+0xa)` | `801d3494` |
| Window Y `s8` | `*(i16*)(a0+0xc)` | `801d3490` |

`s8` is a **running Y cursor** advanced down the panel: `+0x13` after the
header, `+0x2f` / `+0x2b` / `+0x38` between the status sub-blocks. `s7` is
reloaded from `a0+0xa` at each block and set to `WX+0x10` for the list pages.

The record layout (`0x80084708 + n*0x414`, stride `0x414`) is the live party
record array seeded by the new-game template; see
[`new-game-table.md`](../formats/new-game-table.md) and
[`spell-table.md`](../formats/spell-table.md).

## Submenu dispatch

The folded submenu id (0..5) selects the page. Raw ids 6..10 alias onto 1..5
(a second bank onto the same five layouts).

| id | page |
|---|---|
| 0 or 5 | full status page (name + LV + HP/MP + 6 stats + 7 equip slots + XP) |
| 1 | skills / accessory-passive list |
| 2 | magic list |
| 3 | moves / arts list |
| 4 | header only (equipment edited elsewhere) |

The id is a branch selector, not a table index. The per-page string labels and
data all index by the **character** `uVar1`, e.g. the class/Seru name via
`*(u32*)(0x801e46d4 + uVar1*4)`.

## Header row (always drawn)

`Yrun = WY`. Offsets are relative to `(WX, WY)`.

| element | prim | X | Y | source |
|---|---|---|---|---|
| character name | STR | +8 | +0 | record `+0x2A7` |
| "LV" label | ICO | +0x50 | +2 | icon code `0x0a` |
| LV value | NUM | +0x60 | +0 | record `+0x130`, 2 digits |
| class/Seru label | ICO | +0x8a | +0 | icon code `0x45` (conditional) |
| class/Seru name | STR | +0x96 | +0 | `*(u32*)(0x801e46d4 + uVar1*4)` |

After the header, `s8 += 0x13`. Instr `801d3478`..`801d35c8`.

## Status page (submenu 0 or 5)

Header `Yrun = WY+0x13`. Two stat rows (HP then MP), then a gauge, then a 3x2
derived-stat grid, then a 7-slot equipment grid, then Experience / Next Level.

**HP row** (`Y = WY+0x13`) / **MP row** (`Y = WY+0x20`): current at `X+0x30`,
max at `X+0x58`, base at `X+0x84` (all 4-digit NUM); separators (UI-glyph) at
`X+0x50`, `X+0x7c`, `X+0xa4`. HP triplet = record `+0x106 / +0x104 / +0x11c`;
MP triplet = record `+0x10a / +0x108 / +0x11e`. Number colour comes from
`FUN_800349ec` (HP) / `FUN_80035ea8` (MP), not the string CLUT. Instr
`801d35e8`..`801d374c`. Ink (golden-capture pixel-pinned): the `/` and the
current/max values in the CLUT-7 text white `(206,206,206)`; the whole
parenthesised base group - `(`, value, `)` - in the separator **teal**
`(66,222,222)`. The 4-digit fields end flush against their separators
(`180/ 180 ( 180)`).

**AP gauge**: bar widget at `(X+0x40, WY+0x2d)`, value record `+0x10e`.
`FUN_80034b6c(0x31)` stages the widget kind into `gp+0x14c`; the widget
dispatcher `FUN_8002c69c(x, y, 1, value)` sees kind `0x31` and first calls
the gauge-content renderer **`FUN_8002c0b0(x, y, value)`**, then falls
through to the generic table-driven frame path. Then `s8 += 0x2f`.

The frame is four 1:1 sprites from the system-UI sheet (CLUT row 4; every
rect pixel-verified against the golden `menu_status_town` capture): the
left arrow cap with the red "AP" chip `(128,64,24,16)` at the anchor, the
trough body `(128,80,56,16)` at `+0x18`, the bordered value box
`(176,64,16,16)` (= ICO record `0x69`, baked `dx = 0x50`) and the pointed
right end `(184,80,8,16)` (= ICO record `0x6A`, `dx = 0x60`).

`FUN_8002c0b0` draws the gauge content (see `ghidra/scripts/funcs/8002c0b0.txt`):

- **Meter fill** (`value > 0`): two untextured gouraud quads spanning
  `x+0x1B .. x+0x1B + value/2` (50 px at the 100-AP cap; `value > 100`
  clamps the width to `0xFF` for the wider field-HUD variants), 6 rows at
  `y+5..y+10`: dark-red `rgb(0x80,0x20,0x10)` fading to gold
  `rgb(0xC0,0xA0,0x40)` at the shared middle edge and back - a vertical
  diamond gradient. The fill prims are prepended into the same OT bucket
  as the frame, so they render **on top of** the trough.
- **Value**: `== 100` draws the dedicated "100" glyph, ICO code `0x6B`
  (`(64,136,16,6)`, CLUT row 1) at `x+0x50`; `< 100` draws the tens digit
  ICO `0x6C+tens` at `x+0x50` (only when non-zero) and the ones digit ICO
  `0x6C+ones` at `x+0x56`. The digit records are ten 6x6 cells at
  `(64 + 6*digit, 128)`, CLUT row 4; all at `y+5`.

**Derived-stat grid** (`FUN_801cf650` computes the values first). 3 rows at
`WY+0x42 / +0x4f / +0x5c`, two columns. Left column: label `X+0`, live value
`X+0x28`, `(` at `X+0x40`, growth value `X+0x48`, `)` at `X+0x60`. Right
column: label `X+0x74`, live value `X+0x9c`, `(` at `X+0xb4`, growth value
`X+0xbc`, `)` at `X+0xd4`. Live values (3-digit fields) clamp at 999 and
come from `DAT_801ef088..09c` in text white; growth values from record
`+0x122..+0x12c`, parens + growth in the separator teal. Then
`s8 += 0x2b`. Instr `801d3780`..`801d3b48`.

**Equipment grid** (7 slots): icon + item name. Icon codes from the fixed
array `DAT_801e43f4..4400` = `[0x24, 0x22, 0x23, 0x25, 0x46, 0x46, 0x46]`
(u16 entries); item name via the item-name table
`*(u32*)(0x8007436c + id*0xc)` where `id = *(u8*)(record + 0x196 + slot_off)`.
Slots 0..3 stack at `X+0/+0x10` on rows `WY+0x6d / +0x7a / +0x87 / +0x94`;
slots 4..6 sit in a right column at `X+0x6a/+0x7a` on rows `WY+0x7a / +0x87 /
+0x94`. Then `s8 += 0x38`. Instr `801d3b4c`..`801d3dd8`. Item ids resolve
through [`item-table.md`](../formats/item-table.md). The codes resolve
through the `0x800732a4` UV/CLUT table (below) to 12x12 pictograms in the
system-UI sheet, all CLUT row 8 (gold ramp, pixel-verified vs the golden
capture): weapon fist `(244,36)`, helmet `(244,24)`, body armor `(232,36)`,
boot `(232,48)`, and the shared Goods ring `(0,128)` for slots 4..6. The
icon per slot position is fixed - retail draws all seven pictograms whether
or not the slot is equipped.

**Experience / Next Level** (`Yrun = WY+0xa5`): "Experience" STR at `X+0x18`,
value (8-digit NUM) at `X+0x78` from record `+0x0`; "Next Level" STR at
`X+0x18, WY+0xb2`, threshold at `X+0x78` from record `+0x4`. Instr
`801d3ddc`..`801d3e60`.

## Magic list (submenu 2)

`s7 = WX+0x10`. Header (CLUT 6): "Magic" at `(X, WY+0x13)`, "MP Used" at
`(X+0x60, WY+0x13)`. Rows start `WY+0x28`, pitch `0x0d`, up to 7 visible with a
scroll offset `_DAT_8007bb90`; count gate `*(u8*)(record+0x13c)`. Per spell
(id `record+0x13d`, level `record+0x161`): name via the spell-name table
`*(u8*)(record+0x13d)*0xc + 0x800754d0`; level digit at `X+0x78`; MP cost
(3-digit) at `X+0xa8` via `FUN_80035394`. Selected row draws a cursor and a
CLUT-6 preview line; non-selected rows use CLUT 0. Empty: "-No magic skills-"
at `(X, WY+0x50)`. Instr `801d4098`..`801d43c4`. See
[`spell-table.md`](../formats/spell-table.md).

## Moves list (submenu 3)

`s7 = WX+0x10`. Header (CLUT 9): "Moves" at `(X, WY+0x13)`, "AP Used" at
`(X+0x60, WY+0x13)`. Arts match the arts table `DAT_80075ec4` (stride `0x14`);
up to 7 rows, pitch `0x0d`, scroll `_DAT_8007bb90`. Per art: name (CLUT 7) at
`X+0x10`, AP cost (3-digit) at `X+0x82` (halved when record `+0x800` bit `0x800`
is set). The selected row also draws "Command:" (CLUT 1) plus the command
**direction arrows** via `FUN_8003c310`, stepping X by `0xc` per input, and a
description glyph. Empty: "You have not learned any moves." Instr
`801d43c4`..`801d477c`. See [`art-data.md`](../formats/art-data.md).

## Skills page (submenu 1)

`s7 = WX+0x10`. Loops accessory equip slots 5..7; a slot draws only when its
resolved passive index `< 0x40`. Per slot: label icon (CLUT 6) at `(X+0x10,
Yrun)`, item name at `X+0x20`, and two passive-effect glyphs from the
accessory-passive table `0x8007625c` at `(X+0x30, Yrun+0xe)` (CLUT 4) and
`(X+0x38, Yrun+0x1c)` (CLUT 7). Per-row pitch `0x3b`. Empty: "You do not have
any skills." Instr `801d3e64`..`801d4098`. See
[`accessory-passive-table.md`](../formats/accessory-passive-table.md).

## Equip screen

The Equip screen composes four descriptor-table windows (draw order: tab 2,
party 21, item-list 23, main 22 - the main window's opaque interior occludes
the item-list window's lower span). Content renderers, all in the menu
overlay:

**Tab (id 2)** - `FUN_801DCA94` stages CLUT 7 and draws the "Equip" STR at
the tab window's content origin; the carved banner behind it is caller art
(see `ghidra/scripts/funcs/overlay_menu_801dca94.txt`).

**Party window (id 21, rect `(14,42,80,38)`)** - `FUN_801D2094` (shared with
the status screen's id-26 party list; see
`ghidra/scripts/funcs/overlay_menu_801d2094.txt`). For each present party
member (count `DAT_80084594`, roster order bytes at `0x80084598`; only
roster slots `< 3` draw): the name STR (record `+0x2A7`) at `(X+6,
Y + 0xE*i)`, CLUT 7. The pointing-hand cursor (`FUN_8002B994`) draws at
`X-0xC` on the focused row, gated by the focus word `DAT_801E46C4`
(bit `0x4000` hides, `0x2000` selects the blink variant, low 12 bits =
row).

**Main window (id 22, rect `(14,96,292,108)`)** - `FUN_801D21C0` (see
`ghidra/scripts/funcs/overlay_menu_801d21c0.txt`). Early-outs unless the
shown character's roster byte is `< 3`. First pass:

- "Best Equipment" STR at `(X+0x10, Y)` - cursor row 0 of the window's
  cursor space (`DAT_801E46C0`), hand at `(X, Y)`.
- 7 slot rows at `Y + 0xE*(i+1)`: hand cursor at `X`, 12x12 slot pictogram
  (ICO `FUN_8002C488`, code `DAT_801E43F4[i]` - the same fixed 7-code
  array as the status equipment grid: weapon fist / helmet / armor / boot /
  3x Goods ring) at `X+0x10`, the equipped item's name STR at `X+0x20`.
  Item id: row 0 reads `record[0x196 + *(i16*)(DAT_8007B42C + char*2)]`
  (per-character weapon-slot offset), rows 1..6 read
  `record[0x196 + DAT_801E43E8[row]]`; names via the item-name table
  `0x8007436C + id*0xC`.

Second pass only when the submenu id is settled on the equip screen
(`DAT_801E46A4 == DAT_801E46A8 == 0x13`) and no transition is pending
(`_DAT_8007BB80 == 0`):

- **Cursor row 0 ("Best Equipment")**: for each armament row 0..3 whose
  best-candidate id (`DAT_801EF0C0[i]`) differs from the equipped id: a
  change-arrow glyph `FUN_8003C310(2)` at `X+0x8E` (CLUT 0), then - for
  class-1 (equipment) items - a weapon-class pictogram at `X+0xA8` (class
  from the equip-stat record `+7` bits `0x60`, remapped `{2->2, 1->1,
  0->3}` into `DAT_801E43F4`) with the candidate name at `X+0xB8`
  (non-equipment names land at `X+0xA8`). Below, the **stat-compare
  block**: 3 rows at `Y+0x48/+0x55/+0x62`; 3-char stat label STR
  (`0x801CE9A0/A4/A8`) at `X+0xA0`, current value (3-digit NUM, 999-clamp,
  `DAT_801EF08C/90/94`) at `X+0xC8`; when the preview value
  (`DAT_801EF0AC/B0/B4`) differs, an up/down arrow `FUN_8003C1F8(4|5)` at
  `X+0xE4` (CLUT 6 raised / CLUT 1 lowered) and the preview value at
  `X+0xF0`.
- **Cursor row 1..7**: the selected slot's equipped item id lands in
  `DAT_801E46B0` and, when non-zero, an item info panel draws at
  `(X+0x94, Y+0xC)`: `FUN_801D0F1C` (description text) over two
  `0x90 x 0x28` shade boxes (`FUN_8002C69C`) at `Y+0xC` and `Y+0x44`.

**Item-list window (id 23, rect `(174,22,132,182)`)** is renderer-less in
the descriptor table (frame-only container); its picker content is drawn by
the equip flow outside these window renderers.

Engine port: `engine-render::equip_screen_draws_for` (window contents at
the offsets above; the candidate list fills the id-23 rect at the shared
`0xD` list pitch) + `equip_screen_sprites_for` (pictogram column + hand
cursors from the system-UI atlas), pens disc-parsed from the descriptor
table. The engine's 8th slot row (its equip-array over-model) stays
navigable but icon-less; the stat-compare block previews the hovered
candidate rather than the best-equipment pick.

## Scroll widgets (submenu 2 or 3)

Up arrow (icon `0x67`) when `_DAT_8007bb90 > 0` and down arrow (icon `0x68`)
when more rows follow, both at `X = WX + (a0+0xe >> 1) - 4`. Scrollbar thumb
(bar primitive) at `(WX, WY + (a0+0x10) - 0x28)`, length from `a0+0xe`,
`FUN_80034b6c(3)`. Instr `801d477c`..`801d4838`.

## Options screen

Three functions in the menu overlay (PROT 0899, base `0x801CE818`):

- **Row renderer** `FUN_801D2910`, called by the window-id-48 content
  renderer `FUN_801DCEF0` (a thin `FUN_801d2910(win, 0, 9)` wrapper) - see
  `ghidra/scripts/funcs/overlay_menu_801d2910.txt`. Per display row it
  draws the cursor arrow at content `x-10`, the label string at `x+8` and
  (on value rows) the value string at `x+140`, then advances y by the
  row's layout pitch.
- **Input SM** `FUN_801DA9F8` (browse cursor `DAT_801E46C0`, low 12 bits =
  row, bit `0x1000` = editing, bit `0x4000` = cursor hidden).
- **Value-popup renderer** `FUN_801D2B44` (window id 47).

Three data tables drive the rows:

| VA | contents |
|---|---|
| `0x801E4404` | display layout: 10 × `[u16 row_id, u16 advance]` - row ids `0,1,2,3,6,4,7,9,8,10`, advance 14 px (20 px on the two group-separator rows, Battle Command + Field HP Display) |
| `0x801E44B8` | row descriptors: 8-byte nodes `[config_word_ptr: u32][value_count: u8][label_ink: u8][row_id: u8][string_index: u8]`, walked as a linked list keyed on `row_id` |
| `0x801E442C` | shared string pointer table; a row's value string = `strings[string_index + value + 1]` |

The row set (label / choices / config word - the words live in the saved
`0x800845xx/0x800846xx` config block):

| row | choices | config word |
|---|---|---|
| Battle Camera | Close / Normal / Far | `0x800846C0` |
| Battle Select Attack | Select / Automatic / Command | `0x800846C4` |
| Battle Command | Directional Buttons / ✕-glyph " button" | `0x800846C8` |
| Field Move | Walk / Run | `0x800846CC` |
| Field HP Display | Immediate / Gradual / Display Off | `0x800845C4` |
| Sound | Stereo / Monaural | `0x800846BC` |
| Dual Shock (header, no value) | - | - |
| "  Battles" | Vibration On / Off | `0x800845C8` |
| "  Events" | Vibration On / Off | `0x800845A8` |
| "  Encounters" | Vibration On / Off | `0x800845CC` |

Inks (staged via `DAT_8007B454`): labels ink 7 (white), values ink 6
(gold), the indented Dual Shock sub-row labels ink 5 (teal) - the per-row
label ink is the descriptor node's `+5` byte. While the value popup is
open every non-cursor row drops to ink 0, except a header row above the
cursor which keeps its ink. A hidden row exists in the descriptor list
but not in the layout table: "Battle Voices" (Voices On / Off,
`0x800845AC`) - present strings, never displayed in the US build.

Interaction (`FUN_801DA9F8`): Up/Down move the browse cursor, skipping
valueless rows (the SM re-navigates off the header); Cross opens the
value popup seeded with the current value; Cross inside commits the popup
cursor **directly into the config word** (committing "Events" to
Vibration Off also zeroes the live rumble state `0x8007B92C/0x8007B930`);
Circle backs out of the popup, and out of the screen - there is no
revert, edits are already live. The popup is window descriptor id 47: its
x/w `(170, 128)` are static, y/h are stamped per open
(`y = id-48 y + 0x16 + Σ advances above the cursor row`,
`h = choices × 13 - 4`, flipped up by `choices × 13 + 0x1C` when the
bottom would pass y = `0xB0`). `FUN_801D2B44` lists the choices at a
13-px pitch, text inset `+0x14`, cursor at the content origin.

Engine port: `engine-core::options` (`OPTIONS_DISPLAY_ROWS`,
`OptionsSession` Browsing→Editing SM, `options_popup_content_rect`) +
`engine-render::options_draws_for`; the Sound row drives the audio
mixer's monaural downmix (`engine-audio AudioOut::set_mono`), the other
settings persist in the engine's options config file.

## Draw primitives + CLUT staging

Three shared primitives render everything:

| tag | function | signature | notes |
|---|---|---|---|
| STR | `FUN_80036888` | `(str, count, 0, x, y)` | proportional string; MES control tokens: `0x7c` = line break (`y += 0xe`, x resets), `0xcf b` = set text CLUT inline, `0xce b` = inline icon/number via the `0x80074050` aux record `b` (`[i16 ico_code, u8 x_advance, i8 dy]`; a zero code draws a number variable instead) |
| ICO | `FUN_8002c488` | `(x, y, code)` | one UI-icon sprite; 12-byte-stride table at `0x800732a4`: `+3` CLUT byte (`&0x7f` → row at VRAM y 511; bit `0x40` = alternate encoding `(896+(b&3)*16, 0x1F2+((b&0x3f)>>2))`; bit `0x80` = blend), `+4..+7` = U/V/W/H, `+8/+0xa` = baked dx/dy (codes `0x86..0x8a`, texpage from `0x80073db8`) |
| NUM | `FUN_80034b78` | `(value, digits, x, y)` | decimal digits vs the powers-of-ten table at `0x80073dcc`; one glyph cell per digit at a fixed 8-px pitch, right-aligned in the `digits`-wide field (leading cells blank) |
| CUR | `FUN_8002b994` | `(kind, mode, x, y)` | 16x16 animated cursor sprite; 4-record 0x18-stride table at `0x80073d18` (kind 0 = pointing hand `(152,64)`, 1 = 2-frame `(224/240,64)`, 2 = left triangle `(168,8)`, 3 = right triangle `(168,40)`; all CLUT row 7). Mode 1 animates (idle bob from the `0x80073d78` offset table), 0 draws static |

The palette-staging global is **`DAT_8007b454`** (`0x80080000 - 0x4bac`);
the in-primitive CLUT halfword is `index + 0x7f86`. It is **read only by the
string primitive** `FUN_80036888` (at `80036b74`). Icon and number primitives
carry their own CLUT (icon from the `0x800732a4` table, number from
`gp+0x13c`), so a `DAT_8007b454` write immediately before an ICO/NUM draw is
inert for that draw and is really staging the palette for the next string.
Distinct values seen: 7 (default text - reads back as RGB `(206,206,206)`
in the framebuffer), 5 (status separators - the teal `(66,222,222)`
parenthesised-value ink), 6 (magic header + skill labels), 9 (moves
header), 4 (skill passives), 1 (command label + arrows), 0 (non-selected
magic rows).

## Record fields consumed

Field offsets into the `0x414`-stride live record emitted by this panel:

| offset | field |
|---|---|
| `+0x0` | cumulative experience (8-digit) |
| `+0x4` | next-level threshold |
| `+0x104 / +0x106 / +0x11c` | HP max / current / base |
| `+0x108 / +0x10a / +0x11e` | MP max / current / base |
| `+0x10e` | AP-gauge value (the persistent out-of-battle AP; 0 on a fresh party - the new-game template zeroes it) |
| `+0x122..+0x12c` | six growth-stat values |
| `+0x130` | displayed level (matches the starting-level randomizer target) |
| `+0x13c / +0x13d / +0x161` | spell count / spell ids / spell levels |
| `+0x196..` | equipped item ids |
| `+0x2A7` | name string |

External tables read: item names `0x8007436c`, spell names `0x800754d0`,
equipment stats `0x80074f68`, item effects `0x800752c0`, accessory passives
`0x8007625c`, arts `0x80075ec4`. These are the same records documented under
the per-format pages.

## Engine port

The clean-room engine parses the window-descriptor table from the user's
disc at boot (`legaia_asset::menu_windows`; the play-window falls back to a
pinned mirror of the same rects) and frames each screen's window set with
the reusable 9-slice primitive `engine-render::menu_window_chrome_draws_for`
(the caller-drawn window frame), placed on the shared 320x240 boot-UI stage
via `engine-render::scale_stage_text_draws`. The frame chrome and the navy
**filigree interior** both come from the system-UI TIM at `PROT.DAT[0x018E0]`
CLUT row 2 (the same sheet as the save-screen chrome and the UI-icon atlas):
the gold-bronze 9-slice tiles plus the marbled-blue interior region
(`OVERLAY_SYSTEM_UI_PANEL_INTERIOR`, `(128,0,32,29)`). The pause menu tiles
the raw interior tile in **both axes** (`SaveMenuAtlasRects::panel_filigree`,
an un-gradient-baked copy of that region) under a flat darkening tint - retail
modulates it with a per-window gouraud gradient; the flat multiply is a close,
non-streaking approximation. (The save/load screen keeps the gradient-baked
`panel_interior` variant stretched to its panel height; only the pause-menu
windows pass `tile_filigree = true` to `nine_slice_panel_into`.) The status
main panel renders
through `engine-render::status_screen_draws_for` at the byte-pinned offsets
above, hung off the id-28 content origin; the satellite windows through
`status_satellite_draws_for`; the top-level list / money box / party panel
through `field_menu_draws_for` + `field_menu_info_draws_for`. The
HP/MP/level/equipment values come from the typed character record in
`legaia_save` (derived-stat grid = live `+0x110` window + growth
`+0x122..+0x12D` window pairs). The **LV / HP / MP labels, the AP gauge and
the equipment pictograms are ported UI-icon sprites** - their source rects
are the `0x800732a4` icon-table records verbatim (labels = codes
`0x0A/0x07/0x08` at `(192/208/224, 86, 16, 10)` CLUT row 1; pictograms =
the `DAT_801e43f4` slot codes, CLUT row 8; gauge pieces + red digit strip,
CLUT row 4 - every rect and placement pixel-verified against the golden
`menu_status_town` capture), staged into the atlas and emitted by
`engine-render::status_icon_sprites_for` at the pinned status offsets while
`status_screen_draws_for(.., label_icons = true)` suppresses the ASCII
stand-ins (the AP text readout and empty-slot equipment text included; an
occupied slot's item name lands at the retail `+0x10` name offset).
The AP gauge's **meter fill** and value digits follow the traced
`FUN_8002c0b0` layout (gradient fill = a procedurally-baked column of the
gouraud endpoint colours stretched to `value/2` px; per-row linear
interpolation approximates the GPU DDA until an AP>0 retail capture pins
the sub-pixel truncation - both golden captures hold AP 0); the gauge
value feeds from the persistent record `+0x10E` AP, not the battle
gauge. The satellite windows are sprite-ported at the traced offsets:
the party-list pointing hand + Condition-pager triangles
(`status_satellite_icon_sprites_for`, frame-0 statics of the
`0x80073d18` cursor table), the summary LV label and the per-character
ATR element icons (extension-strip TIM `PROT.DAT[0x10178]` decoded with
the `PROT.DAT[0x10028]` row-500 palettes). The title tabs wear the
carved plaque via the shared `engine-render::tab_banner_draws` (cap /
tiled body / cap, CLUT row 12) with the label in CLUT-7 white; tab
windows draw no 9-slice frame. Number fields lay out on the retail
fixed 8-px digit cells (`num_field_draws`), and the parenthesised
base/growth groups use the retail teal ink. The Equip screen renders its
retail four-window set (tab 2 + party 21 + item-list 23 + main 22)
through `equip_screen_draws_for` + `equip_screen_sprites_for` at the
traced `FUN_801D21C0` / `FUN_801D2094` offsets (see
[Equip screen](#equip-screen)). Still engine-styled: the top-level row
content (renderer `FUN_801CFD68` untraced) and the Items / Spells /
Arts / Equip-picker screens (their content layouts do not fill the
pinned windows yet, so they keep a generic frame).
