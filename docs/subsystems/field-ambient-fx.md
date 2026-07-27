# Field ambient animation - the moving parts of a "static" scene

Field maps are not static: water shimmers, waterfalls roll, and jou's fused
Juggernaut ground pulses under lightning. None of that is vertex data in the
environment pack - it is three runtime mechanisms layered over the scene's
VRAM and mesh pool, all disc-authored, all per-scene.

| Mechanism | Carrier | Stepper | What it animates |
|---|---|---|---|
| [Walker table](#mechanism-1---the-scene-walker-table-bundle-type-6-slot) | Scene bundle type-6 slot | `FUN_8001ADA4` case 0xB | CLUT-cell `MoveImage` cycling: water / waterfall shimmer |
| [Ambient move-VM tree](#mechanism-2---the-ambient-move-vm-effect-tree) | Prescript stager bundle + MAN P1 effect scripts | Move VM + `FUN_80021DF4` render tail | Palette pulses (HSV cycling), lightning, ambient SFX, particles |
| [Texture strips / morphs](#mechanism-3---strip-cycling-and-vertex-morphs) | Move records (op `0x40`) / bundle type-7 VDF | Move VM / morph stager `FUN_8001C604` | Texel strip frames; vertex deformation |

Confidence: **Confirmed** (disassembly) for the walker-table chain, the
ambient install chain, the mode-3 CLUT-cell cycler, and the VDF pack format;
**Inferred** where marked.

## Mechanism 1 - the scene walker table (bundle type-6 slot)

The CLUT-walk `MoveImage` table documented for the world-map ocean
([`world-map.md`](world-map.md) "Ocean / water animation",
[`clut_walk`](../../crates/asset/src/clut_walk.rs)) is **not kingdom-only**.
The asset-type dispatcher installs any bundle's type-byte `0x06` slot at
`DAT_8007B7C8` (`FUN_8001F05C` case 6) and field init spawns one walker actor
per entry; the kingdom bundles were just the first carriers found. Twelve
bundles ship a populated table:

| Scenes | Table shape |
|---|---|
| `map01` / `map02` / `map03` | 8 entries (ocean head + shimmer cells), byte-identical |
| `garmel`, `dohaty` | 1 entry: dest `(80, 506)`, 8 frames from park row 505 |
| `geremi`, `rayman`, `rayman2`, `tunnelb`, `tunnelc`, `son`, `edson` | 2 entries: dest `(0, 505)` from row 504 + dest `(160, 506)` from row 503 - the shared waterfall / water table |

Every other scene's type-6 slot is a 4-byte placeholder (`count = 0`).
The field carriers' park strips ride the same slot-0 TIM_LIST raw CLUT-block
records the kingdoms use (`clut_walk::park_strips`); resolution is **by type
byte**, not slot position - the `rayman`-family carrier is the MAN-less
count-4 table variant (`[1, 2, 6, 0x14]`).

Parsers: `legaia_asset::clut_walk::{from_scene_bundle, scene_park_strips}`
(disc-gated `crates/asset/tests/field_anim_tables_real.rs` pins the carrier
set). Engine consumers: the play-window water animator (field scenes now
resolve their own bundle's table) and the site field-scene viewer
(`web-viewer::field_scene::FieldSceneAnim`).

## Mechanism 2 - the ambient move-VM effect tree

### Install chain

At scene entry, each MAN **partition-1 effect-actor script** (a dedicated
record of the shape `install id N` + infinite loop - the Shift-JIS-named
"effect" actors of the prescript consumer census,
[`scene-bundles.md`](../formats/scene-bundles.md#scene_event_scripts---prescript-only))
fires field-VM op `0x34` sub-3. The chain:

```text
field VM op 0x34 sub-3 (arg)
  → FUN_800252EC(arg + 1)              ; record = _DAT_8007B8D0 + offsets[id]
  → FUN_80021B04(parent+0x14, ..., record, 0x1000)
                                       ; seat the part, PC = 2, and run its
                                       ; move-VM bytecode ONCE immediately
  → FUN_80023070 every game tick thereafter
```

Installer records fan out with move-VM op `0x25` (spawn child from the
prescript bundle); each child is also first-run inside the parent's spawn op,
which is what sequences the self-modifying fan-outs below. The counted-loop
pair op `0x18`/`0x19` (and `0x1A`/`0x1B`) drives repeated spawns: `0x18`
latches the PC and a counter, `0x19` decrements and jumps back to the saved
PC + 2 **while the decremented count has not underflowed** (retire is the
underflow past zero, advancing 1 word; a counter of N runs the body N + 1
times; the `0x4000` bit marks a never-decrementing infinite loop). The
retire/loop conditions and the `+2` land point are raw-`jr`-table facts of
`FUN_80023070` (`ghidra/scripts/funcs/80023070.txt`, `0x800235DC..` /
`0x80024150` epilogue) - the decompiled C renders the loop-back as a dead
`goto` chain.

### The CLUT-cell HSV cycler (the "pulsating flesh")

A `model_sel = 0x4000` render-mode part (`actor[+0x5A] = 3`) whose program
runs op `0x2C` `[x, y, w, h]` captures that VRAM rect into a per-actor
buffer (`FUN_8005842C` descriptor init + StoreImage; `w >= 0x11` heap, else
the inline `+0xAC` buffer) and arms the gate `+0x9C = 1`. The actor render
tail (`FUN_80021DF4` mode-3 arm, `0x800226D8..`) then every frame:

1. integrates the **H / S / V adds**: `+0x90/92/94 += (+0x96/98/9A *
   DAT_1F800393 * DAT_1F80037D) >> 6` - the tween source/scale registers,
   repurposed; ops `0x2B` (absolute), `0x2E` (velocities), `0x2D` (add)
   steer them;
2. ramps the white-blend amount `+0x68 += (+0x6A * dt) >> 6`, clamped at
   `0x100`;
3. once `+0x9C > 1` calls **`FUN_80019D50`**(mode `+0x9E`, white `+0x68`,
   h `+0x90`, s `+0x92`, v `+0x94`, buffer, descriptor): per captured
   15-bit texel - zero texels stay zero, STP preserved - RGB→HSV
   (`FUN_8001A78C`), `H += h` (mod `0x168`), `S += s`, `V += v` (clamped
   `0..0xFF`), HSV→RGB (`FUN_8001A6C8`, caps `0xF8`), and when
   `mode == 1` a white/invert blend `c += (255 - 2c) * white >> 8`; the
   repacked row is emitted as a fresh `LoadImage` packet onto the captured
   rect (`FUN_800583C8`);
4. advances `+0x9C` (clamped 1000).

So the "pulsating flesh" never moves a vertex: it is **palette-space HSV
cycling on the texture's CLUT rows**, re-uploaded every frame.

Provenance: `ghidra/scripts/funcs/80019d50.txt` (the full HSV kernel),
`80021df4.txt` (mode-3 arm), `8005842c.txt` / `800583c8.txt` (capture /
upload primitives). Engine port: `engine-core::clut_cell_fx`
(`apply_hsv_cell` + `mode3_integrate`) driven by
`engine-core::world::ambient` and applied to the software VRAM by
`World::step_ambient_fx` (renderer re-uploads on change - the same contract
as the scripted-CLUT sibling `World::step_clut_fx`).

### The self-modifying spawn stepper

jou's cycler record opens with ext op `0x2F 0x1E` - the in-place add
`bytecode[pc + op2 + 4] += op3` - targeting **its own following op-`0x2C`
`x` operand** in the shared prescript bundle. Each spawned instance
increments the shared word by 16 and then captures its own (stepped) cell,
so fifteen spawns of one record tile a whole CLUT row in 16-halfword cells.
Ext `0x1E`'s size is **4** (it skips its own operand words): the raw arm at
`overlay_0897_801d362c.txt` `0x801D3E18..` ends `li s2, 0x4` before the
shared `j 0x801D4A3C` size-return - the decompiled C renders that return as
a `func_0x801d4a3c()` label-call and drops the size, the same artifact class
as the label-call idiom in
[`ghidra.md`](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims).

The engine reproduces the fan-out with snapshot-at-spawn semantics
(`world/ambient.rs`); a part's self-write lands one instruction late
relative to retail's direct memory write, which shifts each instance's
captured cell one 16-halfword step (engine cells `0x00..0xE0`, retail
`0x10..0xF0`) - recorded here as a known divergence.

### jou worked example (prescript records, extraction 0630)

jou's MAN carries **one** ambient install (P1[1]: `34 30 00` → record 1).
Record 1 clears system flag `0x364`, then spawns:

| Child | Parts | Role |
|---|---|---|
| record 20 | 1 | Lightning **director**: mode-3 cell `(0, 502)`, player-bbox gates (ext `0x06`), sets flag `0x364`, screen flash (ext `0x3C` fade toward grey), thunder cue (op `0x1D` → `DAT_8007B6DE = 0x20B`) |
| record 21 | **15** (loop `0x18 0x0E` + `0x19`) | The flesh-palette cyclers: mode-3 cells tiling CLUT **row 502**, idle at zero adds, 4-step bright/desaturate decay on flag `0x364` |
| record 22 | 1 | Mode-3 cell `(0x70, 504)` - the lightning palette: idles at `V-add = -255` (dark), jumps bright on flag `0x364`, decays |
| record 23 | 1 | Render-mode-4 setup (op `0x1E` - the VRAM-rect scroller; **Inferred**: rotates a texel strip, see mode-4 note below) |
| record 45 | 1 | Ambient SFX loop: infinite `0x18 0x4000` loop of op `0x1D` cues `0x20E..0x211` with `0x09` waits |

Partition-2 cutscene timelines install args 1..9 (records 2..10) - the
story-beat effects (op `0x13` keyframe-mesh children, op `0x1F` morph
installs, op `0x14` colour ramps at absolute world positions). These are
cutscene-driven, not entry-ambient.

**Mode 4** (`+0x5A = 4`, seated by outer op `0x1E`) is the sibling VRAM
animator: per period `+0xC4` it captures a strip of the rect
`(+0xD0, +0xD2, +0xD4, +0xD6)`, `MoveImage`s the remainder over, and
re-inserts the strip at the far edge - a cyclic scroll of a texel/CLUT rect
(`80021df4.txt` `0x80022CC0..0x80022EE0`; horizontal shift `+0xCC * dt`,
vertical `+0xCE * dt`). Decoded from the disassembly; not yet ported -
see the open threads.

### The master ambient record 0

Town prescripts' record 0 is the fixed run of 8-byte rows
`[u8 a][u8 b][u8 c][u8 d][u16 3][u16 0]` (the "768-byte master ambient
stager"). Its row semantics are still **Unknown** - it is installed like any
record but its body does not walk as move-VM bytecode; the `0x0003` word
would execute as `WORLD_ROTATE_ADD` and the next row byte-pair as an
out-of-range opcode, ending the tick. Whether a dedicated consumer reads the
rows (spawn table?) or the record is effectively inert in retail remains
open.

## Mechanism 3 - strip cycling and vertex morphs

- **Texture strip cycling** (op `0x40` `MOVE_IMAGE`): a move program stamps
  authored VRAM frames over the displayed texel rect - the field 4-frame
  strip cycles live-traced in [`move-vm.md`](move-vm.md#0x40---move_image-size-7).
- **Vertex morphs**: every scene bundle reserves a type-7 **VDF pack**
  (`[u32 count][u32 offsets[count]]` + sub-entries of
  `[u32 record_count]` × `[u32 group][u32 dst_index][u32 count][count × 8-byte
  deltas]`); 61 bundles populate it (jou: 17 sub-entries of ground-vertex
  deltas; the `jouina`/`jouind`/`jouine` interiors carry the largest packs).
  Dispatcher case 7 installs it at `DAT_8007B7DC`, `FUN_8001FBCC` builds the
  sub-entry pointer table at `0x80083E58`, move-VM ops `0x0A`/`0x1F` arm
  per-actor morph lanes, and the morph stager `FUN_8001C604` applies the
  weighted deltas per frame (`engine-vm::vdf_morph`). Parser:
  `legaia_asset::scene_vdf`; disc-gated coverage in
  `crates/asset/tests/field_anim_tables_real.rs`. The morph *render*
  substitution (staged vertices replacing a drawn group's rest pose) is not
  yet wired - see the open threads.

## Engine + viewer wiring

- `engine-core::man_field_scripts::ambient_effect_installs` scans the MAN
  P1 effect scripts; scene entry auto-spawns each install
  (`World::spawn_ambient_record` - PORT of the `FUN_80021B04` prescript
  path, including the spawn-time first run and op-`0x25` recursion).
- `World::step_ambient_fx(vram)` drains the retail game-tick bank, ticks the
  parts, and applies the live `ClutCellFx` writes with a per-rect capture
  cache; the play-window calls it beside `step_clut_fx` and re-uploads on
  change.
- The site field-scene viewer runs both mechanisms in the browser:
  `field_scene_anim_init` / `field_scene_anim_tick` on the WASM viewer,
  with `site/js/field-scene-view.js` re-uploading the VRAM texture on
  change - jou's ground palette pulses and flashes in the assembled view.

## Related

- [`world-map.md`](world-map.md) - the kingdom walker table (ocean).
- [`move-vm.md`](move-vm.md) / [`move-vm-overlay-ext.md`](move-vm-overlay-ext.md) -
  the opcode set the ambient records run on.
- [`effect-vm.md`](effect-vm.md) - the battle-side effect pool (a different
  subsystem; the field ambience never touches it).
- [`../formats/scene-bundles.md`](../formats/scene-bundles.md) - the
  prescript bundle + consumer census.
- [`../formats/asset-type.md`](../formats/asset-type.md) - the type-6 /
  type-7 slot dispatch.
