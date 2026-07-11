# NPC CLUTs at VRAM row 479

PSX field/town NPC TMDs commonly sample CLUT cells along row 479. The
data is **plain PSX TIMs** sitting inside the scene's PROT entries,
uploaded to VRAM by the [`FUN_8001FE70`](../subsystems/asset-loader.md)
battle-init walker (no special "hue-ramp generator" function exists).

## Layout

Each contributing TIM has a CLUT block with `(fb_x, fb_y, w, h) = (0,
479, 256, 1)` - a 256-color row spanning fb_x=0..256, which carves
into sixteen 16-color slots (slot N at fb_x = N*16..N*16+16). Field
NPC TMDs sample these slots via CBA cells `0x77C0..0x77CF`.

The actual contents are **scene-specific**: each town/area embeds its
own row-479 TIMs with the NPC palettes for that scene.

## How the TIMs sit on disc

Within a scene's [`scene_tmd_stream`](scene-bundles.md) PROT entries
(e.g. `0006_town01.BIN`), each row-479 TIM lives inside a type-`0x01`
streaming chunk in the entry's tail. The chunk header is a `(type <<
24) | size` u32:

```
+0x00: u32 chunk header  bytes 20 82 00 01 -> LE u32 0x01008220
                         type byte = 0x01 (high byte) -> "upload TIM"
                         payload size = 0x008220 (low 24 bits) = 33312
+0x04: u32 TIM magic     0x00000010
+0x08: u32 TIM flags     0x00000008 (4bpp + CLUT)
+0x0C: u32 block size    0x0000020C  (CLUT block: 12 hdr + 512 data)
+0x10: u16 fb_x = 0
+0x12: u16 fb_y = 479
+0x14: u16 num_colors = 256
+0x16: u16 num_cluts = 1
+0x18: 512 bytes of CLUT data (256 BGR555 halfwords)
+...:  standard TIM image block (typically a 256×256 4bpp at fb_x=832)
```

The `0x20` leading byte (file order) is the **low byte of the chunk
size field**, not a type. The type byte in `FUN_8001FE70`'s walker
convention is the **high byte of the LE u32** (= `0x01`). This is
the same byte-packing the standard asset-type dispatcher uses (see
[`asset-type.md`](asset-type.md)), but `FUN_8001FE70` gives `type =
0x01` a different semantic than `FUN_8001F05C` does - here it means
"upload payload as a single PSX TIM via `LoadImage`", not
"`TIM_LIST` pack". Calling the standard `FUN_8002541C` streaming
walker on these chunks would dispatch to `FUN_8001F05C` case 1 and
attempt to parse the payload as a `[count + offsets + TIMs]` pack,
which fails (the first payload u32 is the TIM magic `0x10` = 16
which would be read as a 16-entry pack count).

`legaia_asset::tim_scan` detects these via the inner TIM magic at
offset +4 from the chunk header; it does not need to interpret the
`FUN_8001FE70` wrapper. The structured walker that *does* recognise
the wrapper is
[`scene_tmd_stream::battle_tim_chunks`](../../crates/asset/src/scene_tmd_stream.rs)
- it reports every type-0x01 chunk and tags whether the chunk sits
inside `FUN_8001FE70`'s reach (`WalkSource::Tail`) or past the first
terminator in a continuation list (`WalkSource::Continuation`).

## Multi-TIM CLUT merge

Each town typically has **multiple** row-479 TIMs spread across several
PROT entries (e.g. town01 entries 6..9 carry 7 such TIMs). Some are
"full" (slots 0..14 populated), others are "partial" (slots 0..7 only,
remaining slots padded with 0x0000 on disc). All target the same VRAM
cells, producing a CLUT race.

The engine's targeted-upload CLUT pass at
[`legaia_tmd::vram_targeted::build_vram_targeted_from_buffers`](../../crates/tmd/src/vram_targeted.rs)
runs the CLUT block second (after image blocks) and uses
**merge-zeros semantics**: a halfword of `0x0000` in a later upload
does not overwrite a non-zero halfword from an earlier upload. The
net effect is the union of every contributing TIM's non-zero slots,
which yields a fully populated palette row.

Without merge semantics, the partial TIMs' trailing zeros clobber the
full TIMs' slots 8..14 and the town01 prim keep-ratio collapses from
99.3% to 78.6% (the four "field intersection" NPC TMDs lose their
palette anchor).

## What retail's dispatcher does instead

The retail engine uploads these TIMs only during **battle init**, via
the `FUN_800520F0` → `FUN_8001FA88` → `FUN_8001FE70` chain. The
field / town scene loader does NOT touch them. Empirically:

- mednafen captures inside town01 (no battle entered yet) have VRAM
  row 479 fb_x=0..256 entirely zero.
- mednafen captures mid-battle (or post-battle, since PSX VRAM is
  persistent across scene transitions) have row 479 populated.

`FUN_8001FE70` walks the streaming tail until it hits either a
zero-size chunk header or a type-0x02 chunk; for every type-0x01
chunk along the way it calls `LoadImage(payload)` to DMA the TIM to
VRAM. The walker stops at the first terminator. Files with the
two-list shape (`0006_town01.BIN` has chunks at `0x3840`, `0xba64`,
then a zero-padded gap, then `0x16c24`, `0x1ee48`) leave the
continuation list past the terminator unreached by the standard
battle-init dispatch. Whether a separate code path picks them up
later is not pinned; the bytes are reachable as
`WalkSource::Continuation` in the engine helper, and the in-tail
chunks alone supply the same `(fb_y, fb_x)` regions, so the
continuation list may be cold-loaded only by an alternate scenario
(e.g. NPC variants seen in specific scripted events).

## Engine port: field-mode vs battle-mode dispatch

`SceneResources::build_targeted_with_options(..., kind:
SceneLoadKind::Field)` mimics retail's lazy upload by excluding
every `scene_tmd_stream` PROT entry's contributions entirely - both
the leading TMD (`FUN_8001FE70` writes it to the battle character
TMD register `_DAT_8007B864`, never drawn from a field scene) and
its type-0x01 TIM chunks (which upload the CLUTs and textures that
same mesh samples). With both filtered out the field-mode VRAM
matches retail town saves (row 479 fb_x=0..256 = zero) and the
parsed TMD pool excludes the battle character meshes that would
otherwise fail the prim filter en masse for sampling missing CLUT
rows. `SceneHost::enter_field_scene` uses `Field` as its kind so
the engine port matches the retail dispatch boundary.

`SceneLoadKind::Battle` (the legacy default of `build_targeted`)
uploads every type-0x01 chunk eagerly and parses every embedded
TMD, which inflates VRAM compared to retail's field state but
keeps every battle character mesh renderable for tests + diagnostic
surfaces. The town01 keep ratio is 99.3% in battle mode and 100%
(0/0; battle character meshes excluded) in field mode under
disc-gated regression tests.

## Cross-save corroboration

The `mednafen-state vram-dump` CLI extracts the raw 1 MiB VRAM blob.
Row 479 starts at byte offset `0xEF800` (= `479 * 2048`). Slicing 32
bytes at `0xEF800 + slot * 32` gives one CLUT slot. See
[`docs/tooling/mednafen-automation.md`](../tooling/mednafen-automation.md).

## Boot-resident strip band (rows 510/511)

Rows **510 and 511** look superficially like another NPC-palette band
but are a **different mechanism entirely**: they hold the flat-strip
CLUTs of the boot-resident **system-UI TIM bundle**, not scene data.
The bundle is the `prot::timpack` at **raw PROT TOC entry 0**
(CDNAME `init_data`; on-disc `PROT.DAT` sectors 3..55 - the region the
extraction index space skips, since extraction entry 0 is raw entry 2;
a second single-TIM pack sits at raw entry 1). The retail per-TIM
uploader `FUN_800198E0` uploads **every** TIM's CLUT block as a
`w*h × 1` strip at the block's declared origin (the rect
`(clut_x, clut_y, w*h, 1)` is built inline before the `LoadImage`
wrapper `FUN_800583C8`; `see ghidra/scripts/funcs/800198e0.txt`), so
the bundle's declared multi-row CLUT banks land as:

| Row | fb_x | Source TIM (PROT.DAT offset) | Declared CLUT rect |
|---|---|---|---|
| 510 | 0..255 | menu-glyph / interior-page atlas `0x11218` (image `(960,256)` 64×256) | `(0,510,16,16)` |
| 510 | 256..319 | UI sprite strip `0x19438` (image `(960,400)` 60×24) | `(256,510,16,4)` |
| 511 | 0..255 | system-UI sprite sheet `0x018E0` (image `(896,256)` 64×192) | `(0,511,16,16)` |
| 511 | 256..303 | boot cursor `0x01858` | `(256,511,16,3)` |
| 511 | 304..319 | UI element `0x07B00` | `(304,511,16,1)` |

Every strip is **byte-identical to the on-disc CLUT data across
captured save states in every phase** (title, opening cutscene, town
field, dungeon, house interior, world map, battle) - uploaded once at
boot, never evicted, never scene-touched. This is why field/dungeon
env-pack meshes can reference CBA cells on row 510 (`town01` env slots
21/26/74, `rikuroa` slots 50/51/63: CBA `(64,510)` = atlas strip
entries 64..79, texpage `(960,256)`) even though **no scene TIM ever
uploads that row**. Parser `legaia_asset::system_ui_bundle` reads both
raw-entry packs straight out of `PROT.DAT` and reproduces the full
upload (`legaia_asset::interior_page` remains the atlas-only reader).
The pack's six 532-byte non-TIM members are bare **row patches** -
`[u32, u32]` preamble + TIM-style `[u32 bnum][u16 x,y,w,h]` blocks
declaring `(960, 456..458/460..462, 256, 1)` - that overlay the atlas
image rows in place (byte-exact vs live captures in every phase).

Contrast with row 479 above: row 479 is per-scene content raced
through the merge-zeros targeted pass at battle init; rows 510/511 are
global boot residue. A parity oracle must treat them accordingly
(row 479 = scene/battle-history-dependent, rows 510/511 fb_x 0..319 =
static disc bytes).

## See also

- [PSX TIM](tim.md) - the plain TIM form these CLUTs ship as.
- [`subsystems/renderer.md`](../subsystems/renderer.md) - the targeted-upload CLUT pass with merge-zeros semantics.
- [Scene bundles](scene-bundles.md) - the scene PROT entries that carry these palettes.
