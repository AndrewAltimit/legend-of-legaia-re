# Headerless 16bpp stills - `int.tim` / `int2.tim`

Extraction PROT `1221` and `1222` are each exactly `0x28000` bytes of raw
15-bit BGR555 with **no TIM header**. The rectangle that gives them a shape is
not in the file - it is four immediates in the routine that uploads them, so
this page exists to write those immediates down. Parser
`legaia_asset::ringside_still`.

The behavioural side - what the stills are for, which module owns them, and the
rest of that bundle's slots - is on
[`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md#inttim--int2tim---the-ringside-panel-stills).

## Layout

```
+0x00000 .. 0x0A000   band 0   20 sectors   320 x 64 pixels, 16bpp
+0x0A000 .. 0x14000   band 1
+0x14000 .. 0x1E000   band 2
+0x1E000 .. 0x28000   band 3
```

One band is `320 * 64 * 2 = 0xA000` bytes, which is also 20 sectors exactly, so
the two statements of the band size are independent and they agree. Four bands
tile the entry with nothing left over, and pixels run row-major inside a band,
so the whole entry is one 320 x 256 image.

| Field | Value | Where it comes from |
|---|---|---|
| width | 320 (`0x140`) | `rect.w` immediate |
| height | 256 = 4 x 64 | four uploads of `rect.h` = `0x40` |
| VRAM x | 384 (`0x180`) | `rect.x` immediate |
| VRAM y | 0, 64, 128, 192 | the per-band `rect.y` store |
| pixel | BGR555, STP clear | the residue classifier's `bgr555` test |

## What names the rectangle

`FUN_801F6B24` in the PROT `0978` `field_back_read` image (slot-B base
`0x801F69D8`) is a phase machine on a jump table at `0x801F6AA8`; four of its
arms read one band each. The rect lives at `0x801F735C` and only its `y` is
rewritten between bands:

```text
801f6be4  li   v0,0x180         ; rect.x = 384
801f6bec  sh   v0,0x735c(at)
801f6bf0  li   v0,0x140         ; rect.w = 320
801f6bf8  sh   v0,0x7360(at)
801f6bfc  li   v0,0x40          ; rect.h = 64
801f6c04  sh   v0,0x7362(at)
801f6c20  sh   zero,0x735e(at)  ; rect.y = 0, then 0x40 / 0x80 / 0xC0
```

The read is `FUN_8003E964(sector, 0)` seeking `0 / 0x14 / 0x28 / 0x3C`, then
`FUN_8003E800(dst, 0x14, 1)` for 20 sectors; the upload is `FUN_800583C8`,
which is `LoadImage` (it hands the literal at `0x800156D4` to the debug hook
before the libgpu vtable call). See
`ghidra/scripts/funcs/overlay_field_back_read_0978_801f6b24.txt`.

## Which entry is which

The PROT index is **computed**, not a literal, which is why a literal-only
reference sweep finds no loader for either entry:

```text
801f6b90  lhu  v0,0x4824(v0)    ; party slot 0 hp_max_record  (record +0x11C)
801f6b98  lhu  v1,0x480e(v1)    ; party slot 0 hp_curr_live   (record +0x106)
801f6ba4  srl  v0,v0,0x1
801f6bac  sltu s0,v1,v0         ; s0 = current HP < max / 2
801f6c3c  addiu a0,s0,0x4c7     ; raw TOC 0x4C7 + s0
801f6c40  jal  0x8003e8a8       ; the LBA resolver
```

Raw TOC `0x4C7` / `0x4C8` are extraction `1221` / `1222` under the
[+2 correction](cdname.md#numbering-space), and the same `s0` picks between the
two dev path strings on the dev-file branch, which is what ties each index to a
filename: `1221` is `int.tim`, `1222` is `int2.tim`, and `int2.tim` is the
**below-half-HP** variant. The record offsets are the live ones in
[`save-record.md`](save-record.md).

Two details of that sequence are the kind that a backward-only or literal-only
scan loses: the index is formed by `addiu` **in the `jal`'s delay slot**, and
the comparison reads the character record through `lhu` at a `gp`-free absolute
pair rather than through any table.

## Why it has no magic

Nothing in the entry identifies it. Length is the only structural statement it
makes, and `0x28000` is not distinctive on its own, so
`legaia_asset::ringside_still::has_still_shape` is a length test and the
selection is by PROT index. That is not a shortcut around a detector - there is
nothing to detect. A still and any other raw 16bpp VRAM region are the same
bytes.

## See also

- [`tim.md`](tim.md) - the headered form the same pixels take everywhere else.
- [`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md) - the
  owning module and the rest of its bundle.
- [`byte-accounting.md`](../tooling/byte-accounting.md) - `asset account 1221`
  credits the four bands.
