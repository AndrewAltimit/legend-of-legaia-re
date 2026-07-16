# `legaia-font`

Loader and layout helper for the proportional dialog font.

The font itself is documented in [`docs/formats/dialog-font.md`](../../docs/formats/dialog-font.md). This crate consumes the artifacts produced by `font-extract` (below) at `extracted/font/`:

- `dialog_font_atlas.png` - 224×210 RGBA atlas (16 cols × 14 rows of 14×15-pixel glyph cells, packed without inter-cell padding). Glyph cell `c` (for `c in 0x20..=0xFF`) lives at column `c & 0x0F`, row `(c - 0x20) >> 4`.
- `dialog_font_widths.csv` - per-character pixel advance.
- `dialog_font_metadata.json` - VRAM provenance + escape-sequence table (informational).

`font-extract` writes two further files that the loader doesn't read:
`dialog_font_sheet.png` (the raw 256×256 tile page as it sits in VRAM, before
atlas packing) and `dialog_font_vram_4bpp.bin` (the literal packed 4bpp pixels,
for tooling that hunts the on-disc carrier of the font).

## API surface

```rust
use legaia_font::Font;

let font = Font::load_from_extracted("extracted")?;
let layout = font.layout("Hello world");
for g in &layout.glyphs {
    // g.dst_x, g.dst_y, g.width, g.height, g.atlas_x, g.atlas_y
}
let total_width = layout.advance_x;
```

The crate does **not** depend on a renderer - it only produces glyph rectangles in atlas coordinates and screen-relative offsets. Renderer integration lives in `legaia-engine-render`.

### Disc-only construction (no save state)

`Font::from_disc_tim_and_scus(font_tim, scus)` builds the real proportional font straight from a disc: the glyph bitmaps come from the on-disc font TIM (`PROT.DAT` at `FONT_TIM_PROT_DAT_OFFSET` = `0x7F40`, a 4bpp 256×256 page at framebuffer `(896, 0)`), the advances from the SCUS width table. It yields the byte-identical whitewashed atlas `load_from_extracted` produces, so a disc-only consumer - the WASM site's pause menu - renders text exactly like native **without** running `font-extract` or shipping a save state. See [`docs/formats/dialog-font.md`](../../docs/formats/dialog-font.md#on-disc-carrier).

## `font-extract` binary

The crate ships a `font-extract` binary that writes the `extracted/font/` artifacts from a disc-extracted `SCUS_942.54` plus a mednafen save state with the dialog font live in VRAM. Only `--save` is required; `--scus` defaults to `extracted/SCUS_942.54` and `--out` to `extracted/font`:

```bash
# Defaults are the extraction-pipeline layout, so this is usually enough
font-extract --save "$HOME/.mednafen/mcs/Legend of Legaia (USA).<hash>.mc0"

# Explicit form
font-extract \
    --scus extracted/SCUS_942.54 \
    --save "$HOME/.mednafen/mcs/Legend of Legaia (USA).<hash>.mc0" \
    --out extracted/font
```

If you only need the font for rendering and would rather not deal with a save
state at all, use `Font::from_disc_tim_and_scus` instead - it builds a
byte-identical atlas straight from the disc (see below).

Pipeline:

1. Parse the PSX-EXE header on `SCUS_942.54` for its t_addr; read the 256-byte width table at RAM `0x80073F1C` and the 38×4-byte escape table at `0x80074050`.
2. Open the mednafen save state, gunzip if needed, validate the `MDFNSVST` magic, find the `&GPURAM[0][0]` variable inside the `GPU` section, and slice out the 1 MB VRAM payload.
3. Read the dialog CLUT at VRAM (96, 510) and the 4bpp font tile-page at VRAM (896, 0)..(960, 256).
4. Decode 4bpp + CLUT to RGBA8 and write the four artifacts.

Any in-game save state works; the font tile-page is byte-identical across captures.

## Clean-room status

Atlas pixels and widths are derived from a Sony executable + VRAM dump and are tracked in the gitignored `extracted/` tree, never checked in. This crate ships as code only; loading the runtime artifacts is a deployment concern.
