# Dialog font (proportional Latin)

The proportional sans-serif font used by the dialog box, the field menu, and most in-game UI text. It lives in VRAM at runtime and is referenced by every text-rendering primitive the engine emits.

The font has three pieces of static data, all in `SCUS_942.54`:

1. A **256-byte width table** at `0x80073F1C`, indexed by character byte.
2. A **38-entry escape-sequence table** at `0x80074050`, indexed by the byte that follows a `0xCE` runtime escape.
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

where `DAT_800740E8` is a per-call padding override, reset to zero at the end of each render call. Menus and battle draw with it at zero; the field dialog pager `FUN_801D84D0` stores `1` before every row it draws, so dialogue runs one pixel wider per glyph than the same text in a menu (see [Line width and wrapping](#line-width-and-wrapping)). The trailing `+1` is a fixed inter-character gap.

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
    i16  string_id;   // 0 = render runtime variable; nonzero = look up a string
    u8   advance_px;  // pixel advance after rendering this escape
    i8   y_offset;    // Y offset (or variable index when string_id == 0)
};
```

There are 38 entries (table indices `0x00..=0x25`).

| Index | `string_id` | `advance` | `y_offset` | Meaning |
|---|---|---|---|---|
| `0x00..=0x07` | 55..62 | 16 | -2 | Icon strings (likely controller-button glyphs / currency icon) |
| `0x08` | 98 | 12 | +2 | String 98 |
| `0x09..=0x0A` | 132,133 | 12 | 0 | Strings 132/133 |
| `0x0B..=0x0E` | 0 | 32 | 0..3 | **Variable substitution** - `y_offset` is the variable index (HP/MP/gold/exp slot), renderer calls `FUN_80034B78` to format the integer |
| `0x0F` | 137 | 38 | 0 | String 137 (longest single-shot escape - ~6 chars wide) |
| `0x10..=0x13` | 36,34,35,37 | 12 | 0 | **Active actor name** - string IDs 34/35/36/37 align with the in-SCUS actor name strings ("Meta"/"Terra"/"Ozma"/...) |
| `0x14..=0x1C` | 139..147 | 20 | 0 | Strings 139..147 |
| `0x1D..=0x25` | 148..156 | 28 | 0 | Strings 148..156 |

When `string_id != 0`, the renderer calls `FUN_8002C488(x, y + y_offset, string_id)` to draw the looked-up string. When `string_id == 0`, `y_offset < 4` selects which scratch variable (the four runtime-tracked numbers) and the renderer calls `FUN_80034B78` to format and draw it.

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

`FUN_80036044` is not a width. It returns the string's **glyph count** - one per glyph, `0x7C` or `0xCE`, zero per `0xCF`, the spliced length per substitution - which is what a typewriter reveal steps through (`FUN_80036888`'s third argument caps the count drawn). Retail's pixel measurer is `FUN_80035F04`: the same expansion into a 256-byte stack buffer, then the widest `0x7C`-separated line. The count is ported as `legaia_font::typewriter_glyph_count`; its consumer, the pager's row gate at `0x801D8A6C`, compares it against the reveal counter `_DAT_801F2748` and holds a short row for `(0x22 - count) * 4` units of `_DAT_801F275C`.

### Typewriter pacing

The pager's reveal rate is fixed at one unit per frame. Every pager open stores the speed word `_DAT_801F2754 = 1` (`0x801D9118`, `0x801D91D0`, `0x801D9268`, `0x801D9CA4`), and each call adds `DAT_1F800393` (capped at `4`) to the accumulator `_DAT_801F2758` and moves the reveal counter by as many whole speed units as that holds, at most three (`0x801D8A08..0x801D8A70`).

A row ends when the counter reaches the glyph count above, not on the terminator byte, so a colour escape costs no frame. A row under `0x22` units then holds `(0x22 - count) * 4` in `_DAT_801F275C`, which each call drains by `32 * DAT_1F800393` (`0x801D866C..0x801D8690`) - `ceil((0x22 - count) / 8)` frames, never more than five - before the pager moves on. The engine's panel types one glyph per tick, which is the same rate; it does not yet end a row on the count or run the short-row hold (see `legaia_font::typewriter_glyph_count`).

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

- **String IDs in the escape table.** Entries `0x00..=0x07` (advance 16, `y_offset = -2`) likely render multi-character icon strings from the same string pool that backs `FUN_8002C488`. The pool itself isn't yet decoded - its index 34..37 entries match the SCUS-resident actor name strings, suggesting the pool's first ~150 entries are mostly UI strings + actor names.
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

## Accented / non-Latin glyphs (font-patch feasibility)

The [translation pipeline](../tooling/translation/pack-format.md#text-markup-and-encoding) writes only bytes the
retail font can already draw - printable ASCII `0x20..=0x7E` - so the shipped
Spanish/French/German/Italian/Polish packs are **ASCII-folded** (`e` for `é`,
`ss` for `ß`, `l` for `ł`). Adding real accented or non-Latin glyphs is a
**font patch**, not a translation-pack change; this section scopes what it takes.

The glyph atlas is a fixed-size grid: a 16×14 cell layout over source bytes
`0x20..=0xFF` (224 cells), 4bpp, uploaded to VRAM at `(896,0)`. Cells are indexed
directly by byte (the `U/V` formula above), and the per-byte advance comes from
the 256-entry width table at `0x80073F1C`. So a new glyph needs three things:

1. **A free byte slot.** A candidate byte must be renderable as a single glyph:
   *not* a 2-byte opcode (`0x5E`, `0xC0..=0xCF`, `0xFF` - the substitution /
   spacing / color escapes), *not* a terminator (`0x00..=0x1F`), and not already
   used by a string. Sweeping the exported corpus, **~106 single-byte slots at
   `0x80..=0xFF` are unused by any string, ~50 of them currently zero-width
   (blank atlas cells)** - comfortably enough for the accented Latin a
   Spanish/French/German/Italian/Polish set needs (roughly `á à â ä ç é è ê ë í
   î ï ñ ó ô ö ù û ü ß` and the Polish `ą ć ę ł ń ó ś ż ź`, ~35 code points).
2. **A glyph bitmap in that cell.** The 32 KB 4bpp tile-page would gain a 14×15
   drawing in each chosen cell. The on-disc carrier of the tile-page is still
   unclassified (see below), so a patch would instead overwrite the cell **in
   VRAM at upload time** or patch whatever routine does the `LoadImage`.
3. **A width-table entry.** Set `widths[byte]` for each new glyph so the
   proportional layout advances correctly - a same-size in-place byte poke into
   `SCUS_942.54`, exactly the mechanism the translation importer already uses.

What that unblocks and what it doesn't:

- **Accented Latin (es/fr/de/it/pl) is tractable.** It fits the free single-byte
  slots, needs ~35 new cells, and the pack side is a trivial change - drop the
  ASCII-fold and emit the chosen bytes (the markup codec already round-trips any
  byte via `{xx}`). The blocker is purely the glyph bitmaps + the width pokes.
- **Cyrillic (ru) is tractable but larger** (~66 cells for upper+lower) - still
  inside the ~106 free slots, same mechanism.
- **CJK (ja/zh/ko) is *not* reachable this way.** Thousands of glyphs blow past
  the 224-cell single-page atlas and the byte index space; it needs a second
  variable-width glyph bank and a multi-byte encoding in the renderer - a
  substantially bigger engine change, out of scope for a byte-poke font patch.

The one genuinely-missing piece for even the tractable cases is **the on-disc
font-bitmap carrier** (below): until that PROT entry is identified, new glyph
bitmaps can only be injected at runtime (a VRAM overwrite after the font upload),
not baked into the disc image the way the width table and the text are. Pinning
the carrier turns the accented-Latin font patch into a fully static, same-size
disc edit.

## See also

- [MES dialog](mes.md) - the dialog containers this font renders.
- [Translation / language packs](../tooling/translation/index.md) - the ASCII-folded
  packs this feasibility note is the unblock for.
- [`subsystems/renderer.md`](../subsystems/renderer.md) - the renderer that blits the glyph atlas.
