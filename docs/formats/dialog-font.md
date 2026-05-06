# Dialog font (proportional Latin)

The proportional sans-serif font used by the dialog box, the field menu, and most in-game UI text. It lives in VRAM at runtime and is referenced by every text-rendering primitive the engine emits.

The font has three pieces of static data, all in `SCUS_942.54`:

1. A **256-byte width table** at `0x80073F1C`, indexed by character byte.
2. A **38-entry escape-sequence table** at `0x80074050`, indexed by the byte that follows a `0xCE` runtime escape.
3. The **glyph bitmaps**, which sit in VRAM at `(896, 0)..(960, 256)` (a 4bpp tile-page covering 256×256 source pixels). They're loaded from disc into VRAM by an overlay-resident routine; the on-disc PROT entry that carries them has not yet been classified by the static categorizer.

## Glyph layout in VRAM

Format: 4bpp indexed, 16-pixel × 16-pixel cells, 16 columns × 14 rows = 224 cells. Cell `c` (for character byte `c` in the range `0x20..=0xFF`) lives at:

```
U = (c & 0x0F) * 16
V = (c & 0xF0) - 0x20
```

Drawn region within each cell is 14 pixels wide × 15 pixels tall (`W=0x0E`, `H=0x0F` in the GP0 0x64 packet). The remaining 2 pixels of width and 1 pixel of height per cell are inter-glyph guard space.

Character codes `0x00..=0x1F` are reserved for control / escape bytes (`0x7C` newline, `0xCE` escape prefix, `0xCF` color change, `0x20` space) — they do not have glyphs.

## Width table (advance lookup)

```
0x80073F1C  u8 widths[256]
```

256 bytes, indexed by character byte. The advance for character `c` is computed as:

```
advance = widths[c] + DAT_800740E8 + 1
```

where `DAT_800740E8` is a per-string padding override that's normally zero (and is reset to zero at the end of each render call). The trailing `+1` is a fixed inter-character gap.

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
VRAM (96, 510)   // CLUT 0 — dialog grayscale (white-on-transparent text)
VRAM (96 + 16*i, 510)   // CLUT i — colored variants for status text, system prompts
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
| `0x0B..=0x0E` | 0 | 32 | 0..3 | **Variable substitution** — `y_offset` is the variable index (HP/MP/gold/exp slot), renderer calls `FUN_80034B78` to format the integer |
| `0x0F` | 137 | 38 | 0 | String 137 (longest single-shot escape — ~6 chars wide) |
| `0x10..=0x13` | 36,34,35,37 | 12 | 0 | **Active actor name** — string IDs 34/35/36/37 align with the in-SCUS actor name strings ("Meta"/"Terra"/"Ozma"/...) |
| `0x14..=0x1C` | 139..147 | 20 | 0 | Strings 139..147 |
| `0x1D..=0x25` | 148..156 | 28 | 0 | Strings 148..156 |

When `string_id != 0`, the renderer calls `FUN_8002C488(x, y + y_offset, string_id)` to draw the looked-up string. When `string_id == 0`, `y_offset < 4` selects which scratch variable (the four runtime-tracked numbers) and the renderer calls `FUN_80034B78` to format and draw it.

## Rendering pipeline

| Step | Function | Notes |
|---|---|---|
| Source preprocessor | `FUN_80036514` | Expands authoring-time `^X` (0x5E) escapes into runtime `0xCE (X-0x2D)` escape stream. |
| Word-wrap pre-pass | `FUN_80036044` | Called from `FUN_8003CC98`. Wraps lines to fit the dialog box width. |
| Single-line renderer | `FUN_80036888` | Iterates bytes, dispatches escapes, emits one GP0 0x64 sprite per glyph. |
| Multi-line wrapper | `FUN_8003CC98` | `FUN_80036044` + `FUN_80036888`. Used by the field VM dialog opener. |
| Text-actor tick | `FUN_80031D00` | Per-actor text rendering; uses an alternate width-bucketed glyph layout for HUD/status numbers (column-0 stride 8 px, height 12 px) — see `DAT_80073DCC`. |

Per-glyph GP0 packet (variable-size textured rectangle, opaque, with raw-texture color):

```
[0x04 00 00 00]              // OT-list terminator
[0x64 80 80 80]              // cmd 0x64 + RGB shading
[i16 X][i16 Y]               // top-left in screen coords
[u8 U][u8 V][u16 CLUT]       // U,V within texture page; CLUT word
[u16 W=14][u16 H=15]         // sprite size in pixels
```

The texture page is set earlier by a separate GP0 0xE1 (DRAWMODE) primitive — it is **not** embedded in the per-glyph packet.

## Inline control bytes

| Byte | Operand | Meaning |
|---|---|---|
| `0x20` | — | Space. No glyph; advance X by `widths[0x20]` (=4). |
| `0x7C` | — | Newline. Advance Y by 14 px; reset X to line-start. |
| `0xCE` | u8 | Escape — index into the table at `0x80074050`. |
| `0xCF` | u8 | Color change. Sets `DAT_8007B454` (CLUT additive index 0..15). |
| `0x00` | — | String terminator. |
| any other `0x21..=0xFF` | — | Glyph: emit one sprite via the formula above. |

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
| Multi-line wrapper | `ghidra/scripts/funcs/8003cc98.txt` |

The dialog opener that reaches this renderer chain from the [field script VM](../subsystems/script-vm.md) opcode `0x3F` is `FUN_8001FD44` — it sets the "dialog active" story flag (`_DAT_1F800394 |= 0x40`) before forwarding into the renderer.

## What's still open

- **On-disc carrier of the glyph bitmap.** The static categorizer in `crates/asset` doesn't yet recognise the PROT entry that carries the font — the bitmap is reachable only from a save-state VRAM dump. Two unblock paths:
  1. Trace the `LoadImage` (GP0 0xA0) DMA call that uploads the tile-page at `(896, 0)` and identify which PROT entry it pulls from. The `find_lui_writers.py` Ghidra script can locate the LUI+ADDIU pair that loads the source pointer; the destination is the GPU FIFO at `0x1F801810` so the search target is "writes to a struct that ultimately reaches `_DAT_1F801810`".
  2. Diff a save state captured before the title screen finishes booting against one captured during a dialog — the font region transitions from zero to populated, so the disc read that fills it sits in the boot sequence somewhere between `FUN_8003E4E8` (PROT TOC loader) and the first dialog open.
- **String IDs in the escape table.** Entries `0x00..=0x07` (advance 16, `y_offset = -2`) likely render multi-character icon strings from the same string pool that backs `FUN_8002C488`. The pool itself isn't yet decoded — its index 34..37 entries match the SCUS-resident actor name strings, suggesting the pool's first ~150 entries are mostly UI strings + actor names.
- **`0xCC` opcode.** The text-actor renderer at `FUN_80031D00` recognises a small handful of single-byte ops (`0xCC..=0xCF`) inside its glyph stream that are distinct from the dialog renderer's `0xCE/0xCF`. They're outside the dialog font's scope and tracked under the [field script VM](../subsystems/script-vm.md) docs.

## Extraction tools

`extracted/font/` (gitignored — Sony pixel data) is produced by the font-extraction step:

| File | What it is |
|---|---|
| `dialog_font_sheet.png` | The full 256×256 source-pixel font tile-page, 4bpp expanded with CLUT 0 |
| `dialog_font_atlas.png` | Per-glyph atlas, 14×15 cells laid out in 16 columns × 14 rows (224 glyphs total) |
| `dialog_font_metadata.json` | Width table + escape table + VRAM source rect, in machine-readable form |
| `dialog_font_widths.csv` | Just the width table as CSV |

The extractor reads the SCUS executable for the static tables, and a mednafen save state's `&GPURAM[0][0]` section for the live VRAM bytes. The font region is byte-stable across all captured save states (with cosmetic differences only in cells touched by transient UI elements that share the tile-page).

The committed extractor is `crates/font/src/bin/font-extract.rs`:

```
cargo run -p legaia-font --bin font-extract -- \
    --scus extracted/SCUS_942.54 \
    --save "$HOME/.mednafen/mcs/Legend of Legaia (USA).<hash>.mc4" \
    --out extracted/font
```

Save-state parsing locates VRAM by searching for the `&GPURAM[0][0]` variable
header (mednafen uses a `u8 name_len; bytes name; u32 size; bytes data;`
record format inside each section); no MDFNSVST section walk is required.
