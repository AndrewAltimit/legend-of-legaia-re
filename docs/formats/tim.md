# PSX TIM (texture)

A standard PlayStation texture format. The format is well-documented externally; we don't reimplement the parser. Magic check: first u32 == `0x00000010`.

```
u32  id          // 0x00000010
u32  flags       // bits 0..2 = pixel mode (0=4bit, 1=8bit, 2=16bit, 3=24bit)
                 // bit 3   = CLUT present
[CLUT block if flag bit 3 set]
[image block]
```

Each block has its own header `(u32 size, u16 dx, u16 dy, u16 w, u16 h)` followed by pixel data.

In the extracted streaming files, all observed TIMs use **type 8** (4-bit indexed with CLUT). They're VRAM-ready textures.

## VRAM emulation in the engine port

`crates/engine-render` emulates a 1024×512 R16Uint VRAM page so per-prim CBA/TSB selectors plus 4/8/15bpp + CLUT decoding can be done in a fragment shader. The viewer uploads every sibling TIM into VRAM so multi-page meshes render with the correct CLUT bindings.

Some character meshes reference CLUT rows that live in **different PROT entries** from their TMD source (the runtime asset chain stitches them together). The viewer's `--vram-extra-dir` flag is the workaround until the chain is fully traced for every scene type.

## Multi-row CLUT blocks

The PSX TIM spec allows a 4bpp TIM's CLUT block to contain multiple CLUT rows (each row is 16 BGR555 entries = 32 bytes), so the same indexed pixel data can be re-rendered under different palettes. Legaia uses this extensively for system-UI sprite sheets:

| Source TIM | Layout | CLUT-row usage |
|---|---|---|
| **System-UI sprite sheet** at `PROT.DAT[0x018E0]` (4bpp, 256×192, 16×16 CLUT block) | Lives in the unindexed pre-`init_data` gap - not reachable through the per-PROT-entry walker. Constants in `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_*`. | **Row 2** = the load-screen panel chrome (gold-bronze 9-slice border + dark-blue marbled interior region). **Row 7** = the pointing-finger cursor (white ink + grey shadow). Other rows render HP/MP/money panels, battle chrome, equipment frames, etc. |
| **Menu-glyph atlas** at `PROT.DAT[0x11218]` (4bpp, 256×256, multi-row CLUT block) | Same pre-`init_data` gap. See `legaia_asset::menu_glyph_atlas`. | **Row 13** carries the "Load" text glyphs the load screen draws inside its panel. Other rows render NEW GAME / CONTINUE / OPTIONS strings + smaller menu labels. |

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

> **Note:** an earlier revision tried to derive an "NPC palette" label
> *structurally* from the CLUT load position `fb=(0, 479)`. That is unsound:
> nearly every 256×256 4bpp scene/field texture page parks its CLUT in that
> same bottom VRAM band (see [NPC palettes](npc-palette.md)), so the rule
> conflated floors / walls / terrain with NPC colour tables. Labels are now
> content-keyed observations, not a CLUT heuristic.

## See also

- [Legaia TMD](tmd.md) - the mesh format that references these textures.
- [TIM-pack](tim-pack.md) - the standalone bundle of multiple TIMs.
- [NPC palettes](npc-palette.md) - the row-479 CLUT TIMs.
- [`subsystems/renderer.md`](../subsystems/renderer.md) - the renderer that uploads TIMs into VRAM.
