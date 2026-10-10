# PSX TIM (texture)

TIM is the standard PlayStation texture file, and Legaia uses it unmodified. A TIM is not just pixels: it also says **where in video memory (VRAM) the pixels go**. Most Legaia textures are indexed - each pixel is a 4-bit or 8-bit index into a colour look-up table (CLUT, the palette), and the CLUT is a second block in the same file with its own VRAM position. The game uploads both blocks, and a polygon later picks its texture page and its palette by VRAM coordinate. That is why the placement fields matter as much as the pixel data, and why a texture exported through its own palette can look wrong.

Implementation: [`crates/tim`](../../crates/tim/README.md) - `parse` (lenient), `parse_strict` (detection-grade), `decode_rgba8`, `encode`.

## Layout

| Offset | Size | Field | Meaning | Confidence |
|---|---|---|---|---|
| `0x00` | u32 | `id` | Magic `0x00000010` | Confirmed |
| `0x04` | u32 | `flags` | Bits 0..2 = pixel mode (`0` 4bpp, `1` 8bpp, `2` 16bpp, `3` 24bpp); bit 3 = CLUT block present | Confirmed |
| `0x08` | block | CLUT block | Present only when flag bit 3 is set | Confirmed |
| after it | block | image block | Always present | Confirmed |

Both blocks share one header:

| Offset | Size | Field | Meaning | Confidence |
|---|---|---|---|---|
| `+0x00` | u32 | `size` | Block length in bytes including this 12-byte header: `12 + w*h*2` | Confirmed |
| `+0x04` | u16 | `fb_x` | VRAM x of the block's top-left, in 16-bit VRAM words | Confirmed |
| `+0x06` | u16 | `fb_y` | VRAM y | Confirmed |
| `+0x08` | u16 | `w` | Width in **16-bit VRAM words**, not pixels | Confirmed |
| `+0x0A` | u16 | `h` | Height in rows | Confirmed |
| `+0x0C` | `w*h*2` | data | CLUT: BGR555 entries. Image: packed indices or 16-bit texels | Confirmed |

An image block's pixel width is `w * 4` at 4bpp and `w * 2` at 8bpp, because one VRAM word holds four or two indices. A CLUT entry is BGR555 with the top bit as the semi-transparency (STP) flag.

The streaming-file TIMs all carry `flags = 8`: 4bpp indexed with a CLUT.

## VRAM placement

VRAM is one 1024x512 grid of 16-bit words that holds the framebuffers, every texture page and every palette. A TIM upload is two rectangle copies into it:

```
VRAM, 1024 x 512 words
+------------------------------------------------------------------+
| display + draw buffers          | texture pages                  |
|                                 |   +----------+                 |
|                                 |   | image    |  w words x h    |
|                                 |   | block    |  at (fb_x,fb_y) |
|                                 |   +----------+                 |
|                                                                  |
| CLUT rows (bottom band, y ~ 475..511)                            |
| [16 or 256 BGR555 entries at the CLUT block's (fb_x, fb_y)]      |
+------------------------------------------------------------------+

draw time:  texel index --> CLUT cell named by the primitive's CBA --> BGR555 colour
            texture page named by the primitive's TSB (tpage)
```

The primitive names a **VRAM cell**, not a file: `CBA` (CLUT base address) selects the palette row and `TSB` the texture page. Whatever was uploaded there last is what the draw reads. See [`subsystems/shading.md`](../subsystems/shading.md) for the whole colour chain.

`crates/engine-render` emulates the 1024x512 R16Uint VRAM page so per-primitive CBA/TSB selection and 4/8/15bpp CLUT decoding run in a fragment shader. Some meshes reference CLUT rows uploaded from **different PROT entries** than their TMD; the engine's scene loader uploads the whole scene's TIMs before drawing, and the standalone asset viewer takes `--vram-extra-dir` to pull in sibling TIMs when it is pointed at a single mesh.

## Multi-row CLUT blocks

The PSX TIM spec allows a 4bpp TIM's CLUT block to contain multiple CLUT rows (each row is 16 BGR555 entries = 32 bytes), so the same indexed pixel data can be re-rendered under different palettes. Legaia uses this extensively for system-UI sprite sheets:

| Source TIM | Layout | CLUT-row usage |
|---|---|---|
| **System-UI sprite sheet** at `PROT.DAT[0x018E0]` (4bpp, 256×192, 16×16 CLUT block) | Lives in the unindexed pre-`init_data` gap - not reachable through the per-PROT-entry walker. Constants in `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_*`. | **Row 2** = the load-screen panel chrome (gold-bronze 9-slice border + dark-blue marbled interior region). **Row 7** = the pointing-finger cursor (white ink + grey shadow). Other rows render HP/MP/money panels, battle chrome, equipment frames, etc. |
| **Menu-glyph atlas** at `PROT.DAT[0x11218]` (4bpp, 256×256, multi-row CLUT block) | Same pre-`init_data` gap. See `legaia_asset::menu_glyph_atlas`. | Rows render NEW GAME / CONTINUE / OPTIONS strings + smaller menu labels. The load screen's "Load" title is **not** here, in row 13 or any other; it is drawn with the dialog font ([`save-screen.md`](../subsystems/save-screen.md)). |

Both TIMs are byte-confirmed against retail VRAM dumps; see [`subsystems/save-screen.md`](../subsystems/save-screen.md#sprite-asset-sources-continue--load-screen) for the pinning method (PCSX-Redux save state → `extract_vram_from_sstate.py` → CLUT-row byte cross-reference against `PROT.DAT`).

Browse them in the asset viewer with `asset-viewer tim extracted/PROT.DAT --offset 0x018E0 --clut <row>` (any of 0..15).

## Cataloging every PROT.DAT TIM

`PROT.DAT` is also indexable as one flat 2048-byte-sector stream. Scanning the
whole image (rather than per-TOC-entry) catches every standard TIM regardless
of which addressing layer hosts it - including the TIMs in the unindexed
system-UI gap before the first entry (the menu-glyph atlas and load-screen
chrome above). `legaia_asset::tim_catalog` does this and maps each hit back to
its owning PROT entry + byte offset (or the gap), producing a per-TIM catalog
keyed by a stable id:

```
asset tim-catalog extracted/PROT.DAT --out catalog.tsv   # or .json
asset tim-catalog extracted/PROT.DAT --rollup            # count + digest
```

### Strict validation (what counts as a TIM)

A magic-only scan over arbitrary bytes turns up many spurious matches - a
coincidental `0x00000010` word inside another TIM's pixel data, blocks with
trailing padding, or `Mixed`/garbage pixel modes. `legaia_tim::parse_strict`
applies the extra checks that separate real, VRAM-ready TIMs from noise:

- **No reserved flag bits.** Only bits 0..3 (pixel mode + CLUT-present) may be
  set; a flags word like `0x00010008` (reserved bit 16 set) is rejected.
- **A real pixel mode.** `pmode` must be 0..=3.
- **Exact block lengths.** Each block's `size` field must equal `12 + w*h*2`
  precisely - no trailing padding.
- **Nonzero dimensions** and an **in-VRAM-bounds image rectangle** (the image
  must fit inside the 1024×512 16-bit framebuffer at its load position).

The **CLUT** rectangle is deliberately *not* bounds-checked: Legaia stores many
NPC palettes at `fb_y` 510..511 (the [row-479 CLUT band](npc-palette.md)) with
heights up to 16, so a legitimate CLUT block extends a few rows past the
framebuffer's bottom edge.

### Flat-strip CLUT uploads

Two Legaia TIM families declare a **multi-row CLUT block** that the runtime
uploads as a single flat horizontal strip of `w × h` entries at the block's
origin, not the declared rect (which would overflow the framebuffer):

- The **field-character atlas** palettes (PROT 0874 §2 entries 1/2/3): each
  declared block lands as a strip on **row 478**
  (`legaia_asset::field_char_textures::upload_to_vram`).
- The **shared interior page** (the 256×256 4bpp TIM at image `(960,256)`):
  its declared 16×16 CLUT block at `(0,510)` lands as a 256-entry strip on
  **row 510** - byte-identical to VRAM row 510 in every captured field
  save from the first post-New-Game scene onward. The TIM's only raw copy
  lives in the **unindexed head gap** of `PROT.DAT` (byte offset
  `0x11218`, after the 3-sector TOC but before the first entry's data -
  the same unindexed region as the system-UI TIMs below), so no per-entry
  read can source it. Town env meshes reference mid-strip CBAs (e.g.
  town01's `(64,510)` = strip entry 64) with texpages inside the
  `(960,256)` image. Parser + uploader: `legaia_asset::interior_page`.

A renderer placing these blocks at their declared rects clips/wraps the
rows past `y = 512` and leaves the strip cells unpopulated - the meshes
that sample them then drop (or render black) even though every byte is on
disc.

Under this rule a flat scan of the retail NA `PROT.DAT` recovers the same TIM
set an independent reference decoder reports, cross-checked item-for-item
(identical offsets, dimensions, bit depths, and palette counts). The lenient
`legaia_tim::parse` is retained for callers decoding bytes already known to be
a TIM (web-viewer thumbnails, sub-asset extraction), where the extra
rejections would only get in the way.

The committed reference catalog
(`crates/asset/tests/data/prot_tim_catalog.tsv`) holds derived metadata only
(offsets, dimensions, CLUT counts, byte lengths, FNV-1a fingerprints) - never
pixel bytes - and a disc-gated regression rebuilds it from the disc and pins
the count + a rollup digest. The in-browser asset viewer builds the same
catalog live from a user-supplied disc and lets you page through every TIM by
id with its CLUT variants.

### Deep catalog: TIMs inside LZS-compressed sections

The flat catalog - like the reference decoder - scans only **raw** bytes, so
any TIM stored inside an [LZS-compressed](lzs.md) `PROT.DAT` section is
invisible to it, and most character and scene textures are compressed.
`legaia_asset::tim_deep_catalog` recovers them as a **separate tier**: it walks
every PROT entry, LZS-decompresses it with `legaia_lzs::decompress_container`
(the same decode path [`tim_scan`](../../crates/asset/src/tim_scan.rs) uses),
and strict-parses every TIM in each decoded section.

```
asset tim-deep-catalog extracted/PROT.DAT --out deep_catalog.tsv   # or .json
asset tim-deep-catalog extracted/PROT.DAT --rollup                 # count + digest
```

Each deep hit is keyed by `(entry index, LZS section index, offset within the
decoded section)` plus dimensions / bpp / CLUT count / byte length / an FNV-1a
of the decoded bytes. The validity gate matters: **LZS "decompresses without
error" is never a validity signal** - the 4 KB ring buffer initialises to
zeros, so random input decodes to plausible-looking bytes (see
[`lzs.md`](lzs.md)). A deep hit is admitted only when the decoded bytes both
pass `parse_strict` *and* decode to RGBA, which rejects the coincidental
TIM-magic-in-noise a magic-only scan of decompressed garbage would produce.

The deep tier is kept wholly separate from the flat catalog (which stays
byte-identical to its reference). It has no external decoder oracle - the
reference decoder doesn't decompress - so its disc-gated regression
(`crates/asset/tests/tim_deep_catalog_coverage.rs`) instead guards the decode
path + validity gate by pinning count + rollup digest + a byte-exact committed
reference (`crates/asset/tests/data/prot_tim_deep_catalog.tsv`, metadata + FNV
only; no decompressed Sony bytes). The viewer surfaces the deep tier as a
distinct "compressed textures" grid below the raw catalog.

### Semantic labels

The catalog records *where* each texture lives, not *what* it is.
`legaia_asset::tim_labels` is a curated label table that answers the "what" for
identified textures. It is keyed by **content fingerprint** - the FNV-1a-64 the
catalogs already record - so a single label propagates to every catalog id that
shares those bytes (duplicate textures, and textures aliased across overlapping
PROT entries), and one table serves both the raw and the deep tier. The label
is surfaced as a `label` column in the committed reference TSVs and in the
viewer's grid + info panel.

A label is one of:

- A **coarse visual category** assigned by inspecting the decoded thumbnail:
  `environment` (floor / wall / structure), `terrain` (overworld ground),
  `foliage`, `character`, `ui-text`, `effect`, or `other`.
- A **precise reverse-engineered role** for a texture whose loader site and
  byte offset are pinned: the menu-glyph atlas, the main-title sprite sheet,
  the four `init.pak` publisher / warning logos, and the load-screen UI sheet +
  party portraits + empty-slot frame.

Both are our own observations - not asset strings or pixel data - so the table
ships in the repo (`crates/asset/src/data/tim_categories.tsv`), like the
ground-truth gamedata tables. A `table_is_valid` check (unique fingerprints,
controlled vocabulary) plus the disc-gated catalog regressions guard it.

The coarse categories were assigned by reviewing the decoded thumbnails:
`asset tim-render-distinct <PROT.DAT> --out <dir>` decodes each distinct
texture (deduped by fingerprint) to a local PNG, and `scripts/asset-investigation/montage_tims.py`
lays them into indexed contact sheets for review. Those PNGs are decoded pixel
data and stay local - only the resulting fingerprint→label table is committed.

> **Note:** an "NPC palette" label cannot be derived structurally from the
> CLUT load position `fb=(0, 479)`: nearly every 256×256 4bpp scene / field
> texture page parks its CLUT in that same bottom VRAM band (see
> [NPC palettes](npc-palette.md)), so the rule conflates floors / walls /
> terrain with NPC colour tables. Labels are content-keyed observations.

## Encoding: PNG -> TIM (texture replacement)

`legaia_tim::encode` is the write side of the parser: it builds a TIM whose
**structure is copied verbatim from an original** - pixel mode, image and CLUT
dimensions, and every `fb_x`/`fb_y` VRAM placement field (image *and* CLUT
blocks; the game depends on those coordinates, e.g. the
[row-479 CLUT sharing](npc-palette.md)) - and whose pixels come from
caller-supplied RGBA (typically a decoded PNG). Same dimensions + bpp + CLUT
layout means the output is byte-for-byte the same *size* as the original,
which is what same-size in-place disc patching requires. This is the encoder
behind `legaia-patcher tim-replace` and the ROM-patcher page's texture panel
(see [`tooling/randomizer.md`](../tooling/randomizer.md#texture-replacement)).

### Alpha -> STP mapping

PSX texels carry a 1-bit STP (semi-transparency) flag, not an alpha channel.
The encoder maps 8-bit alpha as:

| Alpha | Encoded texel |
|---|---|
| `0` | `0x0000` - transparent black (the GPU skips it; RGB is ignored) |
| `1..=254` | STP set + RGB truncated to 5 bits/channel (semi-transparent when the primitive blends) |
| `255` | STP clear + RGB truncated - **except opaque pure black**, which becomes `0x8000` (STP-only black; plain `0x0000` would read back transparent) |

### Original-color reuse (byte-exact round trips)

Before the alpha rule applies, the encoder reuses the original wherever the
new image asks for a color the original already displays (compared in
decoded-RGBA space): a pixel whose position held the same color keeps its
original index / texel verbatim, and other palette hits reuse the first entry
that decodes to the color. This preserves the original's STP choices for
untouched regions, and makes `encode(decode(tim))` reproduce the original TIM
**byte-for-byte** - a disc-gated regression pins that identity across the
entire raw catalog.

### Palette fitting

Indexed modes must fit the palette (16 / 256 distinct 15-bit colors). Colors
already in the original palette are free; new colors overwrite slots the new
image no longer references. Overflow is a hard error listing the offending
pixel coordinates + colors, or - behind an explicit quantize opt-in - the
least-frequent extras fold to their nearest palette color. Only the palette
the image was drawn through (`EncodeOptions::palette`, 0 by default) is ever
rewritten; every other palette of a multi-palette CLUT stays byte-identical.
The game picks a palette per sprite, so a multi-palette TIM is several
colourings of one set of indices, and copying the edited palette over the
others would recolour every region drawn through them. Editing several
palettes at once - per-region views, the palette strip, the composite, the
indexed PNG - is `legaia_tim::multi_palette`; the workflow is in
[`textures-and-fonts.md`](../tooling/translation/textures-and-fonts.md#multi-palette-textures).

## Why an exported palette can look wrong

A TIM export decodes the image through one of the file's own CLUT rows. That is
the colour the game shows only when three things hold, and on Legaia they
often do not:

- **Each palette recolours the whole image.** A multi-palette 4bpp sheet is one
  grid of indices that each sprite / polygon reads through its own palette, so
  any single palette shows most of the sheet in colours it is never drawn
  with.
- **The draw names a VRAM cell, not a file.** A sprite can take a palette
  another TIM uploaded (the system-UI sheet's status badges reach the
  CLUT-only TIM at `PROT.DAT[0x1858]`), and a later upload can overwrite a
  TIM's own palette before anything draws it (the ASCII battle font's
  `(0, 510)` strip, covered by the menu-glyph atlas at boot).
- **The screen multiplies the texel.** The primitive's colour word
  (`texel * colour / 128`) and the depth cue sit on top of the palette, so a
  correctly exported texture still reads darker or brighter than in game.

`legaia_asset::tim_palette_context` resolves the first two from the disc: the
boot-VRAM CLUT state (`BootClutVram`), a per-rectangle palette map of the
system-UI page from the SCUS widget table (`sheet_palette_regions`), the one
per-texel attribution of it (`texel_palettes`), and an "as drawn" composite
decode (`composite_rgba`). The asset viewer's TIM catalog surfaces all of it
in its palette list and notes; the ROM patcher's texture editor reads the same
`texel_palettes`. The whole colour chain, and
what it means for texture editing, is on
[`subsystems/shading.md`](../subsystems/shading.md).

## See also

- [Shading and palettes](../subsystems/shading.md) - how a texel becomes a screen pixel.
- [Legaia TMD](tmd.md) - the mesh format that references these textures.
- [TIM-pack](tim-pack.md) - the standalone bundle of multiple TIMs.
- [NPC palettes](npc-palette.md) - the row-479 CLUT TIMs.
- [`subsystems/renderer.md`](../subsystems/renderer.md) - the renderer that uploads TIMs into VRAM.
