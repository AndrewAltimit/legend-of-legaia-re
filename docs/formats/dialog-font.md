# Dialog font (proportional Latin)

The proportional sans-serif font used by the dialog box, the field menu, and most in-game UI text. It lives in VRAM at runtime and is referenced by every text-rendering primitive the engine emits.

The font has three pieces of static data, all in `SCUS_942.54`:

1. A **256-byte width table** at `0x80073F1C`, indexed by character byte.
2. A **38-entry escape-sequence table** at `0x80074050`, indexed by the byte that follows a `0xCE` runtime escape: controller-button and icon sprites, plus four number slots (see [Escape table](#escape-table-0x80074050)).
3. The **glyph bitmaps**, which sit in VRAM at `(896, 0)..(960, 256)` (a 4bpp tile-page covering 256×256 source pixels). They're loaded from disc into VRAM by an overlay-resident routine.

### On-disc carrier

The glyph tile-page is a plain PSX TIM at **`PROT.DAT` file offset `0x7F40`**: a
4bpp image whose framebuffer is `(896, 0)`, `64` halfwords wide × `256` tall
(= 256×256 4bpp pixels), with a 16-entry CLUT block declaring destination
`(0, 510)`. The font page uses only three palette indices: `0` = transparent
background, `14` = the `(32,32,32)` drop-shadow, `15` = the glyph fill.

This means the font is decodable **straight from the disc, no save state
required** - `legaia_font::Font::from_disc_tim_and_scus` reads this TIM plus the
SCUS width table and produces the same whitewashed atlas the save-state
extraction (`font-extract`) yields. Pinned by a PROT.DAT-wide TIM scan for that
framebuffer (constant `legaia_font::FONT_TIM_PROT_DAT_OFFSET`), byte-verified
against `extracted/font/dialog_font_atlas.png`
(`font::disc_font_matches_extracted_artifacts`).

## Glyph layout in VRAM

Format: 4bpp indexed, 16-pixel × 16-pixel cells, 16 columns × 14 rows = 224 cells. Cell `c` (for character byte `c` in the range `0x20..=0xFF`) lives at:

```
U = (c & 0x0F) * 16
V = (c & 0xF0) - 0x20
```

Drawn region within each cell is 14 pixels wide × 15 pixels tall (`W=0x0E`, `H=0x0F` in the GP0 0x64 packet). The remaining 2 pixels of width and 1 pixel of height per cell are inter-glyph guard space.

Character codes `0x00..=0x1F` are reserved for control / escape bytes (`0x7C` newline, `0xCE` escape prefix, `0xCF` color change, `0x20` space) - they do not have glyphs.

## Width table (advance lookup)

```
0x80073F1C  u8 widths[256]
```

256 bytes, indexed by character byte. The advance for character `c` is computed as:

```
advance = widths[c] + DAT_800740E8 + 1
```

where `DAT_800740E8` is a per-call padding override, reset to zero at the end of each render call. Menus and battle draw with it at zero; the field dialog pager `FUN_801D84D0` stores `1` before every row it draws (`0x801D97D8`) and before every option-picker label (`0x801D9B78`), so dialogue runs one pixel wider per glyph than the same text in a menu (see [Line width and wrapping](#line-width-and-wrapping)). The trailing `+1` is a fixed inter-character gap. The port lays a dialogue row out through `Font::layout_padded` at `measure::DIALOG_GLYPH_PAD` in the builders both play hosts draw with (`engine_ui::dialog_reading_box_text_draws_for`, `dialog_picker_label_draws_for`).

The advance is applied by the **common tail** of `FUN_80036888`'s per-byte loop
(body `0x80036B9C`), which every byte reaches except the four control bytes
`0x00` / `0x7C` / `0xCE` / `0xCF`. The space byte `0x20` branches around the
sprite emit only - it still takes the advance, so a run of spaces measures
`n * (widths[0x20] + 1)`. The table is addressed in biased form
(`0x80073F3C` indexed by `c`, loaded at `-0x20`), which is the same
`0x80073F1C + c` the doc quotes above.

Consequence for measurement: a whole line's pixel width is exactly the sum of
its per-byte advances, with no per-line fudge term. `"Do not remove MEMORY
CARD"` (the save screen's memory-card warning) measures **164 px**; the engine
asserts that number against a disc-decoded font in
`engine-shell/tests/dialog_font_metrics.rs`.

Bytes `widths[0x00..=0x1F]` overlap with three actor-name strings ("Meta", "Terra", "Ozma") that live at `0x80073F24..0x80073F3B`; only entries `0x20..=0xFF` are meaningful for glyph advance.

Sample widths from the table:

| `c` | char | width |
|---|---|---|
| `0x20` | ` ` | 4 |
| `0x21` | `!` | 4 |
| `0x41` | `A` | 7 |
| `0x49` | `I` | 3 |
| `0x4D` | `M` | 8 |
| `0x57` | `W` | 9 |
| `0x69` | `i` | 3 |
| `0x6D` | `m` | 8 |
| `0x7E` | `~` | 9 |

The full table is dumped to `extracted/font/dialog_font_widths.csv` and `extracted/font/dialog_font_metadata.json` as part of the extraction step.

## CLUT

```
VRAM (96, 510)   // CLUT 0 - dialog grayscale (white-on-transparent text)
VRAM (96 + 16*i, 510)   // CLUT i - colored variants for status text, system prompts
```

Sixteen 16-color CLUTs are placed end-to-end across VRAM Y=510, one every 16 horizontal pixels. CLUT 0 is the canonical dialog palette: index 2 = transparent black, index 3 = white, indices 0/1/4..7 = mid-tone grays for anti-aliasing.

The runtime selects which CLUT to use via `DAT_8007B454`, modifiable inline by the `0xCF` color-change escape (see below). The CLUT word written into the GP0 packet is `DAT_8007B454 + 0x7F86`; the constant `0x7F86` decodes as VRAM CLUT-coords `(96, 510)`, so `DAT_8007B454` is just an additive index 0..15.

## Escape table (`0x80074050`)

Triggered by byte `0xCE` in the rendered string. The byte that follows indexes a 4-byte record:

```
struct EscapeEntry {
    i16  string_id;   // 0 = print a number; nonzero = sprite id for FUN_8002C488
    u8   advance_px;  // pixel advance after rendering this escape
    i8   y_offset;    // sprite Y offset from the line top (or counter index when string_id == 0)
};
```

There are 38 entries (table indices `0x00..=0x25`). An operand past `0x25` reads whatever follows the table.

- **Sprite escapes** (`string_id != 0`): `FUN_80036888` calls `FUN_8002C488(x, y + y_offset, string_id)`. The `string_id` is not a string: it is a **UI-icon sprite id** into the 12-byte records at `0x800732A4` (U/V/W/H and a CLUT byte; the record layout and the CLUT-byte encoding are in [`field-menu.md`](../subsystems/field-menu.md)). None of the ids here takes the special `0x86..=0x88` / `0x8A` path.
- **Number escapes** (`string_id == 0`): the renderer prints the signed halfword at `0x801C6460 + y_offset * 2` through `FUN_80034B78` (`0x80036A54..0x80036A94`). That array is the field VM's script-counter slot table, written by `4C CA/CB/CC` ([`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md)). The writer is called with a minimum width of `0`, suppresses leading zeros and always draws the units digit, so any value `<= 0` prints a single `0`. On the disc only `0x0B` and `0x0E` occur, in the `koin1` and `koin3` dialogue. The port resolves them when a box opens (`World::dialog_substitutions` -> `dialog::script_counter_digits`) and types the number as the escape's one reveal unit.

The sprites decode against the **system-UI sheet** at VRAM `(896, 256)`: its UVs land on the icons there and on accent-glyph cells in the font page. The sprite carries no texture page of its own, and the `DR_MODE` for the font page (tpage `0xE`) that `FUN_80036888` links at `0x800369B8` goes into the same OT slot *before* the sprite, which the slot's head insertion makes execute after it. Every texel and palette sits in the boot-resident TIMs at the head of `PROT.DAT` (the system-UI sheet at `0x018E0`, a one-palette TIM at `0x07B00` for CLUT byte `0x13`, and the four row-498..501 palette TIMs at `0x10178` / `0x100D0` / `0x10028` / `0x0FF80`). Decoder `legaia_font::escape_icons`.

The port draws them through the shared layout: `Font::with_escape_icons` appends the decoded sprites below the glyph cells, `Font::layout` places a sprite escape at the pen `y_offset` down and advances by the table's width (no inter-glyph spacing, `0x80036A10`), and `text_draws_for` draws it untinted, since `FUN_8002C488` emits it at `0x808080` whatever the pen. Every host font attaches them (the native boot font, the play page, the minigames page), and the dialog panel keeps the escape pair in its page - one typewriter unit - so the reading box draws it too. The numeric escapes still lay out as nothing.

| Index | Sprite id | Size | Advance | `y_offset` | Draws |
|---|---|---|---|---|---|
| `0x00..=0x07` | 55..62 | 16x16 | 16 | -2 | Controller buttons: X, Circle, Square, Triangle, R1, R2, L1, L2 |
| `0x08` | 98 | 12x12 | 12 | +2 | `G` gold badge |
| `0x09` / `0x0A` | 132 / 133 | 12x12 | 12 | 0 | `I` (one target) / `A` (all targets) badges |
| `0x0B..=0x0E` | 0 | - | 8 per digit | 0..3 | Script counter `0..3` as a number |
| `0x0F` | 137 | 38x12 | 38 | 0 | "Ra-Seru" in Japanese kana, a text sprite |
| `0x10..=0x13` | 36, 34, 35, 37 | 12x12 | 12 | 0 | Equip-slot icons: arms (fist), head (helmet), body (armor), legs (boot) |
| `0x14..=0x1A` | 139..145 | 20x12 | 20 | 0 | Element plates: fire, thunder, wind, water, earth, light, dark |
| `0x1B` | 146 | 20x12 | 20 | 0 | Monster plate |
| `0x1C` | 147 | 12x12 | 20 | 0 | A 12 px window at U 0 of the plate row, fire palette: the fire plate's left edge (`0x14` reads from U 6) |
| `0x1D..=0x23` | 148..154 | 28x12 | 28 | 0 | Winged element icons, same order as `0x14..=0x1A` |
| `0x24` | 155 | 28x12 | 28 | 0 | The monster plate read 28 px wide from U 226 (`0x1B` reads 20 px from U 230), palette `0x4F` |
| `0x25` | 156 | 28x12 | 28 | 0 | The fire plate read 28 px wide from U 0, fire palette |

The advance column is the retail table's; the measurer (`legaia_font::measure`) reads it from the same table. The numeric escapes advance `8` px per digit when drawn - see [What a line measures](#what-a-line-measures).

### Symbol names and aliases

The translation pipeline names every entry once, in `legaia_patcher::translation::symbols::SYMBOLS`: a human name for the translation workbench's symbol palette and a readable alias a pack may type in place of the hex token (`{btn:x}` = `{ce:00}`, `{icon:fire}` = `{ce:14}`, `{num:0}` = `{ce:0b}`). The alias encodes to the same two bytes, and export always writes `{ce:NN}`; the alias list is on [the pack-format page](../tooling/translation/pack-format.md#symbols).

The first list of these symbols was contributed by **Henrique Stanke Scandelari (Stann0x, [github.com/Stann0xus](https://github.com/Stann0xus))** while working on a Brazilian Portuguese translation. Checked sprite by sprite against the disc, it agrees on every icon it names; the disc adds three things it does not say:

- `0x0B..=0x0E` are not empty: they print a number (a script counter), which reads as nothing in a text dump because the entry has no sprite.
- `0x1C` is not a broken icon of its own: its record reads 12 px of the element-plate row from U 0, which catches the fire plate's left edge.
- `0x24` and `0x25` exist (the table has 38 entries, not 36): wider reads of the monster and fire plates.

## Rendering pipeline

| Step | Function | Notes |
|---|---|---|
| Source preprocessor | `FUN_80036514` | Expands authoring-time `^X` (0x5E) escapes into runtime `0xCE (X-0x2D)` escape stream. |
| Typewriter glyph count | `FUN_80036044` | Called from `FUN_8003CC98`. Counts the units a typewriter reveal steps through; it neither measures pixels nor wraps. |
| Single-line renderer | `FUN_80036888` | Iterates bytes, dispatches escapes, emits one GP0 0x64 sprite per glyph. |
| Draw and count | `FUN_8003CC98` | `FUN_80036044` + `FUN_80036888`: draws one string, returns its glyph count. |
| Text-actor tick | `FUN_80031D00` | Per-actor text rendering; uses an alternate width-bucketed glyph layout for HUD/status numbers (column-0 stride 8 px, height 12 px) - see `DAT_80073DCC`. |

Per-glyph GP0 packet (variable-size textured rectangle, opaque, with raw-texture color):

```
[0x04 00 00 00]              // OT-list terminator
[0x64 80 80 80]              // cmd 0x64 + RGB shading
[i16 X][i16 Y]               // top-left in screen coords
[u8 U][u8 V][u16 CLUT]       // U,V within texture page; CLUT word
[u16 W=14][u16 H=15]         // sprite size in pixels
```

The texture page is set earlier by a separate GP0 0xE1 (DRAWMODE) primitive - it is **not** embedded in the per-glyph packet.

## Inline control bytes

| Byte | Operand | Meaning |
|---|---|---|
| `0x20` | - | Space. No glyph; advance X like any glyph, `widths[0x20] + DAT_800740E8 + 1`. |
| `0x7C` | - | Newline. Advance Y by 14 px; reset X to line-start. |
| `0xCE` | u8 | Escape - index into the table at `0x80074050`. |
| `0xCF` | u8 | Color change. Sets `DAT_8007B454` (CLUT additive index 0..15). |
| `0x00` | - | String terminator. |
| any other `0x21..=0xFF` | - | Glyph: emit one sprite via the formula above. |

## Provenance

| Subject | Source |
|---|---|
| Width table location + indexing | `ghidra/scripts/funcs/80036888.txt` line 345 (`+ (uint)*(byte *)((int)&DAT_80073f1c + (uint)bVar1)`) |
| Glyph U/V formula | `ghidra/scripts/funcs/80036888.txt` lines 332-335 (`*pbVar4 << 4` for U, `(bVar1 & 0xf0) - 0x20` for V) |
| GP0 packet shape | `ghidra/scripts/funcs/8003c11c.txt` (the simpler text-actor renderer with the same packet layout) |
| Escape table location + entry layout | `ghidra/scripts/funcs/80036888.txt` lines 282-321 |
| Escape sprite draw + number branch | `ghidra/scripts/funcs/80036888.txt` (`0x8003696C..0x800369EC` sprite call, `0x80036A54..0x80036A94` counter read at `0x801C6460`); sprite records + CLUT byte `ghidra/scripts/funcs/8002c488.txt` |
| CLUT base | `ghidra/scripts/funcs/80036888.txt` lines 195-196 (`addiu v1,v1,0x7f86`) |
| Color-change escape | `ghidra/scripts/funcs/80036888.txt` lines 278-280 (case `0xCF`) |
| Author-time `^X` preprocessor | `ghidra/scripts/funcs/80036514.txt` lines 246-249 |
| Draw-and-count wrapper | `ghidra/scripts/funcs/8003cc98.txt` |

This renderer chain draws **field dialogue**, which has no dedicated opcode: a
field NPC's text is its inline interaction-script MES (retail `actor[+0x90]`),
shown by the per-frame actor-dialog SM `FUN_80039b7c` + the dialog pager
`FUN_801D84D0`, triggered by the touch / button-press interaction (no opcode;
op `0x3E` with `op0 < 100` is the scripted-battle install, not a talk) - see [`subsystems/script-vm.md` § Field dialogue](../subsystems/script-vm.md#field-dialogue-has-no-opcode).
(`FUN_8001FD44` is **not** the opener - it is the scene-change packet, reached
by the `0x3F` named scene-change; an earlier note mislabeled it. The
`_DAT_1F800394 |= 0x40` it sets is a scene-transition-pending flag, not a
"dialog active" lock.)

## Line width and wrapping

Byte room is not screen room. A translated line can fit its byte slot and still run past the box it is drawn in, because **no retail text surface wraps**: every line break is an authored `0x7C` or a new `0x1F` dialog line, and an over-long line is drawn in full, over the box frame and off it. `FUN_80036888` has no clip and no length test, and none of its callers measure a line before drawing it.

### What a line measures

A line's width is its pen advance, computed on the **expanded** string. `FUN_80036888` first runs the source through `FUN_80036514` into the buffer at `0x800740EC`, and the walk then sees:

- **Glyph bytes** advance `widths[c] + DAT_800740E8 + 1`, spaces included.
- **Substitution tokens** `0xC1..=0xC5` and `0xC7` are replaced by their text before the walk, so they count at the full width of what they splice in: `0xC1` a party member's display name (record `+0x2A7`; argument `0x63` = the character `DAT_80084597` names), `0xC2` / `0xC4` an item name (`0x8007436C + id*0xC`), `0xC3` a spell name (`0x800754D0 + id*0xC`), `0xC5` an arts name (the `0x80075EC4` table, 20-byte stride, matched on `[character, art]`), `0xC7` one of the 8-byte SCUS names at `0x80073F24`. A `0xC1` inside a spliced string is expanded once more; no other nested token is.
- **`0xC0` and `0xC6`** have no arm in the expander's jump table (`0x80036694` branches them to the copy loop at `0x800367D0`), so they splice in whatever the previous token pointed at. Treat them as undefined.
- **`0xCE` escapes** advance the table's `+2` byte for a string escape. A numeric escape (`string_id == 0`) draws through `FUN_80034B78` and advances `8` px per digit - not the table's `32`, which only the measurer `FUN_80035F04` uses.
- **`0xCF`** (and its author alias `0xFF`) changes ink and adds nothing.

`FUN_80036044` is not a width. It returns the string's **glyph count** - one per glyph, `0x7C` or `0xCE`, zero per `0xCF`, the spliced length per substitution - which is what a typewriter reveal steps through (`FUN_80036888`'s third argument caps the count drawn). Retail's pixel measurer is `FUN_80035F04`: the same expansion into a 256-byte stack buffer, then the widest `0x7C`-separated line. The count is ported as `legaia_font::typewriter_glyph_count`; its consumer, the pager's row gate at `0x801D8A6C`, compares it against the reveal counter `_DAT_801F2748` and holds a short row for `(0x22 - count) * 4` units of `_DAT_801F275C` - see [Typewriter pacing](#typewriter-pacing).

### Typewriter pacing

The pager `FUN_801D84D0` runs once per game tick, every `DAT_1F800393` vsyncs (`2` in field and town scenes), and keeps three words for the row it is typing: the reveal counter `_DAT_801F2748`, the accumulator `_DAT_801F2758` and the short-row hold `_DAT_801F275C`. The draw caps the typed row at the counter (`FUN_80036888`'s third argument), so the counter is how many units of the row show.

- **Opening call.** The call that opens a box, or re-opens one after a page turn, stores counter `1` and accumulator `0` (`0x801D8980..0x801D8990`): one glyph shows at once.
- **Typing call.** Adds `min(DAT_1F800393, 4)` to the accumulator and moves the counter by as many whole speed units as it holds, capped at three when it reaches four (`0x801D8A08..0x801D8A70`). The speed word `_DAT_801F2754` is `1` on every pager open (`0x801D9118`, `0x801D91D0`, `0x801D9268`, `0x801D9CA4`), so the counter gains one unit per vsync - at the field's step, two every other vsync.
- **Row gate.** The same call counts the row with `FUN_80036044` (above) and finishes it once the count is **below** the counter (`slt v1,s0,v1` at `0x801D8A7C`) - one call after the counter reaches the count, not on reaching it. A colour escape costs no unit and each two-byte unit's post-`NUL` overrun costs one. The finish zeroes counter and accumulator, and a row under `0x22` units stores the hold `(0x22 - count) * 4` (`0x801D8A88..0x801D8AA8`).
- **Hold.** A call that finds the hold non-zero runs dispatch case `0x10` instead of typing and drains it by `32 * min(DAT_1F800393, 4)`, clamping at zero (`0x801D866C..0x801D8690`): `ceil((0x22 - count) / (8 * step))` calls, at most three at the field's step. A row that ends its page clears the hold on the next call (state `0x19`, `0x801D865C`), so the last row of a page never holds.

A PCSX-Redux trace of `town01` placement `P1[16]`'s conversation (`scripts/pcsx-redux/autorun_dialog_typewriter_trace.lua`, one CSV row per vsync of those words) shows exactly this: the counter runs `1, 3, 5, ...` on a box's first row and `2, 4, 6, ...` after a finish, a count-25 row finishes on the call whose counter would be 27 and holds `36` for one call, and a count-11 row holds `92`, then `28`, then `0`.

The engine types every pager page this way: `legaia_engine_core::dialog::OwnedDialogPanel` counts each row with `legaia_font::typewriter_glyph_count` and `legaia_engine_core::dialog_pacing` runs the gate, one pager call every `frame_step` ticks, on the path both play hosts drive. `crates/engine-core/tests/dialog_typewriter_pacing_disc.rs` pins the traced row counts and the first box's per-vsync counter and hold against the engine.

The rows the counter types into belong to a scrolling window, not a page buffer: a page turn keeps the previous page's rows and types beneath them, a line after a full window scrolls it a row with no button press, and a confirm press while a row types or holds sets the skip latch `_DAT_801F2750 = 0x25` and completes the page (state `0x0D`, `0x801D89B8..0x801D8A04`; `0x801D86BC` for the hold). The states and the trace that pins them are in [`mes.md` § Row window and scrolling](mes.md#row-window-and-scrolling); the engine's port is `legaia_engine_core::dialog_window`, and both hosts draw its rows through `legaia_engine_ui::dialog_reading_box_text_draws_for`, offset by the scroll and clipped to the box's rows band.

The port is `legaia_font::Font::measure` (`crates/font/src/measure.rs`), which takes the surface's `DAT_800740E8` and a resolver for the runtime substitutions, and reports any token it could not resolve so a width is never silently a lower bound.

### The field dialog box

The pager `FUN_801D84D0` draws each row with `FUN_80036888` at the box's own `x` (`ctx+0x12`) and stores `DAT_800740E8 = 1` before every row (`0x801D96F0`, `0x801D9750`, `0x801D97D8`). A dialogue glyph therefore advances `widths[c] + 2`. The `v0_1_tetsu_dialogue_accept` save state's display list confirms it: the CLUT-7 glyph sprites of the box's first row step exactly `widths[c] + 2` apart.

The box's centre rect is `0xF4` = **244 px** wide (`li a2,0xf4` at `0x801D99CC` into `FUN_8002C69C`), and rows start at its left edge, so a row fits when it measures at most 244 px at pad `1`. The skin draws outside that rect - the fill runs 4 px further and the border 4 px beyond that - so a row of up to 248 px still lands on the fill, but not on clear panel.

A page shows **three rows** (`_DAT_801F2740 = 3`, stored at `0x801D90F0`). Rows are separate `0x1F` lines, pitch `0xF`; a `0x7C` inside a row steps down `0xE` and overlaps the next row, so it is not a way to add one. While a full page waits for confirm, the page-advance hand sits at absolute `x = 0x10A` (`0x801D9834`), `y = box_y + rows*0xF - 0x13`, 16 px square: in the standard box at `x = 0x26` it covers the right end of the page's last two rows from **228 px** in.

Option labels in a picker box draw at `box_x + 0x10` (`0x801D9B6C`), also at pad `1`, so they have **228 px**.

A party name spliced in by `0xC1` is bounded at entry: the name-entry screen `FUN_801F03F0` keeps a typed glyph only while `FUN_80035F04` of the name stays below `0x39` (`0x801F064C..0x801F0654`), so a player-chosen name measures at most **56 px** at pad `0` - up to one more pixel per glyph inside dialogue. The default names are whatever the new-game template and the translation pack carry, and nothing re-checks them.

### Menu, shop and battle columns

The list surfaces draw at pad `0` and stop at the next column, not at a box edge. The pens, all relative to the window's content origin `WX`:

| Surface | Name pen | Next column | Room |
|---|---|---|---|
| Item list (bag row) | `WX+0xC` | count tens cell `WX+0x74` | 104 px |
| Shop buy list | `WX+0x18` | price field `WX+0x80` | 104 px |
| Item info window | `WX` | count `WX+0x7C` | 124 px |
| Status magic page | `WX+0x10` | level `WX+0x78` | 104 px |
| Status moves page | `WX+0x10` | AP field `WX+0x82` | 114 px |

The derivations are in [`field-menu.md`](../subsystems/field-menu.md#name-columns-and-translated-text).

In battle, the message banner and the formation line draw from pen `(16, 12)` in a box **288 px** wide (the `FUN_801D9D3C` immediates that reproduce placement record 67), and the actor-name plaque at `(8, 8)` sizes itself to the measured name, so neither clips; 288 px is the width that keeps a line inside the frame. The battle-intro enemy labels are clamped to `6 <= x <= 0x13A - width`, so one label can reach 308 px, but they share a row and push apart - see [`battle.md`](../subsystems/battle.md#the-battle-intro-enemy-name-banner). Battle text draws at pad `0`: the plaque's measured interior is exactly `widths[c] + 1` per glyph (27 px for a four-letter party name in the captured states).

The pinned budgets are the table `legaia_font::limits::TEXT_LIMITS`, one `TextLimit` per context with its pad and provenance.

### Still open

- The item and spell **description** lines (`FUN_800337B0`, the 27 KB menu-string formatter) have no pinned width; the info window is 144 px wide, which bounds them only by inference.
- Speaker names, the world-map place-name labels, the title and save screens, and the minigame HUDs are not measured here.
- The end of the expansion buffer at `0x800740EC` is not pinned. The zero-initialised region it opens runs `0x206` bytes to the next initialised data at `0x800742F2`, and no dumped routine references an address inside it, which makes 518 bytes an upper bound by inference.

## What's still open

- **`0xCC` opcode.** The text-actor renderer at `FUN_80031D00` recognises a small handful of single-byte ops (`0xCC..=0xCF`) inside its glyph stream that are distinct from the dialog renderer's `0xCE/0xCF`. They're outside the dialog font's scope and tracked under the [field script VM](../subsystems/script-vm.md) docs.

## Extraction tools

`extracted/font/` (gitignored - Sony pixel data) is produced by the font-extraction step:

| File | What it is |
|---|---|
| `dialog_font_sheet.png` | The full 256×256 source-pixel font tile-page, 4bpp expanded with CLUT 0 |
| `dialog_font_atlas.png` | Per-glyph atlas, 14×15 cells laid out in 16 columns × 14 rows (224 glyphs total) |
| `dialog_font_metadata.json` | Width table + escape table + VRAM source rect, in machine-readable form |
| `dialog_font_widths.csv` | Just the width table as CSV |
| `dialog_font_vram_4bpp.bin` | Raw 32 KB 4bpp VRAM bytes (downstream tooling can hash + search PROT for the carrier) |

The extractor reads the SCUS executable for the static tables, and a mednafen save state's `&GPURAM[0][0]` section for the live VRAM bytes. The font region is byte-stable across all captured save states (with cosmetic differences only in cells touched by transient UI elements that share the tile-page).

The committed extractor is `crates/font/src/bin/font-extract.rs`:

```
cargo run -p legaia-font --bin font-extract -- \
    --scus extracted/SCUS_942.54 \
    --save "$HOME/.mednafen/mcs/Legend of Legaia (USA).<hash>.mcN" \
    --out extracted/font
```

Save-state parsing locates VRAM by searching for the `&GPURAM[0][0]` variable
header (mednafen uses a `u8 name_len; bytes name; u32 size; bytes data;`
record format inside each section); no MDFNSVST section walk is required.

## Accented Latin cells

The glyph page indexes every byte `0x20..=0xFF`, so the cells above `0x7E` are ordinary glyph cells. What they hold differs per build, and whether a cell *draws* depends on two things at once: ink in the cell, and a non-zero entry in the width table.

### The retail USA page

The USA font TIM (`PROT.DAT` `0x7F40`) has ink in 32 of the 128 high cells. They are accented Latin letters in the IBM CP437 positions for `0x80..=0x90` and `0x95..=0x9A` (`Ç ü é â ä à å ç ê ë è ï î ì Ä Å É ò û ù ÿ Ö Ü`), plus `œ` at `0x9C`, `Ÿ` at `0x9F`, and a run one cell below CP437 at `0xA0..=0xAC` (`í ó ú ñ Ñ` at `0xA0..=0xA4`, `¿` at `0xA7`, `¡` at `0xAC`). The width table gives 26 of the 32 a zero advance, so such a glyph draws and the next letter lands one pixel to its right, on top of it. The width table's `0xC0..=0xFF` entries follow an ISO 8859-1 shape (`À..Å` 8, `Æ` 13, `Ì..Ï` 4, `à..å` 7) over cells that carry no ink at all.

Retail text hardly reaches either half. In the USA text export, high cell bytes occur only in spell-table entries past the named spells (`0x81`, `0xA8..=0xAB`) and in three scene-dialog lines; and `0xC0..=0xCF` are two-byte dialog opcodes, never glyphs. The page is the same byte-for-byte in VRAM `(896, 0)` across five captured mednafen states from different phases, and the in-RAM width table matches `SCUS_942.54`, so what the disc carries is what draws. The counts are pinned by `crates/patcher/tests/translation_accent_font_real.rs`.

### The PAL page

The official French, German and Italian discs carry the font TIM at the same `PROT.DAT` offset (member 3 of the same boot pack), but it is a different, smaller drawing of every glyph, ASCII included; the three PAL pages differ from each other only in `0x24`. Their high cells follow CP437 for `0x80..=0xA5`, add `ª º ¿ ¡` at `0xA6 0xA7 0xA8 0xAD`, and draw accented capitals and `ß` at `0xB5..=0xB7` (`Á Â À`) and in `0xD3..=0xEB`, mostly at their CP850 positions (`Ë È Í Ì Ó ß Ô Ò Ú Û Ù`); `Ê`, `Î` and `Ï` sit at `0xD5`, `0xDD` and `0xDF` instead.
PAL text writes `Î` as `0xD7` and `°` as `0xF8`, both cells the PAL page leaves as a placeholder box labelled with the cell's own hex, and retail PAL draws that box: no PAL executable remaps either byte ([below](#the-pal-renderer)). The page draws `œ` at `0x9C` and `Œ` at `0x9D`.

### The PAL renderer

The four PAL executables (`SCES_019.44` French, `.45` German, `.46` Italian, `.47` Spanish) share one text renderer, the sibling of `FUN_80036888`: the same body in each, differing only in the data addresses it forms (French entry `0x80037088`, German / Italian / Spanish glyph emits at `0x80037688` / `0x800378BC` / `0x800378EC`). Every dialog, menu and crawl string reaches it - the French field overlay (PROT 0897) calls it from 41 sites, the menu overlay (0899) from 151 - and it addresses each glyph cell straight from the byte, `u = (b & 0xF) << 4`, `v = (b & 0xF0) - 0x20` (French `0x800373B4..0x800373E0`), after one pass through the string preprocessor (French `0x80036BB4`, the `FUN_80036514` sibling) into the line buffer `0x800749CC`.

Nothing on that path touches `0xD7` or `0xF8`. The preprocessor rewrites `^x` to a `0xCE` escape and `0xFF` to `0xCF`, and expands the `0xC1..=0xC7` substitutions, exactly as the USA one does; every `0xD7` / `0xF8` immediate in each of the four PAL executables sits at the same instruction shape as in `SCUS_942.54` (the same sequence of opcode, register and value), the French copies of the field, battle and menu overlays (PROT 0897..0899) add none, and none of those images carries a byte-translation table (no 128-byte window over `0x80..=0xFF` that maps most bytes to themselves and one to another). The width table (French `0x80074718`) gives `0xD7` an I's width of 3 and `0xF8` a width of 5, so the box advances 4 or 6 pixels and the next letter overprints its right edge.
A cold-boot capture of the French disc confirms it: `autorun_pal_glyph_cells.lua` rewrites the first three letters of every string the renderer draws to `0xD7`, `0xF8` and `0xDD`, and the prologue crawl shows the `D7` and `F8` boxes beside a correct `Î` from `0xDD`.

Two differences from the USA path live in the same code:

- **The French ligature fold.** Only the French preprocessor folds the two-letter sequences `oe` to `0x9C` (`œ`) and `OE` to `0x9D` (`Œ`) before drawing (`0x80036D14..0x80036D5C`, and again inside substituted names). The French script spells `coeur` with both letters (no lifted French line carries `0x9C` or `0x9D`), so the ligature exists only on screen; the fold is unconditional, so it fires inside any word with that letter pair.
- **An x-tab escape.** In all four PAL renderers `0xC1 n` with `n >= 8` sets the pen to the line's start x plus `n` (French `0x80037138..0x8003714C`); the French and Italian preprocessors pass such a pair through unexpanded (French `0x80036DD0..0x80036DF8`, Italian `0x80037374..0x8003739C`), where `n < 8` and `n = 0x63` still select a name.

### The layout

`legaia_font::latin::LATIN_CELLS` is the one byte-to-character table the tools share: the importer's accent fold, the lift's `--fold-accents`, the accent font below, and the translation workbench's palette and fixes all read it. It follows the byte values the PAL discs write - CP437 for the lowercase block and the capitals CP437 has, CP850 for the rest - so text lifted from a PAL disc and text typed by hand land on the same cells. Three cells are the tools' own choice, because the CP850 cell for `ã` / `Ã` sits in the opcode window and CP850 has no `Œ`: `ã` is `0x9B`, `Ã` is `0xD0`, `Œ` is `0x9E`.
`œ` and `Ÿ` stay where the USA page already draws them. The PAL page's own `Œ` is at `0x9D`, but no PAL text writes that byte - only the French ligature fold produces it, at draw time - so the layout's `0x9E` loses nothing a lift carries; a lifted French line keeps `oe` as two letters.

## The accent font

The accent font is a patch-time rebuild of every layout cell on the user's own disc, so accented text draws. For each cell with a recipe it copies the base letter out of the same page (`e` for `é`, `A` for `Á`, a dotless `i` for `í`), paints a small diacritic mask above it (below it, for the cedilla) and sets the cell's width-table entry to the base letter's own advance. A capital shifts down as far as the mark needs, the way the retail `É` and `Ä` cells do. Ligatures (`æ Æ œ Œ`) join two base letters with a one-column overlap, `¿ ¡` are `?` and `!` turned half a turn onto the descender line, and `ß` and `°` are drawn from masks.

Every mark pixel is drawn in the page's two ink indices, `15` fill and `14` shadow, with the shadow as the fill dilated one pixel right, down and diagonally - the rule every retail glyph but three follows. The result is a function of the input page alone, so no glyph bytes are committed: the patch ships recipes and masks, and the pixels come from the disc being patched.

It is written as two same-size in-place edits: the rebuilt image rows of the font TIM in `PROT.DAT`, and the 256-byte width table in `SCUS_942.54`. ASCII cells and every other width entry are untouched. A disc carries it when rebuilding the font from its own page reproduces every layout cell and width, which is how an import finds it already present (`legaia_font::accent_font::accent_font_state`).

What it does not reach: Latin letters with no cell in the layout (Polish `ł ą ę`, Czech `č ř`) fold to ASCII, and Cyrillic, Greek and CJK need more cells than the page has - a second glyph bank and a multi-byte encoding in the renderer.

How a pack asks for it, and how the tools report characters that will not draw, is in [`pack-format.md`](../tooling/translation/pack-format.md#accents).

## See also

- [MES dialog](mes.md) - the dialog containers this font renders.
- [Translation / language packs](../tooling/translation/index.md) - the pack
  pipeline that writes the accent font.
- [`subsystems/renderer.md`](../subsystems/renderer.md) - the renderer that blits the glyph atlas.
