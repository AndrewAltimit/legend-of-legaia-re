# Standalone TIM-pack format

A multi-TIM container used by certain standalone PROT entries. Distinct from the [pack format](pack.md) used inside streaming chunks.

Implementation: `crates/prot/src/timpack.rs`.

## Header (8 bytes)

```
u8  magic_lo            // arbitrary
u8  magic_hi            // arbitrary
u8  disc               // < 0x10  (discriminator byte, NOT the count)
u8  marker              // == 0x01
u32 tim_num             // entry count at +4; offset table follows at +8
```

The `byte[3] == 0x01` / `byte[2] < 0x10` pair is the magic discriminator; the entry count is the `u32 tim_num` at `+4` (so the offset table begins at byte `+8`). The detection function `is_tim_pack` checks the signature pair, that `tim_num` is positive, and that the offset table fits within the blob.

## Offset table

Each table entry is a `u32` word index, decoded as:

```
byte_offset = word_index * 4 + 4
```

The `+4` is the difference from the [pack format](pack.md): this format adds a constant offset, suggesting the offsets are relative to the END of the count word rather than the start of the pack.

## Item type detection

`detected_ext(item)` returns `"TIM"` if the first byte is `0x10` (PSX TIM magic), else `"BIN"`.

## Boot-resident system-UI instance (raw TOC entries 0 and 1)

The two entries in `PROT.DAT`'s TOC head - **raw TOC entries 0 and 1**, the region the
extraction index space skips (extraction entry `p` = raw entry `p + 2`; see
[prot.md](prot.md)) - are both TIM-packs. Together they are the **boot-resident
system-UI bundle**: raw entry 0 (sectors 3..55) declares 20 members, raw entry 1 one.
Parser: `legaia_asset::system_ui_bundle`; disc pins in `crates/asset/tests/system_ui_bundle_real.rs`.

Raw entry 0's members, in table order: the boot cursor (`PROT.DAT[0x01858]`), the
system-UI sprite sheet (`0x018E0`, image `(896,256)` 64×192), UI elements
(`0x07B00`, `0x07F40`), four small `(896, 498..=501)`-CLUT TIMs, the **menu-glyph /
interior-page atlas** (`0x11218`, image `(960,256)` 64×256, declared CLUT
`(0,510,16,16)`; extractors `legaia_asset::interior_page` / `menu_glyph_atlas`), the UI
sprite strip (`0x19438`, image `(960,400)` 60×24), **six bare row-patch members**
(`0x1A018..0x1AA7C`; see below), and four cursor-part TIMs (`0x1AC90..0x1AED0`, images
at `(976,256..)`). Raw entry 1's single TIM is a UI page at image `(640,0)` with a
256-entry CLUT declared `(0,480,256,1)`.

Two member shapes deviate from a plain TIM list:

- **Flat-strip CLUT upload.** The retail per-TIM uploader `FUN_800198E0`
  (`see ghidra/scripts/funcs/800198e0.txt`) uploads every member TIM's CLUT block as a
  flattened `(clut_x, clut_y, w*h, 1)` strip, NOT the declared `w × h` rect (which for
  the row-510/511 banks would overflow VRAM at `y >= 512`). The bundle's strips populate
  CLUT rows 510/511 (`fb_x` 0..320) plus the `(896, 498..=501)` / `(976, 304..=307)`
  side cells and raw entry 1's row-480 strip. See
  [npc-palette.md](npc-palette.md#boot-resident-strip-band-rows-510511) for the
  row layout and the capture evidence.
- **Row-patch members.** Six members carry no TIM magic: an 8-byte preamble followed by
  a bare TIM-style image block `[u32 bnum][u16 x][u16 y][u16 w][u16 h]` + halfword data
  (`bnum = 12 + w*h*2`). All six declare `(960, y, 256, 1)` for
  `y ∈ {456, 457, 458, 460, 461, 462}` - single rows patched over the atlas image,
  VRAM-edge-clipped to the visible 64 words. Live captures hold exactly these bytes over
  the disc atlas at those rows, in every phase.

The whole bundle is uploaded once at boot and never evicted - resident from the title
screen through every scene/mode. That is why field env meshes can reference CBA cells
on row 510 (`town01` env slots 21/26/74, `rikuroa` slots 50/51/63: CBA `(64,510)`,
texpage `(960,256)`) that no scene TIM ever uploads; the engine mirrors the upload in
its scene VRAM pre-pass (`SceneResources::build_targeted_with_options` +
`BuildOptions::system_ui`), byte-exact under the VRAM static-mask parity oracle.

## See also

- [asset::pack](pack.md) - the structurally similar in-DATA_FIELD pack.
- [PROT.DAT TOC](prot.md) - the index whose standalone entries use this pack.
- [PSX TIM](tim.md) - the texture sub-asset bundled here.
