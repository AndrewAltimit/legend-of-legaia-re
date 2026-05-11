# NPC CLUTs at VRAM row 479

PSX field/town NPC TMDs commonly sample CLUT cells along row 479. The
data is **plain PSX TIMs** sitting inside the scene's PROT entries,
uploaded to VRAM by the standard asset-dispatch path (no special
"hue-ramp generator" function exists).

## Layout

Each contributing TIM has a CLUT block with `(fb_x, fb_y, w, h) = (0,
479, 256, 1)` — a 256-color row spanning fb_x=0..256, which carves
into sixteen 16-color slots (slot N at fb_x = N*16..N*16+16). Field
NPC TMDs sample these slots via CBA cells `0x77C0..0x77CF`.

The actual contents are **scene-specific**: each town/area embeds its
own row-479 TIMs with the NPC palettes for that scene.

## How the TIMs sit on disc

Within a scene's PROT entries (e.g. `0006_town01.BIN`), each row-479
TIM is preceded by a 4-byte chunk-header prefix in the asset
descriptor stream, then the standard PSX TIM:

```
+0x00: u32 chunk header  (e.g. 0x01008220 = type 0x20)
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

`legaia_asset::tim_scan` detects these via the inner TIM magic at
offset +4 from the chunk header; it does not need to interpret the
type-0x20 wrapper.

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

The exact retail dispatch order (and which subset of the row-479 TIMs
get uploaded for a given scene-mode) is not yet pinned. Empirically,
mednafen save states captured mid-tutorial-battle in town01 hold the
specific bytes from `0006_town01 @ 0x1ee4c` at row 479 — meaning the
retail engine ends up with one specific "full" variant. The merge
strategy in the engine port produces a functionally equivalent
fully-populated row but not necessarily byte-identical to retail.

## Cross-save corroboration

The `mednafen-state vram-dump` CLI extracts the raw 1 MiB VRAM blob.
Row 479 starts at byte offset `0xEF800` (= `479 * 2048`). Slicing 32
bytes at `0xEF800 + slot * 32` gives one CLUT slot. See
[`docs/tooling/mednafen-automation.md`](../tooling/mednafen-automation.md).
