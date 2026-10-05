# Shading and palettes - how Legaia colours a pixel

Every textured pixel Legaia puts on screen is the product of a short, fixed
chain: a 4-bit or 8-bit **index** in a texture, a 16-bit **palette entry** that
index selects out of video memory, a per-primitive **colour word** the GPU
multiplies the entry by, an optional **depth cue** that pulls that colour word
toward a far colour, an optional **blend** with what is already on screen, and
a **dither + 15-bit write** into the framebuffer. The colour word is baked into
the mesh for nearly every primitive; the exception, the light-source rows,
gets it from the GTE light ([below](#step-4-the-colour-word---why-raw-textures-look-darker-or-brighter)).

This page walks the chain in order, with the retail routine behind each step,
and ends with what it means for someone editing textures. The per-routine
detail lives on the pages it links: [`renderer.md`](renderer.md) (the TMD
renderer and the port's shaders), [`battle.md`](battle.md) (the battle
ambient and actor tint), [`field-ambient-fx.md`](field-ambient-fx.md)
(animated palettes), [`tim.md`](../formats/tim.md) and
[`npc-palette.md`](../formats/npc-palette.md) (where palettes are uploaded).

## The chain at a glance

| Step | What happens | Retail source |
|---|---|---|
| 1. Texel index | 4bpp: low nibble = left texel; 8bpp: one byte | GPU texture fetch |
| 2. Palette lookup | index selects an entry of the 16/256-entry CLUT at the primitive's CLUT cell in **VRAM** | `CBA` word of the GP0 packet |
| 3. Transparency key | entry `0x0000` is never drawn; `0x8000` is opaque black | GPU rule |
| 4. Colour word | `out = texel * colour / 128`, per channel, clamped at 255 | TMD colour word / sprite colour |
| 5. Depth cue | colour word blended toward the far colour by `IR0` **before** step 4 | `DPCS` / `DPCT` in the TMD dispatch |
| 6. Semi-transparency | STP entries of an ABE primitive blend with the framebuffer (4 modes) | texpage `ABR` bits |
| 7. Dither + write | 4x4 ordered dither, truncate to 5 bits per channel | DRAWENV `dtd`, `_DAT_8007BA66` |
| 8. Display | 5-bit channel shown as 8 bits | video DAC |

Steps 4 and 5 are where "the game applies shading": the same palette entry
lands on screen darker, brighter, or tinted depending on the primitive that
draws it. Steps 1-3 are what a texture export shows.

## Steps 1-3: index, palette, transparency

A 4bpp texture stores one **index** per texel; the colour lives in a palette
(a CLUT - colour look-up table) of 16 entries, each a 16-bit word:

```text
bit 15     STP  (semi-transparency flag)
bits 10-14 blue  (5 bits)
bits  5-9  green (5 bits)
bits  0-4  red   (5 bits)
```

A 5-bit channel shows as `(c << 3) | (c >> 2)` in an 8-bit export (0 -> 0,
31 -> 255). The word `0x0000` is the transparency key - the GPU skips that
texel on every textured draw, opaque or not - and `0x8000` (STP set, all
channels zero) is how the game spells opaque black.

### The palette comes from VRAM, not from the file

A primitive does not name a palette inside its texture file. Its packet
carries a **CLUT address** (`CBA`): VRAM `x = (CBA & 0x3F) * 16`,
`y = CBA >> 6`. The GPU reads whatever 16 halfwords sit there **at draw
time**. A TIM's own CLUT block is simply what that file uploads, and three
things decide whether the palette you see in the file is the one a draw uses:

- **Upload shape.** The retail per-TIM uploader `FUN_800198E0` sends a CLUT
  block of declared `w x h` as a single flat strip of `w*h` entries on row
  `y` (see `ghidra/scripts/funcs/800198e0.txt` and
  [`tim.md`](../formats/tim.md#flat-strip-clut-uploads)). "Palette N" of a
  TIM is therefore VRAM `(clut_x + 16N, clut_y)`, not row `clut_y + N`.
- **Last write wins.** Uploads are ordinary `LoadImage` transfers. In the
  boot-resident system-UI bundle the menu-glyph atlas's 256-entry strip on
  row 510 lands after the ASCII battle font's 16-entry strip at the same
  cells, so the font's own palette never exists in VRAM; the game draws the
  font through the atlas's cell `(208, 510)`. Scene textures race for the
  row-479 band the same way ([`npc-palette.md`](../formats/npc-palette.md)).
- **Sharing.** A sprite or polygon may name any cell. The system-UI sheet's
  sprites take their palettes from the widget-class table's palette byte
  ([`ui_widgets`](../../crates/asset/src/ui_widgets.rs)): row 511 cells 0-15
  are the sheet's own sixteen, cells 16-18 belong to a separate CLUT-only
  TIM uploaded just before it, and the element badges read a 4x4 block at
  `(896.., 498..501)`.

### One texture, many palettes

A multi-palette TIM is **one grid of indices** that several sprites or
polygons read, each through its own palette. Each palette recolours the
**whole** image, so in any single palette most of the texture shows colours it
is never drawn with - the nine status badges of the system-UI sheet are green,
purple, brown and red in the game, each through its own palette, and any one
palette paints all nine in a single colour scheme. That is not a decode bug; it is how
4bpp sheets are stored.

### Which palette draws which region

For the system-UI page (VRAM `(896, 256)`: the menu / battle sheet at
`PROT.DAT` `0x18E0` and the smaller TIMs uploaded beside it) the mapping is
disc data, and one kernel reads it: `legaia_asset::tim_palette_context::texel_palettes`.
The asset viewer's **As the game draws it** view and the ROM patcher's
texture editor (region list, in-game view, `tim-palette-map`) both call it,
so the two can no longer disagree.

- **Who samples what.** Every rectangle comes from the widget-class table
  (`SCUS_942.54` `0x800732A4`, [`ui_widgets`](../../crates/asset/src/ui_widgets.rs),
  [`battle.md`](battle.md#the-widget-class-table---where-every-chrome-sprite-comes-from)),
  expanded per draw arm of `FUN_8002C69C` exactly as the arm emits it: a
  class-5 record is its own sprite; a class-3 plate run adds its cap pair
  (`0x80073A60 + tileset * 8`); a class-4 bar (arm `0x8002EAB4`) draws the
  same cap pair as two sprites and then stretches the record's rectangle
  across a textured quad; a class-0 window adds its eight frame tiles. Every
  piece is drawn through the record's palette byte. The portrait records
  sample the next page and are not part of the map.
- **Which cell.** The palette byte decodes to a CLUT cell
  ([`clut_fb`](../../crates/asset/src/ui_widgets.rs)): bit 6 clear is
  `CBA = 0x7FC0 + (b & 0x3F)`, VRAM `(16 * (b & 0x3F), 511)` - "sub-palette
  `b & 0x3F`"; bit 6 set is the 4x4 badge block at `(896.., 498..501)`. The
  sheet's 16x16 CLUT block lands flattened on row 511, so sub-palette `k`
  below 16 **is** the sheet file's palette `k`, and all sixteen survive the
  boot upload unchanged. Sub-palettes 16-18 are the three rows of the
  CLUT-only TIM at `0x1858`; sub-palette 19 is the button-glyph TIM's own.
- **Overlaps.** Where two records sample the same texel, the attributed
  palette follows the draw path: single sprites (class 5), then plate runs
  (3), bars (4), framed windows (0), the rest; within a path the smaller
  rectangle, then the lower record id. Such texels are really drawn in more
  than one colour - the bar record `0x06` stretches a 16x16 cut of the Curse
  badge's texels through sub-palette 5 while the badge itself draws them
  through 13 - and the kernel counts them (`contested`).
- **Covered texels.** The 64x32 button-glyph TIM at `PROT.DAT` `0x7B00`
  uploads its pixels over sheet texels `(128, 96)..(191, 127)`; the records
  that draw there (`0x37..=0x3E`, sub-palette 19) draw the glyphs. The kernel
  leaves that rectangle out of the map and reports it; the viewer's composite
  shows the glyph TIM there, and the texture editor points at that TIM.
- **Unplaced texels.** Art no record samples has no palette the disc names;
  overlay code may still draw it. The viewer dims it, the editor maps it to
  palette 0.

The viewer reads each cell's colours out of the boot VRAM, so a palette a
later boot upload overwrote shows as the game has it; it also lists the
palettes VRAM holds on a texture's CLUT row and says when a texture's own
palette is overwritten at boot or is all zeros on disc. The editing side of
the same map - download shapes, what an edit rewrites - is in
[`textures-and-fonts.md`](../tooling/translation/textures-and-fonts.md#multi-palette-textures).

## Step 4: the colour word - why raw textures look darker or brighter

Every TMD primitive carries a colour word `[R][G][B][GP0 code]` - one per
primitive for flat (`F`) primitives, one per corner for gouraud (`G`) ones -
and the GPU multiplies the texel by it:

```text
out = texel * colour / 128        (per channel, clamped to 255)
```

`0x80` is neutral. Below it darkens, above it brightens, up to `0xFF`, about
2x. The colours are **baked** by the artists into each mesh: across the field
scenes' environment packs roughly four in five colour components sit below
`0x80`, so most field surfaces draw **darker than their texture file**, and a
minority draw brighter. A texel of `(200, 100, 50)` under a colour word of
`0x60` reaches the screen as `(150, 75, 37)`; under `0xC0` it clamps to
`(255, 150, 75)`. An untextured primitive has no texel and is filled with the
colour directly.

The exception is the **light-source rows** (TMD group flags `0x10..=0x17`).
They carry no colour word; the per-prim dispatcher `FUN_80043390` sends them to
its `NCCS` / `NCCT` handlers, which compute the colour word from the GTE light
against each face's normal: the scene's back colour (`_DAT_8007B788`, `0x20`
per channel unless op `4C 8A` sets another) plus the first light's intensity,
times the object colour. Under the scene-load light a face turned away keeps
an eighth of its texel and a face square to the light a little over the whole
of it. `cave01`'s rock walls are all such rows; most scene packs hold few or
none. The evidence and the formula are in
[`renderer.md`](renderer.md#the-light-source-rows).

2D sprites ride the same rule. The widget-sprite emitters stamp the packet
word `0x64808080` / `0x66808080` (`FUN_8002C488` at `0x8002C4C0` and
`0x8002C5C4..0x8002C5CC`): colour `0x808080`, the neutral multiply, so menu and
HUD sprites show their palette colours exactly - which is why a UI sheet,
decoded through the right palette, matches the screen while a field texture
does not.

## Step 5: the depth cue and fog

Before the colour word reaches the GPU, the GTE's `DPCS` / `DPCT` ops blend it
toward the far colour `FC` by the interpolation factor `IR0` (0..4096):

```text
colour' = colour + (FC - colour) * IR0 / 4096
```

On an unfogged field scene `IR0 = 0` and the cue is the identity (a retail
town capture's GTE registers show the baked corner colours passing through
unchanged). Scenes that do use it:

- **The opening prologue** stages a view-depth `IR0` ramp that crushes far
  blue ([`renderer.md`](renderer.md#full-scene-colour-grade),
  [`cutscene.md`](cutscene.md#full-scene-sepia-grade-the-gold-prologue-look)).
- **The world map** reads a per-Z fog LUT out of `SCUS_942.54`
  ([`world-overview-viewer.md`](world-overview-viewer.md)).
- **Battles** cue the procedural ground grid and every actor
  (next section).

Because the cue runs on the colour word and the multiply comes after, a
textured primitive's far colour is `texel * FC / 128`, not `FC`.

## Battle: the ambient ramp and the actor tint

Battles add two runtime colour sources on top of the baked words.

- **The ambient.** `FUN_80050120` ramps a packed battle ambient `ctx+0x890`
  every frame and stores it plus `0x404040` into `0x8007B7B0`, which
  `FUN_80026CE4` copies into the GTE `RGBC` the ground grid is cued from. At
  rest the grid's near colour is `0xC0` per channel; a summon close-up
  latches `ctx+0x243` and pulls it down toward `0x60`, then it climbs back
  after the creature leaves. Every fight's floor fades in over its first 48
  vsyncs. Detail and addresses:
  [`battle.md`](battle.md#the-grids-near-colour-and-cue-depth).
- **The actor tint.** `FUN_8004A908` writes each battle body's colour word
  `+0x74` and cue weight `+0x78` every frame: far bodies are pushed toward a
  darker copy of themselves (the distance fade), status ailments tint the
  body (`0xFF2020`, `0xF020F0`, ...), and a rotted limb draws with red and
  green quartered and blue halved
  ([`battle.md`](battle.md#the-distance-fade),
  [`renderer.md`](renderer.md#rotted-limbs-draw-dark)).

## Palettes that change at runtime

Some palettes are rewritten while the game runs, so a texture export - which
shows the disc state - cannot match every frame:

| Mechanism | What moves | Where documented |
|---|---|---|
| CLUT walk (`MoveImage` of park strips) | water, waterfalls, the world-map ocean | [`field-ambient-fx.md`](field-ambient-fx.md#mechanism-1---the-scene-walker-table-bundle-type-6-slot) |
| HSV cycler `FUN_80019D50` | "pulsating flesh", lightning flashes | [`field-ambient-fx.md`](field-ambient-fx.md#the-clut-cell-hsv-cycler-the-pulsating-flesh) |
| Scripted CLUT family (field-VM `4C 61`) | fades and one-shot palette stamps | [`world-map.md`](world-map.md) |
| Prologue palette collapse | every uploaded CLUT rewritten to gold sepia | [`renderer.md`](renderer.md#full-scene-colour-grade) |

A palette that looks like a strip of unrelated colours, or a run of nearly
identical ramps, is often a **park strip**: frames the CLUT walk copies into
the live cell one at a time.

## Step 6: semi-transparency

A primitive whose ABE bit is set blends with the framebuffer. For a
**textured** primitive the choice is per texel: palette entries with STP set
blend, entries without it draw opaque, `0x0000` never draws. An untextured
ABE primitive blends every pixel. The four modes come from the texpage `ABR`
bits (`B` = framebuffer, `F` = the new pixel):

| ABR | Equation |
|---|---|
| 0 | `0.5*B + 0.5*F` (the dialog-box fill uses this) |
| 1 | `B + F` |
| 2 | `B - F` |
| 3 | `B + 0.25*F` |

A PNG export cannot express this; the viewer notes when a palette carries STP
entries. Port detail: [`renderer.md`](renderer.md#set_semi_blend---semi-transparency-blend-modes).

## Step 7: dither and the 15-bit framebuffer

The framebuffer is 15-bit. Retail boots with dithering **on** (`FUN_8001D424`
writes `1` to `_DAT_8007BA66`, and the frame-begin driver `FUN_80016B6C`
copies it into the DRAWENV `dtd` bit every frame), and a field-VM opcode can
change it per scene. With dither on, a 4x4 ordered offset is added before each
channel is cut to 5 bits, which is where the fine cross-hatch on gradients
comes from. The port renders clean by default and reproduces the retail
dither with `set_psx_mode`
([`renderer.md`](renderer.md#retails-dither-law-stated-separately-from-the-ports-default)).

## For modders: editing a texture

- **Edit indices, not a single palette's colours.** On a multi-palette sheet
  the same index means a different colour in each palette. Repainting a
  region in the colours of palette 0 changes every other palette's reading
  of it too. Check the region in the viewer's **As the game draws it** view
  (UI sheets) or in the palette its model uses before you edit.
- **Keep the palette count and the transparency key.** Colour `0x0000` is
  "not drawn" everywhere; use `0x8000` for black
  ([`tim.md`](../formats/tim.md#alpha---stp-mapping) has the encoder's alpha
  rules).
- **Don't pre-brighten a texture that looks dark on screen.** The darkness
  is the colour word (`< 0x80`) or the depth cue, applied on top of the file.
  A brighter texture brightens every primitive that uses it.
- **All-zero palettes are placeholders.** Their VRAM cells are filled by
  another texture of the same scene; recolouring them in the file changes
  nothing unless that other texture is left out.
- **Animated palettes overwrite the file's colours** in the cells they walk.

Replacing a texture on the disc: [`randomizer.md`](../tooling/randomizer.md#texture-replacement)
and [`textures-and-fonts.md`](../tooling/translation/textures-and-fonts.md).

## Port

| Chain step | Port |
|---|---|
| Palette resolution for exports | `legaia_asset::tim_palette_context` (`texel_palettes`: which cell draws which texel; `BootClutVram`: what the cells hold), `legaia_tim::decode_rgba8_with_palette` |
| VRAM + CLUT decode | `engine-render` VRAM pipeline (1024x512 R16Uint, CLUT decode in the fragment shader); `legaia_tim::Vram` |
| Colour word + depth cue | `psx_modulate` / `psx_depth_cue` in the shader prelude, CPU mirror `legaia_engine_render::psx_light` |
| Battle ambient | `legaia_engine_vm::battle_ground_grid::ambient_base_step`, `World::tick_battle_ambient` |
| Actor tint | `legaia_engine_vm::battle_actor_tint`, `battle_actor_draw` |
| Animated palettes | `engine-core::clut_cell_fx`, `World::step_clut_fx`, `legaia_asset::clut_walk` |
| Blend + dither | `psx_blend`, `psx_dither` (`Renderer::set_semi_blend`, `set_psx_mode`) |
