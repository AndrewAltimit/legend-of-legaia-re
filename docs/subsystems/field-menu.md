# Field Menu - Status Panel Renderer

Covers `FUN_801D33D8`, the per-character **status / party panel** renderer.
The field pause menu (game mode `0x17`, the CARD-mode pair) opens this panel
for the Status, Magic, Moves, and Skills tabs; it draws one party member's page
into a caller-supplied window rect. It lives in the **menu overlay** (the same
binary as shop / inn / save; base `0x801CE818`). Source:
`ghidra/scripts/funcs/overlay_menu_801d33d8.txt` plus the shared draw
primitives `ghidra/scripts/funcs/80036888.txt` (string), `8002c488.txt`
(UI-icon sprite), `80034b78.txt` (decimal number).

The panel draws **content only**. The bordered 9-slice window frame is emitted
by the caller, not here (this function never draws a box). Every position below
is an offset from the window origin, which the caller passes in the rect struct
`a0`: `WX = *(i16*)(a0+0xa)`, `WY = *(i16*)(a0+0xc)`. The rect also carries a
width-ish field at `a0+0xe` (scroll-arrow X and scrollbar length) and a height
field at `a0+0x10` (bottom-anchored scrollbar Y). The absolute window
placement is caller data and is not recoverable from this function.

## Contents

- [Plumbing](#plumbing) · [Submenu dispatch](#submenu-dispatch)
- [Header row](#header-row-always-drawn) · [Status page](#status-page-submenu-0-or-5)
- [Magic list](#magic-list-submenu-2) · [Moves list](#moves-list-submenu-3) · [Skills page](#skills-page-submenu-1)
- [Draw primitives + CLUT staging](#draw-primitives--clut-staging)
- [Record fields consumed](#record-fields-consumed)

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
`801d35e8`..`801d374c`.

**HP gauge**: bar primitive at `(X+0x40, WY+0x2d)`, value record `+0x10e`,
length set by `FUN_80034b6c(0x31)`. Then `s8 += 0x2f`.

**Derived-stat grid** (`FUN_801cf650` computes the values first). 3 rows at
`WY+0x42 / +0x4f / +0x5c`, two columns. Left column: label `X+0`, live value
`X+0x28`, growth value `X+0x48`. Right column: label `X+0x74`, live value
`X+0x9c`, growth value `X+0xbc`. Live values clamp at 999 and come from
`DAT_801ef088..09c`; growth values from record `+0x122..+0x12c`. Then
`s8 += 0x2b`. Instr `801d3780`..`801d3b48`.

**Equipment grid** (7 slots): icon + item name. Icon codes from the fixed
array `DAT_801e43f4..4400`; item name via the item-name table
`*(u32*)(0x8007436c + id*0xc)` where `id = *(u8*)(record + 0x196 + slot_off)`.
Slots 0..3 stack at `X+0/+0x10` on rows `WY+0x6d / +0x7a / +0x87 / +0x94`;
slots 4..6 sit in a right column at `X+0x6a/+0x7a` on rows `WY+0x7a / +0x87 /
+0x94`. Then `s8 += 0x38`. Instr `801d3b4c`..`801d3dd8`. Item ids resolve
through [`item-table.md`](../formats/item-table.md).

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

## Scroll widgets (submenu 2 or 3)

Up arrow (icon `0x67`) when `_DAT_8007bb90 > 0` and down arrow (icon `0x68`)
when more rows follow, both at `X = WX + (a0+0xe >> 1) - 4`. Scrollbar thumb
(bar primitive) at `(WX, WY + (a0+0x10) - 0x28)`, length from `a0+0xe`,
`FUN_80034b6c(3)`. Instr `801d477c`..`801d4838`.

## Draw primitives + CLUT staging

Three shared primitives render everything:

| tag | function | signature | notes |
|---|---|---|---|
| STR | `FUN_80036888` | `(str, x, count, y)` | proportional string; MES control tokens `0x7c` (advance x by `0xe`), `0xce`/`0xcf` |
| ICO | `FUN_8002c488` | `(x, y, code)` | one UI-icon sprite; UV/CLUT from a 12-byte-stride table at `0x800732a4`, codes `0x86..0x8a` from `0x80073db8` |
| NUM | `FUN_80034b78` | `(value, x, y, digits)` | decimal digits vs the powers-of-ten table at `0x80073dcc` |

The palette-staging global is **`DAT_8007b454`** (`0x80080000 - 0x4bac`);
the in-primitive CLUT halfword is `index + 0x7f86`. It is **read only by the
string primitive** `FUN_80036888` (at `80036b74`). Icon and number primitives
carry their own CLUT (icon from the `0x800732a4` table, number from
`gp+0x13c`), so a `DAT_8007b454` write immediately before an ICO/NUM draw is
inert for that draw and is really staging the palette for the next string.
Distinct values seen: 7 (default text), 5 (status separators), 6 (magic
header + skill labels), 9 (moves header), 4 (skill passives), 1 (command
label + arrows), 0 (non-selected magic rows).

## Record fields consumed

Field offsets into the `0x414`-stride live record emitted by this panel:

| offset | field |
|---|---|
| `+0x0` | cumulative experience (8-digit) |
| `+0x4` | next-level threshold |
| `+0x104 / +0x106 / +0x11c` | HP max / current / base |
| `+0x108 / +0x10a / +0x11e` | MP max / current / base |
| `+0x10e` | HP-bar value |
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

The clean-room engine renders this panel through
`engine-render::status_screen_draws_for`, framed by the reusable 9-slice
primitive `engine-render::menu_window_chrome_draws_for` (the caller-drawn
window frame) and placed on the shared 320x240 boot-UI stage via
`engine-render::scale_stage_text_draws`. The HP/MP/level/equipment values come
from the typed character record in `legaia_save`.
