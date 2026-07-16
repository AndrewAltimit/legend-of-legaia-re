# legaia-tim

PSX TIM (texture image) parser and PNG exporter, plus a software model of
the PSX VRAM frame the renderer reads from.

TIM is Sony's PSX texture format. It's not Legaia-specific - this crate
follows the canonical PsyQ docs.

## File layout

```text
magic   u32  0x00000010
flags   u32  bit0..2 = pmode (0=4bpp, 1=8bpp, 2=16bpp, 3=24bpp)
             bit3    = has CLUT

[clut block, if bit3 set]
  bs_len  u32   block length, including itself
  fb_x    u16   framebuffer X (CLUT load position)
  fb_y    u16   framebuffer Y
  w       u16   CLUT width  in 16-bit entries
  h       u16   CLUT height in rows
  data    w*h*2 bytes (rows of 16-bit BGR555 + STP)

image block:
  bs_len  u32
  fb_x    u16   framebuffer X (image load position, in 16-bit words)
  fb_y    u16
  w       u16   image width in 16-bit words (NOT pixels for 4/8 bpp)
  h       u16   image height in rows
  data    w*h*2 bytes (raw pixel data)
```

Pixel widths in real pixels:

- 4bpp:  `fb_w * 4`
- 8bpp:  `fb_w * 2`
- 16bpp: `fb_w`
- 24bpp: `fb_w * 2 / 3` (24-bit packed; 3 bytes per pixel)

16-bit pixels are stored `STP|B|G|R` (1+5+5+5 bits, little-endian).

## What it provides

- `parse(bytes) -> Tim` - lenient header + CLUT + image-block parser
  (tolerates trailing block padding; for callers decoding known-TIM bytes).
- `parse_strict(bytes) -> Tim` - reproduces an external reference decoder's
  TIM detector: no reserved flag bits, pmode 0..=3, exact block lengths,
  nonzero dims, in-VRAM-bounds image rectangle (CLUT intentionally not
  bounds-checked - NPC palettes sit at the framebuffer's bottom edge). Used to
  build the flat `PROT.DAT` TIM catalog in `legaia-asset::tim_catalog`.
- `Tim::byte_extent` / `Tim::palette_count` - on-disc footprint + CLUT count.
- `Tim::to_rgba8` - palette-resolve to RGBA8 for PNG.
- `vram::Vram` - software model of PSX VRAM (1024×512 R16 framebuffer).
  Used by `legaia-engine-render` for per-primitive texture-page +
  CLUT-row decode in the fragment shader.

## CLI

```bash
# Header dump: pmode, CLUT block, image dims, framebuffer position
tim info extracted/tim_scan/0005_town01/000.tim

# Single TIM → PNG. Without -o, writes `<path>.png` next to the input.
tim convert extracted/tim_scan/0005_town01/000.tim -o town01_000.png

# Pick a palette row on a CLUT-bearing TIM (default row 0), or emit every row
# as `<path>_clut<N>.png` - the fast way to find which CLUT a mesh expects.
tim convert extracted/tim_scan/0005_town01/000.tim --clut 3 -o row3.png
tim convert extracted/tim_scan/0005_town01/000.tim --all-cluts

# Batch: recursively convert every .tim under a directory.
# Without -o, mirrors the input layout next to it.
tim convert-dir extracted/tim_scan -o png_out
```

## See also

- [`docs/formats/tim.md`](../../docs/formats/tim.md)
- [`docs/subsystems/renderer.md`](../../docs/subsystems/renderer.md) -
  how the engine uploads TIMs into the VRAM model.
