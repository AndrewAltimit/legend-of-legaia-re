# `legaia-font`

Loader and layout helper for the proportional dialog font.

The font itself is documented in [`docs/formats/dialog-font.md`](../../docs/formats/dialog-font.md). This crate consumes the artifacts produced by the extraction pipeline at `extracted/font/`:

- `dialog_font_atlas.png` — 224×210 RGBA atlas (16 cols × 14 rows of 14×15-pixel glyph cells, packed without inter-cell padding). Glyph cell `c` (for `c in 0x20..=0xFF`) lives at column `c & 0x0F`, row `(c - 0x20) >> 4`.
- `dialog_font_widths.csv` — per-character pixel advance.
- `dialog_font_metadata.json` — VRAM provenance + escape-sequence table (informational).

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

The crate does **not** depend on a renderer — it only produces glyph rectangles in atlas coordinates and screen-relative offsets. Renderer integration lives in `legaia-engine-render`.

## Clean-room status

Atlas pixels and widths are derived from a Sony executable + VRAM dump and are tracked in the gitignored `extracted/` tree, never checked in. This crate ships as code only; loading the runtime artifacts is a deployment concern.
