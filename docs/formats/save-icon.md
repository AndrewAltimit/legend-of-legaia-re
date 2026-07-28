# Save-Slot Portrait Sheet

The 16x16 character faces the save UI draws and the PSX memory-card block
icon is cut from. One 4bpp TIM in the **menu overlay** (PROT entry 899)
carries all of them as a single horizontal strip.

Parser: `legaia_asset::save_icon`. Disc pins in
`crates/asset/tests/save_icon_real.rs`. Modding path:
`legaia_patcher::save_icon` (`legaia-patcher save-icon-list` /
`save-icon-export` / `save-icon-replace`).

## Contents

- [Where it lives](#where-it-lives) · [Strip layout](#strip-layout) ·
  [One palette per tile](#one-palette-per-tile)
- [Index rules](#index-rules) - [memory-card block icon](#memory-card-block-icon) ·
  [save-UI per-character face](#save-ui-per-character-face)
- [The de-interleave](#the-de-interleave) · [The second copy](#the-second-copy)
- [Tile 15](#tile-15) · [Modding notes](#modding-notes)

## Where it lives

| Property | Value |
|---|---|
| PROT entry | 899 (the menu overlay; also hosts shop / inn / status / save UI) |
| Offset in entry | `0x1F908` (TIM header) |
| Overlay VA | `0x801EE120` (base `0x801CE818`) |
| Pixel mode | 4bpp, flag word `0x00010008` |
| Image rect | `(960, 224)`, 64 VRAM halfwords x 16 rows = 256 x 16 texels |
| CLUT rect | `(0, 1)`, 256 entries x 1 row |

The flag word sets reserved bit 16, so a jPSXdec-strict TIM reader rejects
it; the lenient reader plus the rect fingerprint is the parse path. The
offset is not the location mechanism - `legaia_asset::save_icon::find_in_entry`
scans the entry for a 4bpp TIM with exactly those two rects, so the sheet is
found from the bytes. The constant is exported for patchers and pinned
against the scan by test.

**Provenance.** The VA `0x801EE120` has exactly one materialisation anywhere
on the disc: the `lui`/`addiu` pair at `0x801DD4D4`, feeding the per-TIM
uploader `FUN_800198E0`. That call sits in the save-screen driver
`FUN_801DD35C`, which uploads two TIMs back to back - the load-screen UI
sheet at `0x801E5120` (image `(960,0)` 64x256) first, then this strip. The
strip lands inside the sheet's rows 224..239, which the sheet leaves
entirely blank, so the later upload overwrites nothing that carries art.

## Strip layout

Sixteen 16x16 tiles laid side by side in one 256-pixel-wide, 16-row image.
Each strip row is 128 bytes (256 texels at 4bpp), and tile `t` occupies
bytes `t*8 .. t*8+8` of every row.

```
row  0: [tile 0 ][tile 1 ][tile 2 ] ... [tile 14][tile 15]   128 bytes
row  1: [tile 0 ][tile 1 ][tile 2 ] ... [tile 14][tile 15]   128 bytes
 ...                                                          x 16 rows
```

At 4bpp a 256-texel row is 64 VRAM halfwords, so at `fb_x = 960` the strip
ends exactly at the VRAM right edge (`960 + 64 = 1024`).

## One palette per tile

The 256-entry CLUT block is **sixteen 16-colour palettes**, not one 256-colour
palette: palette `t` is entries `t*16 .. t*16+16`, and it belongs to tile `t`.
Uploaded to VRAM row 1, palette `t` sits at x = `t*16`, which makes its PSX
CLUT id `(1 << 6) | ((t*16) >> 4)` = `0x40 + t`.

Every consumer uses the same pair of expressions for an index `i`:

| Quantity | Expression |
|---|---|
| Texture u coordinate | `i * 16` |
| CLUT id | `0x40 + i` |

That is why the sheet cannot go through a generic multi-palette texture
replacer: rebuilding the palette and replicating it across CLUT rows - the
correct behaviour for an ordinary multi-palette TIM - would repaint all
sixteen portraits at once.

## Index rules

The tile space is a **character** space: fifteen different faces. Its two
consumers index it by different quantities, which is the thing to keep
straight.

### Memory-card block icon

`FUN_801E1934` composes the save block. It stamps the header magic, patches
the slot digits into the title template at `0x801E4FC8`, and then grabs the
icon **out of VRAM**:

| Step | Instructions | Effect |
|---|---|---|
| Icon rect | `0x801E1B1C..0x801E1B34` | `RECT = (0x3C0 + slot*4, 0xE0, 4, 16)` |
| Icon frames | three `StoreImage` calls | block `+0x80`, `+0x100`, `+0x180` |
| CLUT row grab | `RECT = (0, 1, 256, 1)` | whole palette row into a stack buffer |
| CLUT select | `memcpy(block+0x60, buf + slot*32, 32)` | palette `slot` into the block |

So **tile index = card slot index**, and the save number the game displays
is `slot + 1`. There is no modulo and no clamp; the bound is that a PSX
memory card holds 15 blocks. Verified against real cards: the block whose
directory name ends `-00` carries tile 0, and `-01` carries tile 1.

All three icon frames receive the same rect, so the three 128-byte frame
slots hold identical pixels even though the header's frame descriptor
(`+0x02` = `0x11`) declares a single frame.

### Save-UI per-character face

The save-screen info panel draws one portrait per party member, indexed by
the per-character **party id** the slot buffer carries at `+0x2C + i`
(`0` Vahn, `1` Noa, `2` Gala). `FUN_801E08D8` at `0x801E0C6C` loads that
byte and feeds it through the same two expressions into the sprite
descriptor at `0x801E5048`. A second site in the same function
(`0x801E0ED4`, reached from the early branch at `0x801E09B4`) applies the
identical idiom to the **slot** index instead.

## The de-interleave

A PSX save block stores its icon as a contiguous 16x16 tile:

| Block offset | Size | Field |
|---|---|---|
| `+0x00` | 4 | header: `SC` magic, icon-frame descriptor `0x11`, block count `1` |
| `+0x60` | 32 | 16-entry palette (u16 LE BGR555) |
| `+0x80` | 128 | icon pixels, 16 rows of 8 bytes, contiguous |

All four header bytes are part of the icon's contract: `+0x02` describes the
icon that follows. A writer that stamps only the two magic bytes leaves
`+0x02`/`+0x03` as found, which in a previously-free block is zero - a
correct payload behind a header the BIOS card browser reads as malformed.
The port stamps the whole header plus the slot's digits and portrait through
`legaia_save::card::write_retail_block_identity`.

The strip stores the same tile as 16 eight-byte runs 128 bytes apart. Retail
never converts in software - `StoreImage` reads a VRAM rectangle, and a
rectangular read is contiguous by construction. Offline the conversion is
arithmetic (`SaveIconSheet::tile_block_pixels`), gathering run `row` from
`pixel_data + row*128 + tile*8`.

**A consequence worth knowing:** byte-searching the disc for a real save
block's icon finds the **palette** verbatim in the overlay and finds the
**pixels** nowhere, because the on-disc pixel order differs. That asymmetry
is the layout difference, not a missing asset.

## The second copy

Tiles 0, 1 and 2 also ship as three standalone 16x16 TIMs, already in
contiguous tile layout because each is its own image rect. They are members
16, 17 and 18 of the boot-resident system-UI TIM-pack at **raw PROT TOC
entry 0** ([`tim-pack.md`](tim-pack.md), parser
`legaia_asset::system_ui_bundle`):

| Member | `PROT.DAT` offset | Image rect | CLUT rect | Tile |
|---|---|---|---|---|
| 16 | `0x1AC90` | `(976, 256)` 4x16 | `(976, 304)` 16x1 | 0 |
| 17 | `0x1AD50` | `(980, 256)` 4x16 | `(976, 305)` 16x1 | 1 |
| 18 | `0x1AE10` | `(984, 256)` 4x16 | `(976, 306)` 16x1 | 2 |

Each member is `0xC0` bytes, which is the `0xC0` stride a byte search sees.
Their pixels and palettes are byte-identical to strip tiles 0/1/2, which
makes them an independent oracle for the de-interleave - the disc-gated test
asserts exactly that.

Two things this settles. First, **why no extraction file contains those
bytes**: raw TOC entries 0 and 1 are the head region the extraction index
space skips (extraction index = raw entry - 2, so extraction `0000` is raw
entry 2, `init_data`). The offsets are inside a real PROT entry - just not
one that gets an `NNNN_*.BIN`. See [`prot.md`](prot.md). Reading a raw-entry
number as an extraction number is the trap here, and it is the same +2 skew
[`cdname.md`](cdname.md) documents for CDNAME labels.

Second, it is **not** a pre-de-interleaved copy of the whole sheet - only
three tiles, at a different VRAM residency (row 256, own CLUT rows), and not
the source the block-icon writer reads. That writer reads row 224.

A byte search also reports the palettes at `PROT.DAT` `0x5C5D11C`, which is
not a third copy: it is entry 899's own CLUT block seen through the whole
archive image (entry 899 starts at sector 47227, so `0x5C5D11C` is
entry-relative `0x1F91C`).

## Tile 15

Tile 15 is blank - a single flat pixel index across all 256 texels *and* all
sixteen palette entries zero. Fifteen portraits for fifteen card slots, plus
one tile of width padding that rounds the strip to 64 VRAM halfwords.

No code path selects it. Both index rules are bounded below 15 (card slots
0..14; party ids 0..2), and neither the block-icon writer nor the sprite
emitters clamp or wrap into it.

## Modding notes

`legaia-patcher save-icon-replace` exposes **15 slots, not 16**. Tile 15 is
refused: art written there would never be displayed.

Replacing one portrait is 17 same-size in-place writes - the tile's 32
palette bytes plus its 16 eight-byte pixel runs - so every other portrait
stays byte-identical. The encoder starts from the tile's existing palette
and only claims slots the edit actually needs, so re-encoding an unmodified
portrait reproduces the disc bytes exactly and a shared PPF carries only the
user's own edit.

### Tile 15 as free space

Tile 15's bytes are, on the evidence below, unread:

| Chunk | Offset in PROT entry 899 | Shape |
|---|---|---|
| Palette | `0x1FAFC` | 32 bytes, contiguous |
| Pixels | `0x1FB28 + row*128 + 120`, `row` 0..15 | 16 runs of 8 bytes, stride 128 |

What was checked, and what each check does *not* cover:

- **No reference exists to the sheet's data.** A five-form sweep
  (`scripts/ghidra-analysis/find-address-word-refs.py`: literal word,
  `lui`+`addiu`, `jal`, `j`, PC-relative branch) across `SCUS_942.54`, every
  based overlay image and every PROT entry finds no reference to the CLUT
  data, the pixel data, or tile 15's own addresses. The only hits are
  PC-relative branches in *other* slot-A overlays, which share the load base
  and therefore alias the VA; a branch cannot leave its own image, so none of
  them reaches this data. The sheet is reachable only through its TIM header,
  which has exactly one materialisation.
- **No other TIM targets those VRAM cells.** Sweeping every extracted TIM for
  a rect overlapping the strip's image `(960..1024, 224..240)` or its CLUT row
  finds only this sheet and the load-screen UI sheet it is uploaded over - and
  that sheet's rows 208..255 are entirely zero, so it has no art there to
  lose. Nothing else in the corpus writes the CLUT row at x = 240..255.
- **Both index rules stop short of 15.** See [Tile 15](#tile-15).

The residual risk is a sprite emitter sampling `u = 240..255, v = 224..239`
without going through either index rule. No such emit was found, but the
sweep above proves absence of *references to the bytes*, not absence of a
texture sample - a sample carries no address of its source.

If used, the shape matters: 32 contiguous palette bytes and 128 pixel bytes
in **8-byte runs 128 bytes apart**. That is workable for a small data table
read with a stride and useless for injected MIPS, which needs contiguous
words. The palette row is also re-uploaded on every save-screen entry
(`FUN_801DD35C`), so its VRAM copy is transient; the disc bytes are not.
