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
arms read one band each. That table is one of **two** in the routine, and the
`beqz` on the special-battle word `_DAT_8007BAC0` at `0x801F6BA8` is what picks
between them - a zero word takes the other table (`0x801F6AD8`, the field
texture-page restore), so these arms only run inside a dome or special battle.
The rect lives at `0x801F735C` and only its `y` is rewritten between bands:

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

## What draws it

The upload puts the still in VRAM at `(384, 0)`; what puts it on screen is two
textured quads in the contest hub, PROT `0977` (`arena_init`, slot-A base
`0x801CE818`), emitted by `FUN_801D00F8` at file `+0x18E0`.

That routine is a fork. `_DAT_801D1AE0` - zeroed in the hub's init at
`0x801CEB54`, stored non-zero at `0x801CEC04` - selects between rebuilding the
hub's live scene (six tiled prims through `FUN_801D08EC`, then a 320x240 fill)
and re-presenting the still. On the still arm it writes two `POLY_FT4`
primitives (GPU code `0x2C`, tag length 9 words) into the ordering table whose
base it takes from the scratchpad block at `0x1F800314` (`+0xE0` = OT,
`+0x8C` = the running primitive cursor), one `jal 0x8003D2C4` each:

| | tpage | screen `(x0,y0)-(x3,y3)` | `u` span | VRAM `x` |
|---|---|---|---|---|
| quad 1 | `0x106` | `(0,-20) - (192,220)` | `0..192` | `384..576` |
| quad 2 | `0x109` | `(192,-20) - (320,220)` | `0..128` | `576..704` |

Both carry `v` `0..240`, so the pair is one 320x240 image split down the
middle, and between them they sample VRAM `(384, 0)..(704, 240)` - the still's
own rectangle, minus the bottom 16 rows the upload writes and the draw does
not read. The screen `y` span is `-20..220` rather than `0..240`; where that
lands is the draw environment's offset, which this routine does not set.

The routine's `a0` is clamped to `0..0xFF` and broadcast into all three colour
bytes of each primitive's `code+rgb` word, so the caller's
`*(0x801D1A7C)` is a **fade level** on the still rather than a selector; the
call is skipped entirely when that word is zero.

### Measured live

`scripts/pcsx-redux/autorun_w3b_dome_still.lua` taps the emitter entry, both
fork arms and the two packet-fill sites (at each of which `a1` still holds
that packet's base, so the `0x28` bytes are read straight out of the pool),
and gates every hit on the image's own word at `0x801D00F8` so a run before
`0977` pages in reports nothing rather than counting a stranger's code.

On `minigame_muscle_dome_pcsx` the emitter is entered 79 times over 3600
vsyncs, **every one on the six-tile arm**: `_DAT_801D1AE0` is zero for the
whole first arena visit, so the still arm never runs and no primitive in the
frame carries a 15-bit texture page. The one catalogued save with `0977`
resident (`minigame_muscle_dome`) is in the same position - fork zero, fade
`0x80` - and its ordering table decodes with no `tp = 2` family at all. The
still is not a first-visit draw.

Forcing the latch to `1` at the entry tap and letting the same run continue
gives the arm 190 entries, and every one emits exactly the two packets this
page describes: tpage `0x106` at screen `(0,-20)-(192,220)` and `0x109` at
`(192,-20)-(320,220)`, `code+rgb` words `0x2C` with the fade byte in all
three colour lanes (`0x2C040404` up to `0x2C808080` as the level ramps), and
one caller throughout - `ra = 0x801D00B4`, the hub's `jal` at `0x801D00AC`.

### On a natural re-entry

The forced-latch run above is not needed to see the arm: the checkpoints of
`autorun_muscle_hud_capture.lua` (a dome contest played forward through three
hub visits) already hold it. At the first mode-`0x19` checkpoint of the second
and third visits the latch reads `1`, the hub arm `*(0x801D1A78)` reads `0x0A`,
the backdrop level reads `8` in lockstep with the heading level
`*(0x801D1A84)`, and the primitive pool holds both packets with code word
`0x2C080808`; the first visit's checkpoints read latch `0`, and by the next
fight's mode-`0x14` checkpoint the arm is `0x16` and the level `0`.

So the still is the backdrop of every re-entered hub, and the level it is
drawn at is the hub's `*(0x801D1A7C)` across six arms of `FUN_801CF870`
(the re-entry init seeds arm `0x0A` at `0x801CEE2C` and zeroes the level at
`0x801CECD0`):

| arm | level | ends when |
|---|---|---|
| `0x0A` | `+= 4 dt` to `0x80` (`0x801CFCF8`) | the INTERVAL heading is at full |
| `0x0B` | `-= 2 dt` to `0x40` while tally lane 0 is at its clamp (`0x801CFD84..0x801CFDB8`) | the tally has stopped and the level is `0x40` |
| `0x0C` | held at `0x40` | the heading has drained |
| `0x14` | `+= 4 dt` to `0x80` (`0x801CFEF4`), on the latch-`1` arm only | the level is full |
| `0x15` | held | the ROUND banner's fade-in and hold |
| `0x16` | `-= 2 dt` to `0` beside the banner (`0x801D002C`) | the banner has drained; the next fight starts |

### When the latch is raised

`_DAT_801D1AE0` is not written by the match teardown. It is written by the
arena init `FUN_801CEA6C`, which tests the arena word `_DAT_8007BAC0`
(`lw v1, -0x4540(s1)`, `s1 = 0x80080000`) and forks: on zero - the state
field-VM op `0x3E` leaves when it warps into the minigame - it stores zero to
the latch at `0x801CEB54` and `1` to the arena word; on non-zero it stores
`s2 = 1` to the latch at `0x801CEC04`. So the still is a **re-entry** draw:
the hub presents it when a finished round returns to a hub that has already
been initialised once.

`*(0x801D1A7C)` is a countdown as well as a level. The hub tail at
`0x801D002C..0x801D0040` subtracts `2 x *(0x1F800393)` from it each frame and
clamps at zero, and the call site guards on it (`beqz a0, 0x801D00B8` at
`0x801D00A4`), so the still fades out over a bounded number of frames rather
than holding for the whole hub screen.

### The first visit's arm

With the latch zero the same emitter draws something else
(`0x801D0148..0x801D01B4`): it sets the OT depth `*(0x801D1AA8) = 0x3E8`,
tiles sprite-table record `2` - a brick wall - as a `3 x 2` grid through the
corner-anchored emitter `FUN_801D08EC` at `(i << 7, j << 7)`, scale `0x1000`,
at the same level, and ends with a full-screen `0x3A` gouraud quad through
`FUN_801D1610(0, 0, 0x140, 0xF0)` at OT slot `0x384`, top corners shaded
`0x64`, bottom corners `0`. The level that arm runs at comes from the hub's
first-visit arms (jump table `0x801CE990`):

| arm | level `*(0x801D1A7C)` | what else runs |
|---|---|---|
| `0` / `1` | `0` | the intro card fades in at `4 dt` and holds `0x7B` ticks |
| `2` | `+= 4 dt` to `0x80` (`0x801CF9E4..0x801CFA2C`) | the intro card fades out at the same `4 dt` |
| `3` | held | the title art zooms from scale `0x1640` to `0x1000` |
| `4` | held | the course card `FUN_801D042C` (records `5 + course` and `8`) fades in at `2 dt` |
| `5` | held | a `0xB4`-tick hold, ended early by a `0xF4` pad bit; the course card clears on exit |
| `6` | `-= 4 dt` to `0` (`0x801CFC48..0x801CFC78`) | nothing else draws; at zero the battle load is kicked and the arm becomes `0x14` |

The `shot_00500_m19` frame of the `captures/w1e/dome_vram` run is arm `4` or
`5`: the wall behind "Muscle Dome!" and the course name.

### Why a search for the rectangle did not find it

Three consumers were excluded earlier on the finding that 33 sites disc-wide
materialise `0x180` and none pairs it with `y = 0`. Both halves are true and
the conclusion does not follow: **this emitter never materialises 384**. A
textured primitive addresses VRAM through the packed `tpage` halfword, where
the x coordinate is a *page index* - `384 / 64 = 6`, `576 / 64 = 9` - so the
constants in the code are `0x106` and `0x109`, the `0x100` being `tp = 2`
(16-bit direct colour). Searching the disc for those five words instead
(`0x106..0x10A`, pages 6..10 at page-y 0) leaves one image holding more than
one of them, and it is this one.

## In the port

Both play hosts draw it. The pick runs at the port's battle end for a dome
leg, `World::exit_muscle_dome`, over the fighter's live HP and the lead
record's `+0x11C` maximum (`legaia_engine_core::muscle_ringside::still_prot_index`,
through the loader port's `backread_texture_variant`), and the world keeps the
answer as `MinigameState::muscle_ringside_still`. When a finished leg raises the
INTERVAL screen, each host arms a `muscle_ringside::HubBackdrop` - the level
table above, riding the host's INTERVAL envelope for arms `0x0A..0x0C` and
running `0x14..0x16` itself - and draws
`legaia_engine_ui::ringside_backdrop::ringside_still_quads` at its level,
first in the hub's list, then the ROUND banner over it on arms `0x15` /
`0x16`. The quads resolve onto the still's sheet **through their texture
pages** (`StillDraw::from_quad`), and the sheet is laid down band by band at
the loader's own rects (`still_sheet_rgba` over `backread_slice_rect`): the
native window bakes both stills into its hub atlas, and the play page serves
them as hub sheet `8`. The frame-sliced read schedule itself is not ported as a
schedule - the port reads the whole entry at once.

The pick reads the lead record the way the loader does: the dome door warp
seeds the lead fighter from the record's live HP `+0x106` (and its maximum
`+0x104`), and `World::exit_muscle_dome` writes the fight's HP back into
`+0x106` before it picks `+0x106 < +0x11C / 2`. The warp's opponent is the
contest's own ladder rung - the arena overlay's `(course, round)` naming an
ordinary PROT 867 record - so a real lead meets a real rung rather than a
400-HP stand-in.

The first visit's wall is drawn too, by both play hosts
(`legaia_engine_ui::ringside_backdrop::first_visit_tile_draws`), under the
two screens the port stages when a leg opens on a fresh contest. Its level is
`muscle_ringside::first_visit_backdrop_level`: `0x80` less the intro card
while the card fades out (arm `2`), `0x80` while the leg-open banner is up,
and the banner's own level while it fades out - which works because that
banner runs arms `4` / `5` / `6`'s envelope exactly. Three differences
remain, and are disclosed rather than hidden: the banner draws the ROUND
card where retail's arms `3..5` draw the title art and the course card, the
arm's closing gouraud quad is not drawn (it is semi-transparent, and its
blend is set by a draw-mode word the hub's sprite path has no seat for), and
the standalone minigames page draws neither arm
([`host-drift.md`](../tooling/host-drift.md#ringside-still-on-the-standalone-dome-page)).

The ROUND card itself runs once per leg, as retail's arms `0x15` / `0x16`
run it once. A re-entered hub plays it over the still, and the leg the player
then walks into through the dome door no longer raises it a second time
(`muscle_ringside::leg_open_raises_round_card`).

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
- [`renderer.md`](../subsystems/renderer.md) - the primitive vocabulary the two
  quads above are written in.
- [`byte-accounting.md`](../tooling/byte-accounting.md) - `asset account 1221`
  credits the four bands.
