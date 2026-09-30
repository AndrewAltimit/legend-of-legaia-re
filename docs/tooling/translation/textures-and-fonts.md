# Textures and fonts

Two kinds of text sit outside the string / dialog / `ui_menu` scope of a
language pack: text drawn as pixels in a texture, and any character the retail
font has no glyph for. Both need art rather than a string edit.

## Textures with baked-in text

Some UI text is not a string at all - it is pixels in a TIM. Replacing one
means authoring new glyph art at the **exact same TIM footprint** (identical
width / height / bpp / CLUT layout) so the same-size in-place `DiscPatcher`
write applies.

The `tim` crate can encode a PNG back to a TIM, so a byte-identical-footprint
swap is mechanically possible (the texture-replacement flow in the
[randomizer reference](../randomizer.md#texture-replacement)). The blocker is
art authoring (and, for logos, rights), not the pipeline. Before repainting a
multi-palette sheet, read [shading and palettes](../../subsystems/shading.md#for-modders-editing-a-texture):
each palette recolours the whole image, and the game draws each sprite through
its own one. None are patched by
the translation pipeline - each is a scoped follow-up. Legally, the boot /
publisher logos must be left untouched regardless.

The text-bearing textures on the retail disc:

| Texture | Where | Baked text | Footprint / notes |
|---|---|---|---|
| Title wordmark | PROT 0888 (dup 0889 / 0890), `legaia_asset::title_pak` | "Legend of Legaia" logo, `PRESS START BUTTON`, TM / (C) copyright bands; an unused `<DEMO>` band retail never samples | Bands are sub-rects of one 256x256 TIM (`TITLE_BAND_*`). The logo is title art (a proper noun); `PRESS START BUTTON` is a candidate for a same-footprint band swap. Copyright bands must stay. |
| Title menu `NEW GAME` / `CONTINUE` | title overlay (inside PROT entry 0899 at `+0xEB44`) | rendered at runtime from the **dialog-font glyph atlas**, not a baked band (retail ignores the embedded footer band) | So this is *text*, but it lives in the title overlay code region the pipeline does not address by coordinate. Follow-up: pin the two label strings' VA window like a `ui_menu` pool. |
| Save/Load UI | PROT 0899 `+0x16908` (`SLOT n` pill) + the pre-`init_data` `PROT.DAT` gap (`Load` panel TIM) + the title-overlay memcard atlas `0x801E5120` | baked `SLOT 1..` pill label, the `Load` panel wordmark, and Japanese memcard strings in the atlas | Small 4bpp TIMs at fixed offsets; a same-footprint pill/panel swap is feasible. See [`save-screen.md`](../../subsystems/save-screen.md). |
| Config-screen TIMs | PROT 0899 `+0x169DC` / `+0x1F91C` | small option-screen chrome TIMs that sit after the config **string** pool (the strings are the `ui_menu` menu labels, already translatable) | Chrome art; only replace if a label is baked rather than drawn from the string pool. |
| Boot / publisher logos | PROT 0895 `init.pak` (`legaia_asset::init_pak`) - PROKION, SCEA / Sony | brand logos ("licensed by", studio marks) | **Do not alter** - trademark art, not localizable text. Listed only so a sweep does not mistake them for translatable UI. |
| Opening prologue caption | opening-sequence baked caption TIM (the narration **crawl** itself is `0x1F`-framed text and *is* covered via the dialog corpus) | a baked caption still shows English under the crawl | The crawl narration translates through `scene_dialog` / `inline_text`; only the baked caption TIM would need an art swap. |

## Multi-palette textures

A 4 bpp TIM stores palette **indices**, and many carry several 16-colour
palettes (the menu / battle UI sheet at `PROT.DAT` `0x18E0`, 256x192, holds
sixteen). The file does not say which palette a region is meant to be seen
in: every draw packet names its own CLUT, so the game picks a palette **per
sprite**. One texture is therefore several colourings of the same pixels, and
a PNG decoded through any single palette shows most regions in the wrong
colours - which is why a plain download of the UI sheet "comes with only the
first palette".

### The edit loop

Four download shapes, all of which the upload side (the ROM-patcher's "Your
edited PNG" box, or `tim-replace`) recognises by shape - no flag says which
one a file is:

| Shape | Size (UI sheet) | Edits change |
|---|---|---|
| **Image** through one view | 256x192 | pixels; a colour the region's palette lacks takes a slot that palette's pixels do not use |
| **Composite** - the image plus one row of colour cells per palette below it | 256x320 (16 rows of 16x8 cells) | pixels and palette colours in one file; a recoloured cell carries to every untouched pixel using it |
| **Palette strip** - the palettes alone | 256x256 (16x16 cells) | colours only; no index moves. Any integer cell size is accepted, every cell must be one flat colour |
| **Indexed PNG** - the stored indices, one palette as `PLTE` | 256x192 | indices only; no palette moves. Recognised when its `PLTE` equals one of the texture's palettes |

A *view* is either one palette (`Palette k`) or **in-game colours**: each
region through the palette the game draws it with, available when that
mapping is known (below). A plain image or composite is matched back to the
view that explains most of its pixels, so it does not matter which one it
was exported through.

```bash
legaia-patcher tim-export --input DISC.bin --offset 0x18E0 --format composite -o ui.png
legaia-patcher tim-palette-map --input DISC.bin --offset 0x18E0     # which region, which palette
legaia-patcher tim-replace --input DISC.bin --offset 0x18E0 --png ui.png --patch ui.ppf
```

`tim-export` takes `--format image|composite|strip|indexed` and a view:
`--clut K`, or `--in-game` (the default when the map is known and no `--clut`
is given). On the site, the texture editor grows a **Show colors through**
selector and three extra downloads for any texture with more than one
palette.

What the write guarantees: the TIM keeps its exact size and layout, **only
palettes the edit changes are rewritten** (every other palette stays
byte-identical), and an unedited download re-uploads as a no-op. Changing a
palette colour changes it everywhere the game draws with that palette - the
strip is the place to recolour a whole family of sprites at once.

### How a region's palette is chosen

For the UI sheet the mapping is disc data. The widget-class table in
`SCUS_942.54` at `0x800732A4` (parser `legaia_asset::ui_widgets`, described
in [`battle.md`](../../subsystems/battle.md#the-widget-class-table---where-every-chrome-sprite-comes-from))
gives every UI sprite its sheet rectangle and a palette byte `b`; with bit 6
clear, the sprite is drawn through **sub-palette `b & 0x3F`** of VRAM row
511. The sheet's own 16x16 CLUT block is those sub-palettes 0..15 (row `k`
sits at `(16k, 511)` at runtime), so the widget's palette byte *is* the
palette index in the downloaded file. Plate runs add their cap pair and
framed windows their eight frame tiles, both under the record's palette.
`legaia_patcher::texture_palettes` turns that into the per-pixel map;
`tim-palette-map` prints it.

Three edges of that map:

- **Sub-palettes 16..18** (the Stone, Rage and Faint status badges) live in a
  CLUT-only sibling TIM at `PROT.DAT` `0x1858`. Those badges show in their
  true colours, but their palettes are read-only here - paint them with the
  colours they already have, or edit that TIM's own palette strip.
- **Texels `(128, 96)..(191, 127)`** are covered at runtime by the 64x32
  button-glyph TIM at `PROT.DAT` `0x7B00` (uploaded to VRAM `(928, 352)`,
  palette sub-palette 19; see
  [`minigame-muscle-dome.md`](../../subsystems/minigame-muscle-dome.md)).
  What the game shows there is that TIM, so edits to the sheet under it are
  never seen - edit the glyph TIM instead.
- Texels no widget record samples are drawn through palette 0 in the map.
  Overlay code can still draw some of them with a palette of its own; where
  two records sample the same texels, single sprites win over plate runs,
  plate runs over framed windows, then the smaller rectangle.

For every other multi-palette texture no such table is decoded: every
palette is an equally valid view, and the composite shows them all at once.

## Font-patch scope

The retail USA font draws plain letters only, so accented Latin needs the
**accent font**: glyphs for the Western European accents drawn into the
dialog font's high cells at patch time, built from the user's own disc, with
a width for each. A pack asks for it with `accents: font`
([`pack-format.md`](pack-format.md#accents)); how it is built and what it
writes is in [`dialog-font.md`](../../formats/dialog-font.md#the-accent-font).

What it leaves out: Latin letters with no cell in the layout (Polish, Czech,
Hungarian) fold to ASCII, and Cyrillic, Greek and CJK need more glyphs than
the page holds - a second glyph bank and a multi-byte encoding in the
renderer, which no tool here attempts.

The official PAL discs carry their own accent glyphs. See
[`pal-localizations.md`](../pal-localizations.md) for:

- the CP437-aligned accent byte layout the accent font shares;
- how the official French/German/Italian text aligns id-/order-for-order to
  the USA disc (`legaia-patcher translate diff-disc`);
- how to lift it onto USA coordinates (`translate lift-official`);
- the per-string versus per-MAN fit rate (`translate fit-report`).
