# Field ambient animation - the moving parts of a "static" scene

Field maps are not static: water shimmers, waterfalls roll, and jou's fused
Juggernaut ground pulses under lightning. None of that is vertex data in the
environment pack - it is three runtime mechanisms layered over the scene's
VRAM and mesh pool, all disc-authored, all per-scene.

| Mechanism | Carrier | Stepper | What it animates |
|---|---|---|---|
| [Walker table](#mechanism-1---the-scene-walker-table-bundle-type-6-slot) | Scene bundle type-6 slot | `FUN_8001ADA4` case 0xB | CLUT-cell `MoveImage` cycling: water / waterfall shimmer |
| [Ambient move-VM tree](#mechanism-2---the-ambient-move-vm-effect-tree) | Prescript stager bundle + MAN P1 scripts | Move VM + `FUN_80021DF4` render tail | Palette pulses (HSV cycling), VRAM-rect scrolling, lightning, ambient SFX, particles |
| [Texture strips / morphs](#mechanism-3---strip-cycling-and-vertex-morphs) | Move records (op `0x40`) / bundle type-7 VDF | Move VM / morph stager `FUN_8001C604` + envelope `FUN_80020740` | Texel strip frames; vertex deformation (render substitution) |

Confidence: **Confirmed** (disassembly) for the walker-table chain, the
ambient install chain, both render-tail arms (the mode-3 CLUT-cell cycler
and the mode-4 rect scroller), the record-0 SFX bank, and the VDF pack
format; **Inferred** where marked.

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
| record 20 | 1 | Lightning **director**: mode-3 cell `(0, 502)`, strike cadence randomised by ext `0x05` `RAND_ADD` (writes `min + rand % range` into the next wait's operand), player-bbox gates (ext `0x06`), sets flag `0x364`, screen flash (ext `0x3C` fade toward grey), thunder cue (op `0x1D` → `DAT_8007B6DE = 0x20B`) |
| record 21 | **15** (loop `0x18 0x0E` + `0x19`) | The flesh-palette cyclers: mode-3 cells tiling CLUT **row 502**, idle at zero adds, 4-step bright/desaturate decay on flag `0x364` |
| record 22 | 1 | Mode-3 cell `(0x70, 504)` - the lightning palette: idles at `V-add = -255` (dark), jumps bright on flag `0x364`, decays |
| record 23 | 1 | Render-mode-4 setup (op `0x1E` - the VRAM-rect scroller; **Inferred**: rotates a texel strip, see mode-4 note below) |
| record 45 | 1 | Ambient SFX loop: infinite `0x18 0x4000` loop of op `0x1D` cues `0x20E..0x211` with `0x09` waits |

Partition-2 cutscene timelines install args 1..9 (records 2..10) - the
story-beat effects (op `0x13` effect-descriptor children, op `0x1F` morph
installs, op `0x14` colour ramps at absolute world positions). These are
cutscene-driven, not entry-ambient.

### The VRAM-rect scroller (render mode 4)

Mode 4 is the sibling of the CLUT-cell cycler, and it animates **texels in
place**: an authored VRAM rect is rotated under whatever meshes sample it,
with no vertex, UV or CLUT touched. It is what makes waterfalls fall.

Move-VM op `0x1E` seats it in one instruction - `+0x5A = 4` then seven
operands into `+0xC4` (period reload), `+0xCC` / `+0xCE` (per-period
horizontal / vertical step) and the rect `+0xD0..+0xD6` (`x, y, w, h`), in
that order (`FUN_80023070` `0x80023694..0x800236F0`). The countdown `+0xC6`
is *not* seated, so a freshly spawned part fires on its first tick.

The render tail then runs, per game tick (`80021df4.txt`
`0x80022CB8..0x80022EE0`):

1. `+0xC6 -= DAT_1F800393` - the adaptive frame step **alone**; unlike the
   mode-3 arm this one does not fold in the `DAT_1F80037D` speed scalar.
   The branch is `sll v0,0x10; bgez`, so the arm fires exactly on the tick
   the stored halfword's sign bit sets, i.e. on underflow.
2. On that tick `+0xC6` reloads from `+0xC4` and the two steps are read;
   otherwise both stay zero and both arms below are skipped.
3. Each non-zero step runs the same three-call strip rotate, horizontal
   first (`0x80022D08..`), vertical second (`0x80022DF8..`). With
   `sw = +0xCC * frame_step`: `FUN_8005842C` (`StoreImage`) captures
   `(x, y, sw, h)` into a scratch buffer bump-allocated off `0x1F8003A0` at
   `((sw*h*2) + 3) / 4 * 4` bytes; `FUN_80058490` (`MoveImage`) slides
   `(x + sw, y, w - sw, h)` onto `(x, y)`; `FUN_800583C8` (`LoadImage`)
   re-inserts the strip at `(x + w - sw, y, sw, h)`. Net: a **cyclic left
   rotation** by `sw` halfwords. The vertical arm is the transpose
   (`sh = +0xCE * frame_step`, top strip out, remainder up, strip back in at
   `y + h - sh`) - a cyclic up rotation.

Eight scenes put a live scroller on screen from their plain scene-entry
ambient tree, resolved by walking the records through the move VM (a
*linear* scan for the op word over-reports - the records jump):

| Scene | Rect `(x, y, w, h)` | Per-period step |
|---|---|---|
| `jou` | `(0x220, 0x80, 0x0E, 0x80)` | up 1 |
| `jouinb` | `(0x280, 0x100, 0x14, 0x80)`, `(0x280, 0x180, 0x14, 0x80)`, `(0x294, 0x100, 0x14, 0x50)` | up 3 / 6 / 7 |
| `jouine` | `(0x240, 0x100, 0x14, 0x80)`, `(0x240, 0x180, 0x14, 0x50)` | up 3 / 7 |
| `korout` | `(0x280, 0x00, 0x40, 0x100)` | up 1 |
| `koin3`, `other7` | `(0x280, 0x00, 0x08, 0x100)` | up 2 |
| `deroa` | `(0x268, 0xA0, 0x18, 0x60)` | up 1 |
| `noaru` | `(0x240, 0x00, 0x18, 0x60)`, `(0x258, 0x00, 0x08, 0x20)` | up 1 / 1 |

Every entry-reachable carrier scrolls **vertically only**, upward, over a
rect in the upper texture band (`x >= 0x200`) - falling water and energy
columns, never a CLUT row. Each carrier's record is the same three-line
shape jou's record 23 uses: the `0x1E` seat, then an infinite `0x1A` /
`0x1B` wait loop, so the VM parks and the render tail scrolls forever.

Engine port: `engine-core::world::ambient::vram_scroll` (`mode4_integrate`
countdown + `rotate_rect` texel kernel), queued per game tick by
`World::tick_ambient_fx` and applied to the software VRAM by
`World::step_ambient_fx`. Unlike the mode-3 write - recomputed each frame
from a cached capture - the rotate is **destructive**, so the queue is
drained in tick order inside the step. Coverage:
`crates/engine-core/tests/ambient_mode4_scroll_disc.rs`.

### The master ambient record 0 - the per-scene SFX descriptor bank

Town prescripts' record 0 is the fixed run of 8-byte rows
`[u8 p][u8 t][u8 l][u8 n][u16 3][u16 0]` (the "768-byte master ambient
stager"). It is not move-VM bytecode and never was: it is the **per-scene
extension of the sound-effect descriptor table**
([`sfx-table.md`](../formats/sfx-table.md)), covering cue ids `>= 0x200`.

The addressing is what pins it. In field mode the bundle base
`_DAT_8007B8D0` is the scene buffer + `0x12800` - the prescript bundle
(`field_asset_loader` `0x8001F840..0x8001F864`, `lw v0,0xd8(s3)` with
`s3 = 0x1F800314`). Both SFX consumers then reach the bank the same way,
through the bundle's own offset table:

```text
id <  0x200:  desc = DAT_8006F198 + id*8               ; the static table
id >= 0x200:  desc = _DAT_8007B8D0 + offsets[0]        ; = record 0
                     + (id - 0x200)*8
```

`FUN_800250D4` (`0x800250FC..0x8002514C`) keys `+3 & 0x1F` voices on;
`FUN_80016B6C` (`0x80016C24..0x80016CB0`) drains the cue ring and, on the
same descriptor, prints the designer's `"setbl p:%d t:%d l:%d n:%d id:%d"`
line from bytes `+0..+4`. `offsets[0]` is the identical word
`FUN_800252EC` reads for stager id 0 - so "record 0" and "the runtime SFX
bank" are two names for one address.

The row shape falls straight out of that, and the disc bears it out: `+0`
program, `+1` tone (consecutive within a program), `+2` level (clustered in
the low 60s, exactly where the static table's `l` sits), `+3` voice count
`1..=2`, `+4` = **category 3** - the variable VAB slot a per-scene bank has
to key - and `+5..+7` zero in every row, the same trailer the static table
carries. The record is sized per scene, not fixed: jou reserves 96 rows and
populates 40 (`0x200..=0x227`); `rugi` carries 21, all populated. jou's own
tree is the worked example - its lightning director cues `0x20B` and its
ambient SFX loop cues `0x20E..0x211`, i.e. rows 11 and 14..17 of its own
record 0, all inside the populated span.

Confidence: **Confirmed** (disassembly). What each row's `p` / `t` selects
inside the scene's VAB is the open half - that is the same question the
static table leaves open, not a record-0 question.

Two knock-ons worth carrying: `0x8007B8D0` is a shared *current-bundle*
slot, not one subsystem's pointer (the boot sound-bank loader
`FUN_8001FA88` puts its own `0x1800`-byte buffer there and immediately
saves that bank's record-0 address at `gp+0x678`, because the next scene
load overwrites the slot); and record 0 being data is why it must **not**
be spawned as a stager - walking it as move-VM bytecode reads the `0x0003`
category word as `WORLD_ROTATE_ADD` and dies on the next row.

## Mechanism 3 - strip cycling and vertex morphs

- **Texture strip cycling** (op `0x40` `MOVE_IMAGE`): a move program stamps
  authored VRAM frames over the displayed texel rect - the field 4-frame
  strip cycles live-traced in [`move-vm.md`](move-vm.md#0x40---move_image-size-7).
- **Vertex morphs** - the type-7 **VDF pack** chain, next section.

### The VDF vertex-morph chain

Every scene bundle reserves a type-7 **VDF pack**
(`[u32 count][u32 offsets[count]]` + sub-entries of
`[u32 record_count]` × `[u32 group][u32 dst_index][u32 count][count × 8-byte
deltas]`); 61 bundles populate it (jou: 17 sub-entries of ground-vertex
deltas; the `jouina`/`jouind`/`jouine` interiors carry the largest packs;
`rikuroa` carries its pack as a streaming `DATA_FIELD` VDF chunk instead of
the bundle slot). Dispatcher case 7 installs the decoded pack at
`DAT_8007B7DC` and `FUN_8001FBCC` builds the sub-entry pointer table at
`0x80083E58`. Parser: `legaia_asset::scene_vdf`; disc-gated coverage in
`crates/asset/tests/field_anim_tables_real.rs`.

**Arming** is the ambient move-VM tree itself: a stager part **with a
mesh** - `model_sel` binds scene-pack TMD `model_sel - 5` (the retail
global-TMD table `DAT_8007C018` keeps the five character meshes ahead of
the pack, `DAT_8007B6F8 = 5`) - runs op `0x0A`
`[reset][count][(vdf_idx, up, down) × count]`, which writes the lane
sub-entry indices (`+0xB0 + i`, bytes), the per-lane ramp velocities
(`+0xB8`/`+0xC8`), and sets the actor flag bit `0x1000`. The ramp envelope
`FUN_80020740` then moves each lane's weight (`+0xA0 + i*2`) per frame,
steered by the `+0x62` envelope flags the record sets with op `0x32`
(rikuroa `0x0400` = hold at peak; town0e `0x1000` = recycle the pulse).
Op `0x1F` is the direct-install sibling (writes the index bytes + four
weights outright). Corpus census of op-`0x0A` carriers: `rikuroa`/
`rikuroa2` records 69/70 (spawned from the entry install behind system
flags `0x281`/`0x282` - the generator sacs swell only in that story
state), `town0e` records 10/11 (spawned ×3+1 from its record-1 tree - see
[below](#town0es-installer-is-a-placed-actor-not-an-effect-actor)),
`jagaroom` records 20/21 (not referenced by any op-`0x25` in the table -
non-entry installs). **jou arms nothing at plain entry**: no `0x0A`
anywhere in its 47 stager records; its flesh-growth morphs ride the P2
cutscene chains (op `0x1F` in record 13).

#### town0e's installer is a placed actor, not an effect actor

The install op is the same one every other ambience uses - `0x34` sub-3
with arg 0, i.e. stager record 1 - but it does not sit on a dedicated
effect-actor script. It is the **second instruction of partition-1
placement 29**, a full placed actor with its own dialogue: `25` (nop),
`34 30 00` (install), then a `SysFlag.Test 0x1A` that either parks the
actor at the off-map `(0x7F, 0x7F)` tile and self-loops, or seats it on a
real tile and runs its script. The install is ahead of that branch, so it
fires whichever way the flag reads.

That is why a shape-filtered census of *pure* effect scripts
(`engine-core::man_field_scripts::ambient_effect_installs`, which requires
the record to contain nothing but nops, flag writes, the install and the
self-loop) reports town0e as having no ambient install: the record is not
pure, not absent. `scene_stager_installs` - the unfiltered op-`0x34` sub-3
census over all three partitions - finds it.

What the disc bytes do **not** settle is the moment it fires. Retail
pre-runs each partition-1 placement's channel at scene entry, and the
install is ahead of every blocking op in this record, so it should land in
that first slice - but that step is an inference from the settled pre-run
mechanism, not something a live capture has shown for this scene.

**Render substitution** (`FUN_8001ADA4` `0x8001B424..`, per drawn group):
when the part's flags carry bit `0x1000`, `FUN_8001C604(actor, group)`
copies the group's rest-pose GTE vertices into scratch at the top of the
`_DAT_8007B85C` buffer, applies every armed lane's matching records
scaled by the lane weight (`FUN_8005B038`: `dst += delta * weight >>
12`, GPF saturation), retargets the group-table vertex pointer at the
scratch for that draw, and the caller restores the authored pointer
afterwards - the rest pose is never mutated.

Engine: kernels in `engine-vm::vdf_morph` (record walk, GPF blend,
ActorState envelope bridge), envelope on armed ambient parts in
`World::tick_ambient_part`, morph surface
`World::{ambient_morph_parts, current_morph_deltas, take_morph_dirty_slots}`.
Consumers rebuild just the dirty pack meshes with the deltas staged onto a
cloned TMD (`ResolvedTmd::with_group_deltas`) - the substitution as data:
native play-window (`field_morph_live` draw substitution), site
field-scene viewer (`field_scene_morph_slots`/`_positions`), web play
runtime (`field_morph_slots`/`_positions`).

**Scene-entry VDF pulse** (enhancement, `engine-core::vdf_pulse`): for a
scene whose pack is populated but whose stager table never arms morph
lanes in any story state (jou), the host installs a rolling envelope over
the pack at entry - one lane per sub-entry, cascading up and back down
forever, each sub-entry targeting the pack meshes its records fit exactly
(`dst + count == n_vert`). The delta arithmetic is the retail kernel
chain; the arming (lanes, velocities, entry trigger) is the engine's own -
jou's fused-Juggernaut ground throbs at plain entry instead of only
during its cutscene set pieces. Scenes with retail arming are untouched
(the installer self-guards on any op-`0x0A` stager record).

## Engine + viewer wiring

- `engine-core::man_field_scripts::ambient_effect_installs` scans the MAN
  P1 effect scripts; scene entry auto-spawns each install
  (`World::spawn_ambient_record` - PORT of the `FUN_80021B04` prescript
  path, including the spawn-time first run and op-`0x25` recursion). It is
  the shape-filtered census, so a scene whose install rides a placed
  actor's script (town0e) does not auto-spawn - the unfiltered
  `scene_stager_installs` is what finds those.
- `World::step_ambient_fx(vram)` drains the retail game-tick bank, ticks
  the parts, and applies both render-tail arms: the `ClutCellFx` writes
  through a per-rect capture cache, and the mode-4 strip rotations in tick
  order. The play-window calls it beside `step_clut_fx` and re-uploads on
  change.
- The spawn-time first run also runs the render-tail arms, so a part
  seated at scene entry emits its first cell write / strip rotate on the
  entry tick rather than the one after.
- All three render surfaces drain the world's VDF-morph dirty set beside
  the palette fx (see the mechanism-3 section): the play-window
  substitutes rebuilt meshes into its draw lists, the two web surfaces
  re-upload just the dirty meshes' position streams.
- The site field-scene viewer runs both mechanisms in the browser:
  `field_scene_anim_init` / `field_scene_anim_tick` on the WASM viewer,
  with `site/js/field-scene-view.js` re-uploading the VRAM texture on
  change - jou's ground palette pulses and flashes in the assembled view.
- The site **play** page runs them through the live engine instead: the
  scene host spawns the ambient tree at scene entry, and
  `LegaiaRuntime::tick_frame` drains it (plus the scripted CLUT fx and a
  walker-only `FieldSceneAnim` whose park strips land in the host VRAM at
  scene rebuild) each sim tick - the browser twin of the play-window's
  `apply_world_clut_fx`. `site/js/play-app.js` polls
  `field_vram_take_dirty` per frame and re-uploads `field_vram_bytes` only
  on real texel changes; the drain is battle-guarded like the native path.

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
