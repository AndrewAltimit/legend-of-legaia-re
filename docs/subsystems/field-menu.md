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
- [Top-level pause menu](#top-level-pause-menu) · [Equip screen](#equip-screen) · [Options screen](#options-screen)
- [Submenu state machines](#submenu-state-machines) · [Items screen](#items-screen) · [Magic screen](#magic-screen)
- [Inn stay](#inn-stay-there-is-no-inn-screen)
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
live window carries its descriptor id). The Status / Equip / Options sets
come from the six catalogued mednafen states; the Items / Magic sets from
PCSX-Redux captures pad-walked to each screen
(`scripts/pcsx-redux/autorun_menu_screen_dump.lua` over the
`sol_to_karisto_worldmap` scenario state - SELECT opens the menu, the
walk confirms into each command, and the probe dumps framebuffer + RAM
at parked checkpoints):

| screen | windows (draw order) |
|---|---|
| top-level pause menu | 50 command list `(24,24,104,94)` -> `FUN_801CFD68`; 49 money/play-time box `(24,178,104,24)` -> `FUN_801D0148`; 51 right party panel `(144,24,152,180)` -> `FUN_801D030C` |
| Status | tab 3 -> `FUN_801DCAD8`; 26 party list `(14,38,60,38)` -> `FUN_801D2094`; 27 "Condition" pager `(14,92,60,10)` -> `FUN_801D30A4`; 30 summary `(14,134,60,70)` -> `FUN_801D31EC`; 28 **main panel** `(90,16,218,188)` -> `FUN_801D33D8` |
| Equip | tab 2 -> `FUN_801DCA94`; 21 party `(14,42,80,38)` -> `FUN_801D2094`; 23 item list `(174,22,132,182)` (renderer-less container; its lower span is occluded by 22); 22 main `(14,96,292,108)` -> `FUN_801D21C0` |
| Options | tab 4 -> `FUN_801DCB1C`; 48 settings `(24,40,256,148)` -> `FUN_801DCEF0`; 47 value popup `(170, *, 128, *)` -> `FUN_801D2B44` (y/h stamped per open - see [Options screen](#options-screen)) |
| Items | tab 0 -> `FUN_801DCA0C`; 13 command `(32,44,80,38)` -> `FUN_801D0D18`; 15 item list `(174,22,132,182)` (renderer-less); 17 info `(14,108,144,40)` -> `FUN_801DCB60` - see [Items screen](#items-screen) |
| Magic | tab 1 -> `FUN_801DCA50`; 18 spell list `(174,22,132,182)` (renderer-less); 19 caster `(14,40,144,96)` -> `FUN_801D2C98`; 20 spell info `(14,152,144,52)` -> `FUN_801D2E74` - see [Magic screen](#magic-screen) |

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

All five tab content renderers - `FUN_801DCA0C` (Items), `FUN_801DCA50`
(Magic), `FUN_801DCA94` (Equip), `FUN_801DCAD8` (Status), `FUN_801DCB1C`
(Options) - are the same two-instruction shape: stage text CLUT 7, then
draw the label string at the content origin `(WX, WY)` with no offset of
its own. The tab's identity is entirely the string operand, which is why
one engine function serves all five.

Engine primitives: `engine-ui::tab_banner_draws` composes the plaque and
`engine-ui::tab_label_draws` puts the word on it. Both take the same
`pen`, so framing and labelling a tab need one pair of coordinates.

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

## Top-level pause menu

Three descriptor-table windows (see the window table above): 50 command
list, 49 money/play-time box, 51 party info panel. Sources
`ghidra/scripts/funcs/overlay_menu_801cfd68.txt` / `_801d0148.txt` /
`_801d030c.txt`.

**Command list (id 50, `FUN_801CFD68`)**: seven rows at `(WX+0x14,
WY + n*0xe)`, in draw order **Items, Magic, Equip, Status, Options,
Load, Save** - all staged CLUT 7. The selected row draws the
pointing-hand cursor at `(WX, row_y)` via the animated-cursor primitive
`FUN_8002b994` (skipped entirely when state word `DAT_801e46bc` bit
`0x4000` is set; bit `0x2000` selects the dimmed cursor variant). Rows
gray to CLUT 0 when blocked: Load when the dialog-context pointer
`DAT_8007b450` targets an `0x0D` byte, Save when the save-enabled flag
`DAT_8007b6a8` is clear.

The seven labels are **NUL-terminated C strings** in the menu overlay's
leading rodata string pool (PROT 0899, base `0x801CE818`): `@Items` at
`0x801CE9D0`, then `@Magic` / `@Equip` / `@Status` / `@Options` / `@Load` /
`@Save` in order. `FUN_801CFD68` loads each by a `lui`+`addiu` pair
(`addiu a0, a0, -0x1630` at base+0x1560 = `0x801CE9D0`), so the pointer
targets the leading `0x40` (`@`) marker byte the string primitive
`FUN_80036888` consumes; the visible label is the tail. The same pool
(`0x801CE81C..0x801CEC78`) holds the options-screen choices, the derived
stat labels (`ATK`/`UDF`/`LDF`/`SPD`/`INT`/`AGL`, `Experience`,
`Next Level`), and the shop / equip / status command strings
(`@Best Equipment`, `@Condition`, `@Moves`, `@MP Used`, `@AP Used`,
`@Command:`, `@Buy`/`@Sell`/`@Quit`, ...). The **battle** overlay
(PROT 0898, same base) keeps its own command / result pool at
`0x801F4B98..0x801F4D2A`: `Spirit` / `Defense` (the Defend command) /
`Escape` / `Begin` plus the victory / defeat / escape / ambush messages -
the `Attack` / `Arts` / `Magic` / `Item` command-ring labels are UI-icon
sprites, not text. These pools are the coordinate windows the translation
pipeline's `ui_menu` section patches same-size in place
(`legaia_rando::translation::ui`; see
[`translation.md`](../tooling/translation.md)).

**Money / play-time box (id 49, `FUN_801D0148`)**: money pictogram (ICO
`0x62`) at `(WX, WY+2)` with the amount as an 8-digit field
(`FUN_80034b78`) at `(WX+0x28, WY)`. When the casino-coin flag
(`FUN_8003ce64(8)`) is set, a coin row follows: ICO `0x66` at
`(WX, y+0x10)`, coin bank `0x800845A4` 8-wide at `(WX+0x28, y+0xe)`.
The play-time row draws ICO `0x63` at `(WX, y+0x10)` and the clock from
the 60 Hz tick counter `0x80084570`: hours 3-wide (clamped 99, then
minutes/seconds pin 59) at `+0x20`, colon glyphs (`FUN_8003c1f8` code 9)
at `+0x38`/`+0x50`, zero-padded 2-wide minutes/seconds (`FUN_80034e4c`)
at `+0x40`/`+0x58`. When the coin row shows, the live window grows past
its descriptor rect - the Items/Magic-era capture holds id 49 at
`(24,166,104,38)` against the table's `(24,178,104,24)`.

**Party info panel (id 51, `FUN_801D030C`)**: one block per roster
member (ids `u8[3]` at `0x80084598`, count `0x80084594`; live record
`0x80084708 + id*0x414`) at stride `0x3e`. Per block: name (`+0x2A7`)
at `(WX+0x10, Y)`; LV icon (ICO `0x0a`) at `(WX+0x70, Y+2)` with the
2-digit level (`+0x130`) at `WX+0x80`; HP label (ICO `0x3f` - the same
`(208,86,16,10)` sheet rect as status code `0x07`) at `(WX+0x28,
Y+0x11)` with current/max 4-digit fields at `WX+0x38`/`WX+0x60` and the
slash at `WX+0x58` on row `Y+0xf`; MP likewise (ICO `0x40`) on rows
`Y+0x1e`/`Y+0x1c`; and the kind-`0x31` AP gauge widget at `(WX+0x28,
Y+0x29)` fed from the persistent AP `+0x10E`. HP / MP value ink comes
from per-member health-tier color fns (`FUN_800349EC` /
`FUN_80035EA8`); the full-health tier is the plain CLUT-7 white.

Engine port: `engine-ui::field_menu_draws_for` +
`field_menu_info_draws_for` (text) and `field_menu_icon_sprites_for`
(hand cursor, money/time pictograms, LV/HP/MP labels, per-member AP
gauges via the shared `ap_gauge_sprites` widget). The engine shows the
coin row only when the casino bank is nonzero; the health-tier ink
thresholds stay untraced.

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

Engine port: `engine-ui::equip_screen_draws_for` (window contents at
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
`engine-ui::options_draws_for`; the Sound row drives the audio
mixer's monaural downmix (`engine-audio AudioOut::set_mono`), the other
settings persist in the engine's options config file.

### Dev-menu EVENT FLAG editor (debug build only)

The retail *debug* build's developer menu lives in overlay 0897 (the
warp-applier + flag-editor toolset), not in the retail pause menu. Its
EVENT FLAG editor is a raw index/value poke tool - it is what produced the
capture states once misread as a mystery `_DAT_8007BA78` writer
(`docs/reference/re-settled-threads.md`). Two value-adjust kernels are
ported for completeness, faithful to the disassembly (`FUN_801dbd04` /
`FUN_801db8f4` in `ghidra/scripts/funcs/overlay_0897_*`):

- **Value step** (`FUN_801dbd04`): the edited flag index/value
  `DAT_801f2aa0` moves by Up/Down (fine `0x8`, coarse `0x80` while
  Triangle is held) and Left/Right (`±1`), then clamps to `[0, 0xFFF]`.
- **List cursor** (`FUN_801db8f4`): the flag-list row cursor
  `DAT_801f2e90` decrements, wrapping past the `'X'` (0x58) end sentinel
  in the stride-`0xA` table at `DAT_801f2e94` to the last real entry.

These read the **packed** pad words (`_DAT_8007bb84` edge, `_DAT_8007b850`
held) from `FUN_8001822C` - `0x10` Triangle … `0x1000` Up / `0x2000`
Right / `0x4000` Down / `0x8000` Left - not the raw-BIOS `PadButton`
layout. The two sibling worklist rows are decompiler fragments of the same
dispatcher, not standalone functions: `FUN_801d3444` is the PC-delta
entry-pointer advance (`addiu s8,s8,0x10; j …`), and `FUN_801d9bbc` is a
menu-row text emit left as an `engine-ui` draw seam. Engine port:
`engine-core::dev_menu` (`edit_flag_value`, `flag_list_prev`,
`EventFlagEditor`); no draw-list code, so the row render stays with the UI
crates.

## Submenu state machines

Every pause-menu screen's input handling is one per-submenu tick
function, dispatched from the master menu tick (inside `FUN_801DC1CC`,
menu overlay) through the function-pointer table at VA `0x801E4F40`,
indexed by the submenu word `DAT_801E46A4`. The master tick keeps a
requested/settled pair: a handler *requests* a switch by writing
`DAT_801E46A4`; the master tick compares it against the settled copy
`DAT_801E46A8` and, on a change, zeroes the shared per-submenu phase
word `DAT_801E46AC` before dispatching - every submenu entered starts at
phase 0. Items-screen slots of the table: 5 = command window
(`FUN_801D7C00`), 6 = the Use list (`FUN_801D7E50`), 7 = the Throw Out
list (`FUN_801D8734`), 9/0xA/0xB/0xC/0xD = the Use flow's per-effect-class
apply/target flows; 0xE/0xF = the Magic screen's caster/list handlers,
0x13 = the Equip screen (the `DAT_801E46A4 == 0x13` gate of
`FUN_801D21C0`).

The list windows themselves (descriptor kind 4) are paged by the
SCUS-resident **kind-4 list kernel `FUN_80032A44`** (below), which the
overlay talks to through a small global protocol: `_DAT_8007BB94` =
list mode/result (0 idle, 1 browsing, 2 = row confirmed, 3 =
cancelled, 4 = parked behind the command window), `_DAT_8007BB88` = the
selected row's payload (low 12 bits of the row entry - the bag-slot
index on the item lists), `_DAT_8007BB9C` = the selected row's class
nibble (`entry & 0xF000`, the screen-id key `FUN_80034250` dispatches
descriptions on), `_DAT_8007BB90` / `_DAT_8007BB98` / `_DAT_8007BBA0` =
scroll top / selected row / row count. The overlay SMs set mode 1 and
poll for 2/3, gating every phase step on the window-slide latch
`_DAT_8007BB80 == 0`.

### The kind-4 list kernel (SCUS `FUN_80032A44`)

Runs per frame per live list window whose id byte (live window `+0x1C`)
is in the allowlist at `0x80073E1C` (`02 03 22 07 08 09 0A 0E 0F 10 0B
05 19`, `0x23`-terminated). The list node (live window `+0x18`, built
by the allocator `FUN_80030104`: `count*2 + 0x2A` bytes) holds `+0x0`
scroll top, `+0x2` visible rows (`(content_h - 4) / 0xE`), `+0x4` row
count, `+0x6` selected row, and per-row u16 entries from `+0x28`. A row
entry packs `[class: high nibble][0x800 = disabled][0x400 = alt-ink]
[payload: low 12 bits]`; the overlay's content builder (`FUN_80030628`)
rebuilds the entries, the kernel only reads them.

**Navigation** (held pad `_DAT_8007BB84`, mode 1 only): Up (`0x1000`,
`80032ae8..80032c74`) steps the selection up, wrapping to the page's
last row at the page top (`80032b28`); Down (`0x4000`,
`80032b44..80032b84`) steps down, wrapping to the page top past the
page bottom or the last row; Left (`0x8000`, `80032b90`) pages up while
`top > 0`; Right (`0x2000`, `80032c1c`) pages down while `top + visible
< count`, clamping the selection to `count - 1`. Up/Down never scroll -
Left/Right are the only scroll, which is why the lists read as fixed
12-row pages. Confirm (edge mask `0x800846D0 & _DAT_8007B874`,
`80032ccc..`): a disabled row (`entry & 0x800`, `80032d04`) buzzes
(cue `0x23`); otherwise mode = 2 + cue `0x20` (`80032d34`). Cancel
(mask `0x800846D4`): cue `0x37`, mode = 3 (`80032dcc`). Move cues
(`0x21`) enqueue into the 4-slot UI ring at `0x8007B6D8`.

**PAGE header** (`80032e18..80032f20`, drawn while the count is
non-zero): the current page number is recovered by walking `visible`
-sized steps until the accumulator hits the scroll top. All glyphs are
`FUN_8002C488` UI-icon sprites from the `0x800732A4` table: the "PAGE"
small-cap tag is **ICO `0x76`** (atlas UV `(80,136)`, 24x8, CLUT byte
1 - the teal ink) at `(WX + W - 0x38, WY - 2)`; the gold page digits
are **ICO `0x7A + digit`** (6x8, UV `(64 + 6*digit, 144)`) - current
page at `WX + W - 0x20` (tens; ones at `+6`), the slash **ICO `0x79`**
(UV `(120,136)`) at `+0xD`, the page total at `+0x14`/`+0x1A`. The
digit-sprite path leading-zero-suppresses the tens cell.

**Rows** (`80033050..`): the row block vertically centres -
`row0_y = WY + (content_h - visible*0xE)/2 + 5`, pitch `0xE` (12 rows
at `WY + 0xC` for the item-list rect). Per-row draw switches on the
entry's class nibble: `0x1000` = bag row (name via the row-name
resolver `FUN_8002FF8C` at `WX+0xC`; bag count from `0x80085959 +
slot*2` as 8-px digit cells from `WX+0x6C` with skip-advance
leading-zero logic - a 1-digit count inks the `WX+0x74` cell, which is
the capture-pinned field); `0x6000` = bag row with an equip-slot
pictogram (icon id via equip record `+7` bits `0x60` through the
halfword table `0x80073A90`, name at `WX+0x1C`) - the Equip screen's
candidate list; `0x9000` = passive row (ICO `0x46` + name at
`WX+0x1C`); `0x7000` = ICO `0x21` + name (payload used as the id
directly); `0x3000`/`0xA000` = shop rows (name from item record `+4` at
`WX+0x18`, 5-digit price from record `+2` at `WX+0x80`; `0xA000`
stages ink 5); `0x8000` = fixed-advance name (monospace override byte
`0x80073F20`); `0x2000`/`0x5000`/`0x4000` = plain name rows.

**Row ink** (`8003312c..80033154` and per-class clones): each row
stages ink 7 (white), dropping to 0 (grey) when the row's `0x800`
disabled bit is set and to 1 when `0x400` is set - **unless the list is
parked** (mode 4), which keeps every row white. The pad-walked captures
show the whole Use-list page grey once the hand enters it, so the
overlay's row rebuild is setting `0x800` across the field list in that
state - which bit-set path the rebuild takes there is the one remaining
untraced half; the engine keeps the capture behaviour (white parked,
grey browsing).

**Hand + page arrows** (`80032f5c..8003304c`): hand
`FUN_8002B994(0, browsing, WX - 6, row_y)` (suppressed when parked, and
in mode 0 for window 11 - the Door of Wind list); blink-gated page
triangles (frame word `0x80084570 & 0x18`) - ICO `0x27` at
`(WX - 0xC, WY + h/2 - 3)` while scrolled, ICO `0x28` at
`(WX + w + 4, WY + h/2 - 3)` while rows remain. Against the live list
rect the capture-pinned right-arrow spot `(WX + 0x84, WY + 0x53)`
resolves these formulas at `w = 128, h = 172` - the live rect trims the
descriptor's `132x182`.

Engine port: `engine-core::pause_screens::list_kernel_navigate` (the
navigation phase, page-local wrap + page flip); the row/header draws
stay in `engine-ui::pause_lists` at the capture-pinned pens.

Shared helpers, both menu-overlay resident:

- **Cursor navigate `FUN_801D688C(cursor_ptr, rows, wrap)`** - held-pad
  confirm/cancel masks (`DAT_801EF0F0`/`F4`) return 1 (SFX `0x36`) / 2
  (SFX `0x37`); pad-edge Up/Down (`_DAT_8007BB84` bits `0x1000`/`0x4000`)
  move the cursor word's low 12 bits (SFX `0x21`), wrapping when `wrap`
  is set, and return 3. The high cursor-word bits pass through
  untouched (`0x4000` hide, `0x2000` dim variant, `0x1000` editing).
- **Window-script runner `FUN_801D6628(script)`** (this VA is the menu
  overlay's runner - the actor VM of `actor-vm.md` is a different
  overlay at the same base). A script is 4-byte entries
  `[op: u8][window_id: u8][arg: u16]`, op 0 terminates; the op jump
  table sits at `0x801CED70`. Ops used by the pause screens: 1 =
  create-if-absent + slide to the descriptor home rect, 2 = open at a
  packed position, 3 = poke live-window byte `+0x1D`, 4 = close
  (slide out), 5 = close all, 6 = zero live-window `+0x20` (snap the
  slide motion), 8 = destroy, 9 = create + slide to `arg`, 0x0A =
  destroy + re-create in place (content refresh keeping the animated
  position). The live windows are `0x5C`-stride structs (descriptor id
  at `+0x8`, rect at `+0xA`) - see the window-table notes above.

## Items screen

Four descriptor-table windows (draw order: tab 0, command 13, list 15,
info 17 - the live-list order of the pad-walked capture). The pause-menu
submenu word `DAT_801E46A4` holds `5` while the command window has focus
and `6` once the hand enters the list (`7` for the Throw Out list).

**Command window (id 13, `FUN_801D0D18`)** - three rows at `(WX+0x14,
WY + row*0xE)`: "Use" / "Throw Out" / "Arrange" (`@`-marker strings in
the menu-overlay rodata pool at `0x801CEA10..`). Text stages CLUT 7,
dropping to CLUT 0 when the bag scan (slots `0x80085958 + i*2` =
`[id, count]` over `_DAT_8007B5EA.._DAT_8007B5EC`) finds no held item.
The hand cursor (`FUN_8002B994`) draws at `(WX, row_y)` gated by the
cursor word `DAT_801E46C0`. See
`ghidra/scripts/funcs/overlay_menu_801d0d18.txt`.

**Item list (id 15)** - renderer-less in the descriptor table; the page
is drawn by the SCUS **kind-4 list kernel `FUN_80032A44`** (see [the
kernel section](#the-kind-4-list-kernel-scus-fun_80032a44)) over
class-`0x1000` bag rows. Rows start at `(WX+0xC, WY+0xC)`, pitch `0xE`,
12 rows per page: item name, then the bag count as 8-px digit cells
whose 2-digit field spans `WX+0x6C..0x7C` (a 1-digit count inks the
`WX+0x74` cell - the capture-pinned spot). The whole page draws CLUT-7
white while the command window has focus (the kernel is *parked*, mode
4, which forces white) and drops to CLUT-0 grey once the hand enters
the list (the per-row `0x800` dim bit; the hand at `WX-0xC` is the
selection highlight - no row tint). The header row sits above row 0:
the "PAGE" small-cap tag - ICO sprite `0x76`, atlas UV `(80,136)` 24x8,
teal ink `(16,181,156)` - at `WX+W-0x38`, and the gold `cur / total`
fraction (digit sprites ICO `0x7A..0x83`, slash `0x79`) from
`WX+W-0x20`, ending flush at the content right edge. Blink-gated page
triangles (ICO `0x27`/`0x28`) mark further pages - the right one at the
capture-pinned `(WX+0x84, WY+0x53)` (`PAGE 1 / 6` in the capture).

**Info window (id 17, `FUN_801DCB60`)** - draws only while an item id is
staged in `DAT_801E46B0`: the 2-digit bag count (CLUT 6) at
`(WX+0x7C, WY)` (count re-resolved through the bag-slot scan
`FUN_80042EE0`), then the shared item-info panel `FUN_801D0F1C`: name
(CLUT 6, the item-table `+4` pointer) at `(WX, WY)`, description (CLUT 7,
the item-table `+8` pointer - see
[`../formats/item-table.md`](../formats/item-table.md)) at
`(WX, WY+0x10)`, and -
for accessories - the passive-effect lines from the `0x8007625C` table
at `(WX, WY+0x38)` (CLUT 4) and `(WX, WY+0x48)` (CLUT 7), plus a
single/all-scope icon (`0x84`/`0x85`) at `WX+0x84`. A Point Card
(`id 0xFE`) instead draws "Points Left" at `(WX+0x18, WY+0x41)` with the
8-digit bank `_DAT_800845B4` at `(WX+0x38, WY+0x4E)`. The renderer
always emits a second framed widget box `FUN_8002C69C(WX, WY+0x38,
0x90, 0x28)` under its own window - the empty lower-left box of the
capture; the passive / points lines land inside it. See
`overlay_menu_801dcb60.txt` / `overlay_menu_801d0f1c.txt`.

### Command sub-flows (Use / Throw Out / Arrange)

The command SM `FUN_801D7C00` (submenu 5) phases through the shared
phase word `DAT_801E46AC`: phase 0 zeroes the staged item id/count
(`DAT_801E46B0`/`B4`), parks the list (`_DAT_8007BB94 = 4`) and re-runs
the screen's window script; phase 1 navigates the three rows with
`FUN_801D688C(&DAT_801E46C0, 3, 1)`. Every confirm first re-runs the
bag scan (slots `0x80085958 + i*2` over `_DAT_8007B5EA.._DAT_8007B5EC`,
a slot counts only when **both** id and count bytes are non-zero) and
buzzes (SFX `0x23`) on an empty bag. Then, by row: **Use** requests
submenu 6 (SFX `0x20`), **Throw Out** requests submenu 7 (SFX `0x20`),
**Arrange** stays in submenu 5 and jumps to phase 2, which calls the
sort kernel `FUN_801D64A8`, zeroes the list scroll
(`_DAT_8007BB90`/`BB98`), re-opens the list window and returns to
phase 1 (SFX `0x36`). Cancel (result 2) requests submenu 1 - back to
the top-level pause menu.

**Arrange kernel `FUN_801D64A8`** - the bag sort behind the Arrange row:
allocates a 256-byte scratch and inverts the menu overlay's
display-order table at `0x801E4A88` (`table[rank] = item_id`, file
offset `0x16270` in PROT 0899) into an id -> rank map (ascending fill -
a duplicated id keeps its last rank), then selection-sorts the bag slot
pairs by that rank, considering only occupied slots and breaking once
none remain - emptied slots sink behind the occupied run. See
`ghidra/scripts/funcs/overlay_menu_801d64a8.txt`; engine
`engine-core::menu_arrange` (parser + kernel).

**Use list `FUN_801D7E50`** (submenu 6): phase 0 hides the command hand
(cursor `|= 0x1000`), runs the window script (re-creating list 15 in
place, snapping windows 13/17), phase 1 arms the list kernel
(`_DAT_8007BB94 = 1`), phase 2 stages the hovered slot's id/count into
`DAT_801E46B0`/`B4` every frame and polls the kernel: cancel (3)
returns to submenu 5; a pick (2) dispatches on the picked item's
effect-class byte (item record `+1` indexes the `0x800752C0`
item-effect table - see
[`item-effect-table.md`](../formats/item-effect-table.md)) at
`801d7f80..801d7fd8`: class `0x80` -> submenu 0xB, `0x81` -> 0xC,
`0x82` -> 0xD, anything else -> submenu 9 (the all-party apply) when
the effect's `+2` flag bit `0x20` is set, else 0xA (the single-target
apply). All five routes are traced below; engine kernel
`engine-core::pause_screens::use_route_for_effect`.

**All-party apply `FUN_801D7FF8`** (submenu 9, the flag-`0x20`
effect route): phase 0 stages the pick - `FUN_801D6A54(item_id)`
derives the target-panel preview mode `DAT_801E46CC` from the picked
item's record (below) - then sets the target cursor `DAT_801E46C4` to
preview mode (`| 0x2000` - the all-row hand) and runs the window
script at `0x801E4C30` - close-all, reopen the tab, snap windows
13/17 and **open window 14, the party target panel**. Phase 1 polls
confirm/cancel (`FUN_801D688C(&DAT_801E46C4, 0, 0)` - count 0, so no
row navigation: the whole party is the target; cancel returns to
submenu 6). Confirm applies the pick: SFX `0x25`, the SCUS
item-effect applier `FUN_800402F4(effect_class, effect_arg,
roster_id[cursor], 0)`, the ability-bit rebuild `FUN_80042558`, then
consumes one copy from the bag slot (`FUN_80043048(slot, 1)`); when
the stack runs out (or the item stops resolving, `FUN_8003043C`) a
~20-frame timer phase (`DAT_801E46D0` reused as the accumulator
against the scratchpad frame delta `DAT_1F800393`) runs before
dropping back to the list - to submenu 5 when the bag emptied.

**Single-target apply `FUN_801D8308`** (submenu 0xA, the default
route): same shape with a navigable hand - phase 0 stages the preview
mode, clears the cursor's high bits and runs the script at
`0x801E4C48` (identical to submenu 9's - same window set); phase 1
navigates the party rows (`FUN_801D688C(&DAT_801E46C4,
party_count(0x80084594), 1)`, gated on `_DAT_8007BB80 == 0`); a
confirm first re-checks usability through the SCUS validator
`FUN_8003FB10(effect_class, effect_arg, roster_id[cursor])` - failure
buzzes (`801d8480`, cue `0x23`) - then sets cursor bit `0x1000`
(static hand) and applies via the same
`FUN_800402F4`/`FUN_80042558`/`FUN_80043048` chain (`801d84b0..`).
When the applier reports a result through `_DAT_8007BB78` (seeded
`0xFF` before the call, `801d850c`), a notify window opens (script
`0x801E4C60` = open window 8, renderer `FUN_801DCD58`) and waits for a
confirm press before closing (`0x801E4C68`). Exhaustion runs the same
20-frame timer + bag rescan.

**Preview-mode derivation `FUN_801D6A54`**: mode 0 unless the item's
record kind byte is `2` **and** its effect class is `6` - the
permanent-stat Waters. The effect arg maps `0 -> 1` (Life Water),
`5 -> 1` (Magic Water - shares the HP/MP panel), `1 -> 2` (Power
Water), `2 -> 3` (Guardian Water), `3 -> 4` (Swift Water), `4 -> 5`
(Wisdom Water). Engine kernel
`engine-core::pause_screens::target_panel_mode`.

**Party target panel (window id 14, rect `(174,28,132,176)`, renderer
`FUN_801D0520`)** - replaces the item list column during target pick.
One block per roster member (ids `0x80084598`, count `0x80084594`,
roster byte `< 3` only), pitch `0x3E`. Header: name (record `+0x2A7`)
at `WX+0x14`, LV icon (ICO `0x0A`) at `(WX+0x58, Yb+2)`, 2-digit level
(`+0x130`) at `WX+0x68`. Body switches on the preview word
`DAT_801E46CC`:

- **Plain rows** (mode 0, and modes 2/4/5): HP icon (ICO `0x3F`) at
  `(WX+0x1C, Yb+0x11)`; 4-digit current HP (`+0x106`) at `(WX+0x2C,
  Yb+0xF)` staged with the `FUN_800349EC` tier ink; slash
  (`FUN_8003C1F8` cell 6, white) at `WX+0x4C`; 4-digit max (`+0x104`)
  at `WX+0x54` (`801d0764..801d07d4`). MP row (ICO `0x40`, `+0x10A` /
  `+0x108`, `FUN_80035EA8` tier) mirrors it at `Yb+0x1E`/`Yb+0x1C`
  (`801d07ec..801d0850`) - skipped in mode 3 (`801d07e4`).
- **Mode 1** (Life/Magic Water): both HP and MP rows draw
  `eff_max ( base_max )` - tags ICO `0x64`/`0x3F` (HP) and
  `0x65`/`0x40` (MP) at `WX+0x14`/`WX+0x28`; the effective maximum
  (`+0x104`/`+0x108`) white at `WX+0x38`; the teal paren group
  (`FUN_8003C1F8` cells 7/8, staged 5 - the status screen's
  parenthesised-value ink) at `WX+0x58`/`WX+0x80` around the
  record-side base maximum (`+0x11C`/`+0x11E`) at `WX+0x60`
  (`801d0658..801d0850`).
- **Stat rows** (modes 2..5, after running the equip-stat aggregator
  `FUN_801CF650(roster_id)` at `801d0854`): `LBL eff ( base )` -
  label ("ATK"/"UDF"/"LDF"/"SPD"/"INT", overlay rodata
  `0x801CE9A0..B0`) at `WX+0x1C`; the aggregator word
  (`DAT_801EF08C/90/94/98/9C`, clamped 999) 3-digit at `WX+0x44`;
  paren cells at `WX+0x5C`/`WX+0x7C` around the record base stat
  (`+0x124/+0x126/+0x128/+0x12A/+0x12C`) at `WX+0x64`. Modes 2/4/5
  draw one row at `Yb+0x29` (ATK/SPD/INT); mode 3 draws UDF at
  `Yb+0x1C` **and** LDF at `Yb+0x29` in the skipped MP row's place
  (`801d08a4..801d0c38`).

The earlier "HP-restore preview" reading of mode 1 is superseded: the
modes are the permanent-stat **Water previews** (effective vs base
maxima/stats); restore items use the plain mode-0 panel. Hand cursor
(`801d0c40..801d0c94`) decodes `DAT_801E46C4`: bit `0x4000` hides it,
bit `0x2000` draws it on every row (all-party), else the low 12 bits
pick the row; bit `0x1000` drops it to the static sprite variant.
Drawn `FUN_8002B994(0, variant, WX, Yb)`. Engine port:
`engine-ui::target_panel_draws_for` / `target_panel_sprites_for`, fed
by `engine-core::pause_screens::target_panel_model`. See
`overlay_menu_801d0520.txt`.

**Door of Light route `FUN_801D8A58`** (submenu 0xB, effect class
`0x80`): phase 0 zeroes the confirm cursor `DAT_801E46D0` (**Yes** is
the default - unlike the Throw Out confirm) and opens window 10
(script `0x801E4CBC`; renderer `FUN_801D1DAC`, rect `(76,100,168,40)`).
Yes consumes one **`0x88` Door of Light** (`FUN_80042310(0x88, 1)`,
`801d8b20`) and exits the menu: `DAT_801E46A0 = 0xF2` (fade) +
outer-SM exit code `_DAT_8007B43C = 4` (`801d8b5c..801d8b6c`) - the
field-side dungeon-escape handoff. No or cancel returns to submenu 6.

**Door of Wind route `FUN_801D8B90`** (submenu 0xC, class `0x81`):
phase 0 parks the kernel and saves the Use-list scroll
(`_DAT_8007BB98/90 -> DAT_801EF070/74`, `801d8bd8..801d8bf8`); phase 1
zeroes the scroll and opens **window 11** - the destination list, same
rect as list 15, renderer-less, kernel-driven (the kernel hides its
hand for window `0xB` while idle); phase 2 re-arms the kernel; phase 3
on a pick reads the 6-byte quick-travel placement record
`0x80073A98 + slot*6` (the `FUN_80030628` case-`0x19` landmark table -
`legaia_asset::worldmap_menu`) and stages `+2 -> 0x80084628`,
`+4 -> 0x80084624`, `+5 -> 0x8008462C` (`801d8c88..801d8ccc`), consumes
one **`0x89` Door of Wind** and exits with code `_DAT_8007B43C = 5` -
the world-map warp. Cancel restores the saved scroll and returns to
submenu 6.

**Incense route `FUN_801D8D94`** (submenu 0xD, class `0x82`): window
12 Yes/No confirm (script `0x801E4CE4`; renderer `FUN_801D1F10`, rect
`(76,88,168,54)`), cursor seeded Yes. Yes consumes one **`0x8A`
Incense** and applies the encounter suppression through the standard
applier - `FUN_800402F4(class, arg, roster_id[target-cursor], 0)` with
the class/arg read live from Incense's own item-effect record (the
`lbu 0x49e1(0x8007xxxx)` at `801d8e78` is item record `0x8A`'s `+1`
effect-index byte), then returns to the Use list (submenu 6). No
menu exit - the flow stays on the Items screen.

Engine port of the three special routes:
`engine-core::pause_screens::SpecialUseSession` (+ the fixed consume
ids `0x88/0x89/0x8A` and exit codes 4/5 as named constants).

**Throw Out list `FUN_801D8734`** (submenu 7): phase 0 re-points the
live list window from descriptor 15 to descriptor 16 (live-window
`+0x8` id write; descriptor 16 shares list 15's rect with a zeroed `f1`
word) and runs the enter script; phase 1 arms the kernel; phase 2
stages the hovered slot and polls - cancel restores descriptor 15 and
returns to submenu 5; a pick opens the **confirm window** (script: close
command window 13, open window 9) with the confirm cursor
`DAT_801E46D0` seeded to **1 ("No")**, phase 3. Phase 3 navigates the
two rows (`FUN_801D688C(&DAT_801E46D0, 2, 1)`); confirming row 0 (Yes,
SFX `0x37`) **zeroes both bytes of the selected bag slot pair** - the
whole stack is discarded, no compaction - then applies a scroll fix-up
(deleting the last row steps the selection and scroll back one) and
closes the confirm; if the bag rescan comes up empty the flow restores
descriptor 15 and drops back to submenu 5, otherwise the list re-arms.
"No" or cancel just closes the confirm back to the list.

**Confirm window (id 9, rect `(14,38,144,54)`, renderer
`FUN_801D1B20`)** - drawn from the staged slot (`_DAT_8007BB88`): the
item name (CLUT 7; the item-table `+4` string, whose leading byte is
its glyph count) at `(WX, WY)`; the bag count (min-1-digit
`FUN_80034B78`) at `WX + 8 + glyphs*0xC`; "You are about to" 8 px past
the count (16 px for a 2-digit count) on the same row; "Throw out?" at
`(WX+6, WY+0xE)`; then CLUT 5: "Yes" at `(WX+0x3C, WY+0x1C)` and "No"
at `(WX+0x3C, WY+0x2A)` with the hand at `WX+0x28` on the focused row
(cursor word `DAT_801E46D0`, the standard hide/dim bit layout). The
name renderer also honors a `0xF1` second-byte escape substituting a
character-record name (`record+0x2A7`). The four strings live in the
menu overlay's leading rodata pool (`@You are about to` at
`0x801CEA60`, `@Throw out?` / `@Yes` / `@No` following). See
`overlay_menu_801d1b20.txt` / `overlay_menu_801d8734.txt` /
`overlay_menu_801d7c00.txt` / `overlay_menu_801d7e50.txt`.

Engine port: `engine-ui::items_screen_draws_for` /
`items_screen_sprites_for` (window contents + hand / page arrows at the
pinned pens) plus `items_throw_confirm_draws_for` /
`items_throw_confirm_sprites_for` (the id-9 confirm window), fed by
`engine-core::pause_screens` (`PauseItemsSession` - the
command/list/throw-out focus model over the item-use flow, real bag
counts, page flip, the Yes/No confirm defaulting to No, whole-stack
discard) with names / descriptions / accessory passive lines resolved
from the executable via `pause_screens::MenuTextTables`
(`World::install_menu_text`) and the Arrange ranks via
`World::install_menu_overlay_tables` (`engine-core::menu_arrange`,
id-order fallback without the overlay). Discards reach the world bag
through `field_menu_dispatch::apply_inventory_outcome`. The engine
draws the Throw Out list with the same paged layout as the Use list -
whether the retail descriptor-16 variant pages or row-scrolls hangs on
the untraced `f1` word (the deletion fix-up's pitch subtraction hints
at row scrolling). Both hosts (play-window + the web play page) render
this screen through the same builders.

## Magic screen

Four descriptor-table windows (draw order: tab 1, list 18, caster 19,
info 20). Submenu word `0x0E` = caster focus, `0x0F` = list focus.

**Caster window (id 19, `FUN_801D2C98`)** - one block per roster member
(ids at `0x80084598`, count `0x80084594`, roster byte `< 3` only) at
`Yb = WY + 1 + i*0x23`: name (CLUT 7, record `+0x2A7`) at `WX+0x14`; LV
icon (ICO `0x0A`) at `(WX+0x60, Yb+2)` with the 2-digit level
(`+0x130`) at `WX+0x70`; MP icon (ICO `0x40`) at `(WX+0x24, Yb+0x10)`
with the 4-digit current (`+0x10A`) / slash (`FUN_8003C1F8` code 6,
CLUT 7) / 4-digit max (`+0x108`) at `WX+0x34 / +0x54 / +0x5C` on row
`Yb+0xE` - the numbers stage the `FUN_80035EA8` MP tier ink. Hand
cursor at `(WX, Yb)`, gated by this screen's cursor word
`DAT_801E46C8`. See `overlay_menu_801d2c98.txt`.

**Spell list (id 18)** - renderer-less; same capture-pinned page layout
as the item list (rows from `(WX+0xC, WY+0xC)`, pitch `0xE`, 12 rows,
PAGE header, white -> grey focus drop, hand at `WX-0xC`). Each row is a
single string whose leading `0xCE` escape draws the element icon plate,
so the name ink starts 25 px right of the row pen (the wider winged
Ra-Seru-magic icon advances 22 px - "Meta" indents differently in the
capture).

**Info window (id 20, `FUN_801D2E74`)** - draws only while a spell id is
staged in `DAT_801E46B0`: the spell-name string (CLUT 6, leading element
icon) at `(WX, WY)`; the learned level - looked up in the highlighted
character's spell list (`+0x13C` count / `+0x13D` ids / `+0x161`
levels) - as a "Lv`n`" string at `WX+0x78`; the description string
(`stats[+4]` index into the pointer table at `0x80075DB0`, CLUT 7,
multi-line at the `0xE` pitch) from `(WX, WY+0xE)`; then "MP Used"
(CLUT 4) at `(WX+0x18, WY+0x2A)` with the 3-digit cost at `WX+0x74` -
the base cost `stats[+3]` run through the MP-cost kernel
`FUN_80035394`, digits drawn in the same green. See
`overlay_menu_801d2e74.txt`.

Engine port: `engine-ui::magic_screen_draws_for` /
`magic_screen_sprites_for`, fed by `engine-core::pause_screens::magic_screen_model`
over the field spell-menu session (caster focus = `CharSelect`, list
focus = `SpellSelect`; per-caster MP cur/max + learned levels from the
character records, descriptions via `MenuTextTables` /
`legaia_asset::spell_names`). The per-caster MP-cost kernel discount
(`FUN_80035394`: the `+0xF4` ability word's Half bit `0x20` shaves 50%,
Quarter bit `0x10` shaves 25%, Half winning when both are set) is applied to
the displayed cost, matching the retail info window and the battle cast path.

## Dialog reading box (FUN_801D84D0)

The field dialog pager `FUN_801D84D0` (dialog overlay) draws the NPC /
event message box with the same window emitter `FUN_8002C69C` the menu
uses. Geometry, pinned from the live pager context (`*DAT_801C6EA4`) in
the `v0_1_tetsu_dialogue_accept` save state plus the on-screen
framebuffer:

- **Reading box centre rect** = `(ctx+0x12, ctx+0x14, 0xF4,
  lines*0xF + 5 - 8)` with `ctx+0x12 = 0x26` (38), `ctx+0x14 = 0x10`
  (16) and `_DAT_801F2740 = 3` lines - the box sits at the **top** of
  the screen. The emitter's standard skin extends ~8 px beyond the
  centre rect on every side: measured footprint `x 30..289, y 8..65`,
  with the outermost 4 px as the tan border band and the translucent
  gouraud gradient fill spanning the rest (centre inflated by 4).
- **Interior fill** = two stacked semi-transparent gouraud `POLY_G4`
  quads (top RGB `(0x18,0x18,0x28)`, bottom `(0x40,0x40,0xA0)`,
  composing to `0.25*back + 0.75*gradient`).
- **Text pen** = the box origin exactly: each line draws at
  `FUN_80036888(line, 0, 0, ctx+0x12, ctx+0x14 + i*0xF)` with the ink
  staged CLUT 7. Measured first-line ink starts at `x 38, y 18`.
- **Advance hand** (page-wait state `0x19`) = `FUN_8002B994(1, 1,
  0x10A, ctx+0x14 + lines*0xF - 0x13)` - `0x10A = x + w - 0x10` for the
  standard box.
- **Option picker box** = `x 0x26, y 0x94 + ((4-n)*0xF)/2, w 0xF4,
  h 0x38 - (4-n)*0xF` (2..4 options); option rows at `x+0x10`,
  `y + i*0xF`; hand cursor `FUN_8002B994(0, 1, x-6, y + cursor*0xF)`.

Engine port: `engine-ui::dialog_window_chrome_draws_for` (centre-rect
semantics, border+fill inflation), `dialog_advance_hand_sprite`,
`dialog_option_hand_sprite`; the play-window's `dialog_stage_layout`
carries the rects.

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

### Ink CLUT rows

The staged index selects a 16-colour CLUT at VRAM `(16*(6+index), 510)`
(the in-prim halfword `index + 0x7f86` decoded as `x = (c & 0x3f) * 16`,
`y = c >> 6`). The **main ink is palette entry 15**; entries 12..14 hold
the outline/shade ramp. Entry-15 values read off the golden
`menu_status_town` VRAM:

| index | entry-15 RGB | role |
|---|---|---|
| 0 | `(132,132,132)` | grey (non-selected rows) |
| 1 | `(107,107,231)` | lavender (command labels) |
| 2 | `(231,33,0)` | red (downed - 0 HP) |
| 4 | `(107,222,107)` | green (skill passives) |
| 5 | `(66,222,222)` | teal (separators / base values) |
| 6 | `(231,173,0)` | gold (warning tier, headers) |
| 7 | `(206,206,206)` | white (default text) |
| 9 | `(222,90,0)` | orange (critical tier, moves header) |

### HP / MP health-tier inks

The status page and the top-level party panel stage the HP / MP number
fields (current **and** max) through two per-character tier functions:

- **`FUN_800349EC`** (HP): `hp == 0` → 2 (red); `hp <= max/4` → 9
  (orange); `hp <= max/2` → 6 (gold); else 7 (white). A non-zero
  status halfword at record `+0x12E` forces the gold tier at any HP.
- **`FUN_80035EA8`** (MP): same quarter/half thresholds without the
  zero case (`mp <= max/4` → 9, `<= max/2` → 6, else 7).

Engine port: `engine-ui::menu_hp_ink` / `menu_mp_ink`.

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

## Inn stay (there is no inn screen)

An inn stay is not a menu, a session, or a native routine. Retail composes
it **inline in the scene's MAN script** out of ops this page's sibling
[`script-vm.md`](script-vm.md) already documents, and the only
inn-specific thing in the whole engine is one opcode that heals.

The restore is `0x4C` outer-nibble-8 sub-2:

```text
4C 82 <slot>        ; 3 bytes, PC += 3
```

Against the 0x414-stride character record based at `0x80084708`, the
dispatcher arm writes each max into its current:

```text
*(u16 *)(record + 0x106) = *(u16 *)(record + 0x104);   ; hp_cur = hp_max
*(u16 *)(record + 0x10A) = *(u16 *)(record + 0x108);   ; mp_cur = mp_max
```

The max/current roles are corroborated by the level-up routine, which
grows `+0x104`/`+0x108`, clamps them to 9999/999, then clamps
`+0x106`/`+0x10A` against them (see `ghidra/scripts/funcs/80042558.txt`).
The slot is a **literal operand**: a script that heals slots 0/1/2 heals
exactly those records rather than walking the active party.

A paid stay wraps that restore in generic ops:

| Step | Op |
|---|---|
| Innkeeper's greeting + the price line | `0x1F` dialogue segments |
| Yes / No | MES-embedded option picker |
| Can the player afford it? | `0x4E` gold gate, jumping to the refusal line |
| Take the money | `0x3A` `ADD_MONEY` with the negative charge |
| Fade out, wait, fade in | `0x34` / `0x35` / `0x36` + `0x4A` |
| Heal | one `4C 82 <slot>` per party member |

Two consequences fall out of that shape. The charge and the restore are
**fully decoupled**, so a free rest (a bed, an infirmary) is the same tail
with the gate and the debit dropped - which is why restore triples appear
in many more scenes than gold gates do. And the price lives in the script,
which is why `legaia_asset::inn_costs` locates the charges per scene and
why there is no inn cost table to find.

Scenes carrying a gate + debit pair include `retock` (240 G), `ropeway`
and `rayman2` (200 G), `koin1` (280 G) and `koin2` (200 G); `koin4` and
`koin1b` carry several sites each. Some inns append a story-flag-gated
tail that sets a system flag and `0x3F`-warps to a `DREAM` scene - the
restore still runs first, unconditionally.

The engine hosts the opcode at
`engine-core::world::vm_hosts::op4c_n8_sub2_restore_party_slot`, so retail
inn scripts heal the live party on both hosts. `MenuRuntime::open_inn`
remains an engine-side convenience (a yes/no session with an explicit
cost) for tests and tooling - it is **not** a port of a retail screen,
because retail has none.

## Engine port

Every draw builder named on this page lives in **`legaia-engine-ui`**, not in
`legaia-engine-render`. `engine-render` re-exports the whole crate
(`pub use legaia_engine_ui::*`), so an `engine-render::` path still compiles -
but the code is in `engine-ui`, and the distinction is the point of the split:
`engine-ui` is the renderer-agnostic, wgpu-free leaf, which is what lets the
browser play page build these same menus without linking wgpu.

The clean-room engine parses the window-descriptor table from the user's
disc at boot (`legaia_asset::menu_windows`; the play-window falls back to a
pinned mirror of the same rects) and frames each screen's window set with
the reusable 9-slice primitive `engine-ui::menu_window_chrome_draws_for`
(the caller-drawn window frame), placed on the shared 320x240 boot-UI stage
via `engine-ui::scale_stage_text_draws`. The frame chrome and the navy
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
through `engine-ui::status_screen_draws_for` at the byte-pinned offsets
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
`engine-ui::status_icon_sprites_for` at the pinned status offsets while
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
carved plaque via the shared `engine-ui::tab_banner_draws` (cap /
tiled body / cap, CLUT row 12) with the label in CLUT-7 white; tab
windows draw no 9-slice frame. Number fields lay out on the retail
fixed 8-px digit cells (`num_field_draws`), and the parenthesised
base/growth groups use the retail teal ink. The Equip screen renders its
retail four-window set (tab 2 + party 21 + item-list 23 + main 22)
through `equip_screen_draws_for` + `equip_screen_sprites_for` at the
traced `FUN_801D21C0` / `FUN_801D2094` offsets (see
[Equip screen](#equip-screen)). The top-level menu renders the traced
row / money-box / party-panel content (see
[Top-level pause menu](#top-level-pause-menu)). The Items and Magic
screens' retail window sets and content layouts live in
`engine-ui::pause_lists` (`items_screen_draws_for` /
`magic_screen_draws_for` + `_sprites_for` siblings, window ids + pinned
rect fallbacks in the same module): the command / caster / info windows
at the decompile-pinned pens above, the list pages at the
capture-pinned rows with the white-to-grey focus drop, hand cursors and
page arrows from the system-UI atlas. The spell element-icon plates and
the "PAGE" small-cap tag are not yet ported as sprites - the builders
hold their measured gaps / text stand-ins. Hosts still frame these
screens generically pending the play-window / web wiring; the
HP / MP health-tier inks on the status page remain the other open
fidelity item.


### Tactical Arts chain editor (engine extension)

The chain editor (`engine-ui::tactical_arts_editor_draws_for`, backed by
`engine-core::tactical_arts_editor::ChainEditor`) has **no retail
pause-menu row**: retail's top-level list is the seven rows above, and
composing a named command chain outside battle is not a retail feature.
It is an opt-in engine extension, and it needs an entry point that does
not invent an eighth row.

That entry is Triangle on the **Status** screen, which swaps the status
sub-session for a chain editor on the character the panel is currently
showing (`engine-core::field_menu_dispatch::try_open_arts_editor`).
Status is the retail surface that lists a character's arts, and retail's
status panel reads Left / Right / L1 / R1 / Circle / Start only - so
Triangle is unclaimed there and the extension costs no retail input.
Closing the editor parks the resume cursor back on Status.

Both hosts reach it through the same seam, and both project the live
editor through the shared `field_menu_dispatch::arts_editor_view` - the
character-name lookup, the pretty-printed sequences, the phase mapping
and the "+ New" room check are one implementation, so the two hosts
cannot drift apart on them. Saving folds the edit back into the world's
saved chains via `World::chain_library` / `store_chain_library`, so the
next battle's Arts rows reflect it.
