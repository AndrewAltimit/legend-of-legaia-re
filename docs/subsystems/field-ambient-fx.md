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
ambient install chain, the actor pool and its halted-part free, both
render-tail arms (the mode-3 CLUT-cell cycler and the mode-4 rect scroller),
the record-0 SFX bank, and the VDF pack format; **Inferred** where marked.

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

At scene entry the placement installer runs each MAN **partition-1 placement**
through the field VM for one frame slice, and every op `0x34` sub-3 that slice
executes installs one stager record. The chain:

```text
field VM op 0x34 sub-3 (arg)
  → FUN_800252EC(arg + 1)              ; record = _DAT_8007B8D0 + offsets[id]
  → FUN_80021B04(parent+0x14, ..., record, 0x1000)
                                       ; seat the part, PC = 2, and run its
                                       ; move-VM bytecode ONCE immediately
  → FUN_80023070 every game tick thereafter
```

**Scene entry is not a second mechanism.** `FUN_8003A1E4` carries no install
code of its own: it calls `FUN_801DE840` - the field VM - for one frame slice
per just-spawned placement, and the install happens in that dispatcher's op
`0x34` sub-3 arm at `0x801E00B0` like any other. So a load-slice install and
a runtime install are the same instruction reached at two moments, and the
same is true of the port: both go to `engine-core::World::spawn_ambient_record`
(`vm_hosts::effect_anim_trigger` for the runtime arm). A second, thinner pool
exists in the engine (`World::spawn_field_stager`, backed by the
`SummonScene` stand-in) and carries neither render tail; it is the
play-window's debug exerciser and is on no retail path.

The arm also fixes the **seat**: `FUN_800252EC(bytecode[1] + 1, s5 + 0x14,
s5 + 0x24)` where `s5` is the executing script's context, retargeted by the
`0x80` cross-context prefix. `FUN_80021B04` copies `ctx[+0x14..+0x1A]` into
the part's world position (`0x80021B94..`) and `ctx[+0x24..+0x2A]` into its
render banks (`0x80021D8C..`), so a scripted install stages where its actor
stands - not at the player.

The carrier is **not** a distinguished kind of script. Most scenes do put the
install on a dedicated effect-actor record (`install id N` + infinite loop - the
Shift-JIS-named "effect" actors of the prescript consumer census,
[`scene-bundles.md`](../formats/scene-bundles.md#scene_event_scripts---prescript-only)),
but a fully scripted, dialogue-bearing placement installs exactly the same way,
and on the retail disc that is the majority case. What decides it is
[which ops the load slice reaches](#which-installs-fire-at-scene-entry).

#### Which installs fire at scene entry

`FUN_8003A1E4`, the pre-run the placement spawn loop calls per just-spawned
placement, carries its own copy of the per-actor script runner's frame slice.
Two properties of that loop settle the question, and both are raw-branch facts:

- **The pre-run is gated on the record's first opcode.** `0x8003A480` reads
  `bytecode[pc0]` and runs `addiu v0,v1,-0x24; sltiu v0,v0,0x2; beq v0,zero`
  past the whole VM loop - so unless the first byte is `0x24` or `0x25` the
  record's script never runs at load at all. Every placement authored to install
  something at entry opens with `25`.
- **The slice runs while `(opcode & 0x7F) >= 0x20` and breaks *after* executing
  an opcode whose full byte is `0x21`** (`0x8003A4C4` `beq s1,s4` against
  `li s4,0x21` - the raw byte, so the cross-context `0xA1` does not break), or
  when the dispatcher returns an unchanged PC. `FUN_80039B7C`'s per-frame slice
  (`0x80039E20`) is the identical pair of tests.

`0x21` and `0x25` are both "nop" to a disassembler and only one of them ends the
slice. That is the whole mechanism: a record written `25 / 34 30 00 / …` fires
its install in the load slice whatever follows, and the `21` further down is
where the placement parks. A second install *after* that `21` (`edkorout`
P1[15]) belongs to a later slice, which a free-roam placement never gets - the
per-actor ticker only dispatches script stepping for an actor carrying the
script-engaged bit.

Ports read this as the record's **unconditional entry prefix**: walk from the
record's first opcode and stop at the first branch, jump, blocking op or `0x21`.
That is an under-approximation on purpose - a flag-gated install deeper in a
record (`nilboa` P1[3]'s second install, both of `suimon` P1[4]'s) is left out
rather than guessed at, because the branch outcome is runtime state.

`see ghidra/scripts/funcs/8003a1e4.txt`, `80039b7c.txt`. Port:
`engine-core::man_field_scripts::scene_entry_ambient_installs`, with the
per-scene census pinned by the disc-gated
`crates/engine-core/tests/ambient_entry_install_census_disc.rs`. Of the scenes
that install an ambient tree at entry, 22 carry it on a pure effect-actor
record and 41 on an ordinary placement.

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

### The part pool and the halted-part free

A stager part is an ordinary actor out of the scene's shared actor pool, and
the pool is small and fixed. `FUN_800203EC` seeds the free stack with `0x8E`
down to `0` - **143 slots**, `0xD8` bytes apart - `FUN_80020454` pops one per
spawn (returning null when the stack is empty, which `FUN_80020DE0` treats as
an error and reports through `FUN_800567A8`), and `FUN_800204A4` pushes it
back. Every actor in the scene draws on that pool, not the ambient tree alone.

What returns a slot is the per-frame actor-list walk `FUN_8002519C`. Per live
actor it tests `actor[+0x10] & 0x8` - the bit move-VM op `0x08` HALT sets
(`0x800251E8`) - **before** dispatching the actor's tick word, and only the
not-halted branch reaches the `jalr`. So a part renders one last time inside
the call that halts it and never again: its CLUT-cell write stops there, its
strip rotation stops there.

Freeing is one walk later than stopping. The halted arm tests bit
`0x02000000` and, while it is **clear**, branches past the entire teardown to
set it (`0x800251F4` `and`/`beq` against `lui s3, 0x200`) - so the first walk
that sees a halted actor only marks it. The next one runs `FUN_80024DFC`,
releases the heap buffers (including the mode-3 capture at `+0xA8`, keyed on
the tick word being the stager render tail `FUN_80021DF4` with `+0x5A == 3`,
and firing the `+0x5A == 5` cue through `FUN_800250D4`), and pushes the slot
back with `FUN_800204A4`.

That free is what makes the counted-loop fan-outs above finite. Several scenes
are **emitters**: an infinite `0x18 0x4000` loop around a wait and an op-`0x25`
spawn, whose children halt within a few ticks. The live population is then
spawn-rate times lifetime - single digits to a few dozen - while the number of
parts *created* over a minute of standing still runs into the thousands. Read
the pool without the free path and those scenes look like enormous authored
trees; read it at a cap and they report the cap.

Engine: `World::retire_finished_ambient_parts` (run at the top of
`World::tick_ambient_fx`, so the port drops a part one tick earlier than the
two-walk retail teardown - nothing observable rides on the difference, since
the part has already stopped ticking and rendering), cap
`world::ambient::MAX_AMBIENT_PARTS` = the retail 143, exhaustion queryable
through `World::ambient_pool_exhausted` and logged rather than dropped
silently. Coverage:
`crates/engine-core/tests/ambient_runtime_install.rs` (disc-free, with the
never-halting contrast) and `ambient_part_pool_disc.rs` (the corpus census -
every scene's population flat, none near the ceiling).

Two engine-side notes belong with it. A part still holding undrained mode-4
rotations is kept until they land, because the engine queues those for the
next VRAM-bearing step where retail applies each inside the tick that fired
it. And an op-`0x25` operand of **0** is refused: table entry 0 is the
[SFX descriptor bank](#the-master-ambient-record-0---the-per-scene-sfx-descriptor-bank),
not bytecode, and the install op cannot name it (`FUN_800252EC` is called with
`arg + 1`) - so a spawn of record 0 is manufactured rather than authored. In
this port it is manufactured by the move VM's `0x2F` extension arm returning
the default-arm size 1 for sub-ops whose retail arms are wider (`0x25` is
size 3), so `2F 25 <slot>` re-enters the outer dispatcher on its own
sub-opcode word and decodes as a child spawn of record 0.

### The pool's control block `0x8007C348`

The words in front of the pool are a heterogeneous control block, not a table
indexed by channel. Its layout comes from the pool helpers' own stores and
from the stage init that fills it:

| Offset | Content | Written by |
|---|---|---|
| `+0x00` | free-stack **top index** (`0x8E` when full; the pop's `bltz` refuses at `-1`) | `FUN_800203EC` seed, `FUN_80020424` / `FUN_80020454` pop, `FUN_800204A4` push |
| `+0x04`, `+0x08`, `+0x0C`, `+0x10`, `+0x18`, `+0x14`, `+0x24` (pop order) | seven actor-list **sentinel nodes**, each a pool slot popped by `FUN_80020424` and left pointing at itself | per-stage init `FUN_8001E1B4` (`0x8001E324..0x8001E364`) |
| `+0x1C` | the **player** actor | field MAIN_INIT `FUN_801D6704` at `0x801D6D7C`: the return of the spawn allocator `FUN_80020DE0(0x800705FC, *(block + 4))` |
| `+0x28 + 4*i` | the free stack itself, 143 slot pointers | `FUN_800203EC`, then the pop / push pair |

A PCSX-Redux write-watch over `+0x04..+0x27` across a door crossing
(`korb2` -> `kor`, the `korb2_field_card_boot` save,
[`autorun_w7c_kor5_tail.lua`](../../scripts/pcsx-redux/autorun_w7c_kor5_tail.lua)
with `LEGAIA_POOL_WATCH=1`) sees exactly those stores: the seven sentinel
stores at `0x8001E338..0x8001E364` in mode `0x02`, then `+0x1C` at
`0x801D6D7C` about eighty vsyncs later. Read across ten library states
(field, battle, world map and minigame), the six sentinels at `+0x04..+0x18`
are the same six pool slots every time (`0x80083D7C` down to `0x80083944`,
`0xD8` apart), `+0x1C` is `0x80083794` in every field state and `0x800830D4`
in the battle, world-map and minigame ones, and every live free-stack entry
lands on a slot boundary.

So the channel resolver `FUN_8003C83C` (and the inline copy of it in
`FUN_8003A9D4`) does not index this block by channel either: `0xF8` reads
`+0x1C`, `0xFB` walks the `+0x04` list for the node whose tick word is the
world-map entity SM `0x801DA51C`, and any other id walks the `+0x0C` list
comparing actor `+0x50`. Read as `0x8007C348 + 4*i`, index 7 lands on the
player only because the player word happens to sit at `+0x1C`.

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

Seventeen scenes put a live scroller on screen from their plain scene-entry
ambient tree, resolved by walking the records through the move VM (a
*linear* scan for the op word over-reports - the records jump):

| Scene | Rect `(x, y, w, h)` | Per-period step |
|---|---|---|
| `jou` | `(0x220, 0x80, 0x0E, 0x80)` | up 1 |
| `jouinb` | `(0x280, 0x100, 0x14, 0x80)`, `(0x280, 0x180, 0x14, 0x80)`, `(0x294, 0x100, 0x14, 0x50)` | up 3 / 6 / 7 |
| `jouind`, `jouine` | `(0x240, 0x100, 0x14, 0x80)`, `(0x240, 0x180, 0x14, 0x50)` | up 3 / 7 |
| `vell` | `(0x2AA, 0x28, 0x14, 0x80)`, `(0x2AA, 0xA8, 0x14, 0x50)` | up 3 / 7 |
| `korout` | `(0x280, 0x00, 0x40, 0x100)` | up 1 |
| `koin3`, `other7` | `(0x280, 0x00, 0x08, 0x100)` | up 2 |
| `keikoku` | `(0x240, 0x00, 0x20, 0x100)` | up 1 |
| `deroa` | `(0x268, 0xA0, 0x18, 0x60)` | up 1 |
| `noaru` | `(0x240, 0x00, 0x18, 0x60)`, `(0x258, 0x00, 0x08, 0x20)` | up 1 / 1 |
| `dolk` | `(0x2E0, 0xE0, 0x10, 0x20)`, `(0x2F0, 0xE0, 0x10, 0x20)` | up 1 / 2 |
| `dolk2` | the same two rects | up 1 / 1 |
| `jiji` | `(0x310, 0x00, 0x10, 0x20)` | up 1 |
| `dohaty` | `(0x2C0, 0xC0, 0x10, 0x40)` | up 1 |
| `station` | `(0x300, 0x100, 0x18, 0x60)` | up 1 |
| `tunnelc` | `(0x300, 0x100, 0x20, 0x80)`; `(0x00, 0x1FC, 0x100, 0x01)` | up 1; **right 0x10** |

Sixteen of the seventeen scroll **vertically only**, upward, over a rect in
the upper texture band (`x >= 0x200`) - falling water and energy columns,
animated as texels. `tunnelc`'s second seat is the one exception and it is
worth reading as a limit on the generalisation rather than as an oddity: a
full-width one-row rect at `(0, 508)` stepping right by 16 halfwords is a
**CLUT row**, so the same rotate primitive walks a palette when the authored
rect is a palette. Nothing in the mode's implementation distinguishes the two
cases - `StoreImage` / `MoveImage` / `LoadImage` do not care what the texels
mean - and the corpus simply contains one author who used that.

None of them retires: jou's record 23 is the shape to read, three lines long -
the `0x1E` seat, then an infinite `0x1A` / `0x1B` wait loop - so the VM parks
and the render tail scrolls forever.

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

Two knock-ons worth carrying. First, `0x8007B8D0` is a shared
*current-bundle* slot, not one subsystem's pointer: the **battle**
sound-bank loader `FUN_8001FA88` puts its own `0x1800`-byte buffer there
and saves that bank's record-0 address at `gp+0x678`, because the next
scene load overwrites the slot. (This page previously called that a *boot*
loader. Its one caller anywhere on the disc is `0x80051A3C` inside battle
init `FUN_800513F0`, so `bse.dat` occupies the slot per battle, not per
session; the saved `gp+0x678` pointer is what the battle cue router
`FUN_8004FE5C` writes each cue's category through -
[`bse-dat.md`](../formats/bse-dat.md).) Second, record 0 being data is why
it must **not** be spawned as a stager - walking it as move-VM bytecode
reads the `0x0003` category word as `WORLD_ROTATE_ADD` and dies on the next
row.

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
real tile and runs its script.

Both halves of the [entry-slice rule](#which-installs-fire-at-scene-entry)
apply exactly here, which is what makes town0e the worked example for it: the
record opens `25`, so the pre-run runs at all; the install sits ahead of the
`SysFlag.Test`, so it is in the unconditional prefix and fires whichever way
the flag reads. A census that instead discriminates on script *shape* - the
record containing nothing but nops, flag writes, the install and a self-loop -
reports town0e as having no ambient install, because the record is not pure,
not absent.

What the disc bytes do **not** settle is the precise moment inside scene load.
The pre-run mechanism is disassembly-settled and this record is inside its
first slice, but no live capture has shown that slice running for this scene.

The tree it installs is small and entirely about the VDF morphs: record 1 fans
out into two render-mode nodes, three copies of the mesh record binding
env-pack slot 112 and one binding slot 113, and it is the slot-113 part whose
op-`0x0A` arms lanes 10 / 11 under the `0x1000` recycle envelope.

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

- `engine-core::man_field_scripts::scene_entry_ambient_installs` is the
  census both hosts run at scene entry - the
  [entry-slice rule](#which-installs-fire-at-scene-entry) over every P1
  placement, so a scene whose install rides a placed actor's script (town0e)
  auto-spawns like any other. Each install goes to
  `World::spawn_ambient_record` (PORT of the `FUN_80021B04` prescript path,
  including the spawn-time first run and op-`0x25` recursion), and so does the
  **runtime** install the live field VM reaches - `vm_hosts::effect_anim_trigger`,
  seated at the executing context via `World::spawn_ambient_record_at`. One
  retail chain, one port. The narrower
  `ambient_effect_installs` (pure effect scripts only) and the unfiltered
  three-partition `scene_stager_installs` remain as the two sub-censuses the
  format work uses; neither is the entry rule.
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

## Photosensitivity guard

Retail koin3 (the casino dance venue) is the corpus's worst flasher: its
entry-ambient tree spawns ~24 live parts, 21 of them mode-3 CLUT-cell
cyclers over the venue palettes (CLUT rows 504-508), and the strobe row's
records re-key `v_add` **-256 <-> 0 on consecutive game ticks** - a
full-swing bright/black palette strobe at 15 Hz cycles on retail hardware
(the photosensitivity guideline ceiling is 3 flashes per second). This is
authored behaviour, not a port bug, so the mitigation is an engine
enhancement, defaulted to the safe side.

`World::toggles.reduce_flashing` (default **on**, mirrored from
`OptionsState::reduce_flashing` by the windowed hosts) gates a limiter
inside `World::step_ambient_fx`, the one site where every mode-3
`ClutCellFx` becomes texels on all three render surfaces. The move-VM
parts always advance retail-exact; only the **applied** luminance channels
are shaped: `v_add` and `white` slew toward the simulated target at 16
units per elapsed game tick (first application snaps, so scene-entry state
is exact), while `h_add` / `s_add` pass through untouched - the venue's
colored cone sweeps and hue wheels keep their full motion, and the
bright/black strobe collapses to a low-amplitude shimmer (~0.9 Hz full
cycles at the town clock). A drained backlog steps proportionally, so
hosts that bank ticks catch up rather than slow down. The per-rect applied
state (`World::ambient.flash_applied`) clears with the capture cache on
scene entry; turning the option off clears it and restores the
retail-exact steps. Unit tests: `world/tests/flash_limiter.rs`.

The scripted `4C 61` CLUT family (`World::step_clut_fx`) needs no guard:
its fades are already ramps (`ClutFade`) and its one-shots are singular
event stamps, not oscillators.

## Mechanism 4 - the ambient particle emitter

A fourth carrier, and the one that is neither a bundle slot nor a move-VM
record: `FUN_801D6058`, the `+0x08` handler of the `0x18`-byte plain template
at `0x801F271C` in the field overlay's own template table. Field MAIN INIT
`FUN_801D6704` spawns exactly one per field entry - `jal 0x80024c88` at
`0x801D6FD8`, then `+0x1A = 1` to select its **scene** arm - behind a `bnez`
on the field-entry mode word `_DAT_8007B8B8`, so a warp entry (a return from
battle, a minigame or an FMV) skips it.

The scene arm takes twenty-four independent draws per frame, each with a one
in sixteen chance of bursting `(rand & 3) + 1` particles at one point sampled
inside the camera's visible-tile span `DAT_1F8003E8..EB`, centred on the
negated player X/Z that `_DAT_80089118` / `_DAT_80089120` hold. What decides
whether any of that happens is the master gate `_DAT_8007B854`, read by the
handler's first instruction (`0x801D605C`). The gate is **script-driven**,
not per-scene data: field-VM op `0x4C`, outer nibble `3`, sub-`0` sets it
(`0x801E0F38`) and sub-`1` clears it (`0x801E0F44`), off the 16-entry jump
table at `0x801CEEB8`. Its only other reader is the field render pass at
`0x80026EBC`, which stages the four 16-byte UV rows at `0x8007322C` into
scratchpad `0x1F8002D0` when the game mode is `3` and the gate is set - and
then draws the pool (below).

The emitter names tiles; it draws nothing. Each burst point goes to
`FUN_801D629C(tile_x, tile_z)` (the `a2` / `a3` velocity it also loads are
never read), and everything from there on is the **fog pool** - retail's
own name for the system is the dev trace `fog_set %d %d` the emitter can
print, whose two numbers are the pool's live count and its cap.

### The fog pool: spawner, records, render pass

Three routines, one pool at `_DAT_8007B7E0` (see
[`fog_particles`](../../crates/engine-core/src/fog_particles.rs) for the
byte layout):

| Routine | Image | Role |
|---|---|---|
| `FUN_801D629C` | field overlay (0897, file `0x7A84`) | **Spawner.** Rejects a tile outside the walk-region box `0x1F800384..87`; finds the first MAN section-4 region whose open box holds the tile and stops if it is disabled; requires `_DAT_8007BCA8 < _DAT_8007BCB0` (live count under the cap, `0x18` from the field reset); on the overworld drops a tile whose camera-space depth passes `0x4000` (see below); pops a slot off the pool's free stack (`FUN_8001FA34`); fills the record - drift from the region's angle base + random spread through the sin/cos LUTs times its speed, height `-(rand & 0x7F)`, grey `rand & 0x7F`, age rate `(rand & 7) + 8`. |
| `FUN_8003F348` | SCUS | **Walk.** Called only from the render pass's gated site (`0x80026F24`); pushes the matrix stack, folds `RotMatrixX(0x400)` into the camera rotation, then runs the update on every one of the 80 records whose alive byte is set. |
| `FUN_8003F3FC` | SCUS | **Per-particle update + draw.** Kills a record outside the walk box; brightness ramps `0..0xFF` over age `0..0x400`, holds to `0xC00`, then fades and kills; colour is `grey * tint * brightness >> 15` per channel with the tint the op `0x4C 0x12` global multiply (`_DAT_8007BCB8..BA`, `0x80` neutral); drift and age advance by `DAT_1F800393`; the player's `+-0x180 / +-0x80 / +-0x80` box ages it again, three times more with a d-pad bit held; then two halves through `FUN_8003F86C`, and a record whose two halves both cull is freed (`FUN_8001FA68`). |
| `FUN_8003F86C` | SCUS | **Half-sheet emitter.** One `POLY_FT4` (tag `0x09` words, command `0x2E`: textured, semi-transparent, texture-blended), a **view-space billboard** at the particle's depth - see [the sheet is a billboard](#the-sheet-is-a-view-space-billboard); culled when both corners are off the `[-8, 0x148)` columns, both above row `0`, or both below row `0x190`; kept but not drawn between rows `0xF0` and `0x190`; NCLIP-culled at a signed area past `0x1F40` quarter-pixels; linked at OT bucket `SZ >> 5`. On the overworld it also drops a half nearer than `SZ 0x310`, links at `(SZ - 0x10) >> 5` and adds the curvature table's `SY` term. |

The half-width is `(0x180 + (age >> 4)) >> 1`. Retail adds a byte from
`FUN_8003F838` here, but it seeds that PRNG's state with the record's age
rate first, and the step `v = state * 12 + 2; state = (v << 16) + (v >> 16)`
leaves a zero low byte for every rate the spawner can write - the random
term is dead code. The art is two cloud wisps in the effect-texture pool:
texture page `0x0027` (VRAM `(448, 0)`, 4bpp, ABR `1` additive) through
CLUT `0x7640` (`(0, 473)`); the left half samples staged row 1
(`v 0x58..0x6F`), the right half row 0 (`v 0x40..0x57`), each `u 0..0x3F`.
Rows 2 and 3 are staged and unread.

### Where the fog texels come from

The page halfword is the upper half of the second staged UV word
(`0x0027403F` at `0x8007323C`), a GP0 texpage: bits `0..3` give X `7 * 64 =
448`, bit 4 - the Y-base bit - is clear, bits `5..6` are ABR `1`. So the
sheets sample `(448, 0)`, not `(448, 256)`, and the cells they read are
rows `0x40..0x6F` of the `(448, 0)` 64x256 page of the PROT 0874 section-2
effect-texture pool (`legaia_engine_core::scene::upload_effect_textures_into_vram`),
whose CLUT strip also lands on row 473. Retail keeps that pool resident in
field and world-map VRAM: the fog cells and the row-473 CLUT are
byte-identical in nine PCSX-Redux states across `retona`, `town01`,
`chitei2`, `dolk`, `map01`, `son`, `vozz`, `map03` and `vell`
(`scripts/pcsx-redux/extract_vram_from_sstate.py`).

The pool is resident **before** the scene loads. Where a scene TIM covers a
pool rect the scene's texels win: `dolk`'s TIMs reach into the `(448, 0)`
page, and its capture holds the scene's texels on all 1010 halfwords where
the two differ. So a port layers the pool *under* its scene build
(`Vram::underlay`, the order `SceneResources` already uses for the
boot-resident system-UI bundle); writing it over the build clobbers those
texels. Both builds underlay it: the native window's
(`engine-shell` `window/run.rs`) and the engine host's own field entry
(`engine-core::scene::host::scene_entry`), which is the VRAM the browser
play page draws from; the disc-gated
`crates/engine-core/tests/scene_host_effect_pool_underlay_disc.rs` pins the
host side on `dolk` and `vell`. A disc-wide census of
every CDNAME scene built the window's way finds the overlap in `dolk` and
`dolk2` (the `(448, 0)` page), `bubu2` (the CLUT rows), the `ed*` ending
scenes (mostly the `(320, 256)` page) and the non-field `other4..6` /
`befect_data` blocks; every other scene's TIMs miss the pool entirely.

A host whose scene VRAM lacks the pool draws the fog quads against zero
words, and a textured fragment on a zero word is transparent: the pool is
live, the quads are emitted, and nothing reaches the frame. That was the
native window's state until its scene build gained the underlay
(`crates/engine-shell/src/bin/legaia-engine/window/run.rs`, disc-gated
`window/fog_texture_tests.rs`, which pins both retail hashes).

### Pool density against retail

A per-vsync poll of the pool in `vell`
([`autorun_w1a_fog_pool_poll.lua`](../../scripts/pcsx-redux/autorun_w1a_fog_pool_poll.lua),
1800 vsyncs standing at the `map01` entrance, gate raised, cap `0x18`)
reads the alive-record population at 21 to 38, mean 25.9, with the frame
step `DAT_1F800393` at `2` on every sample - `vell` runs at 30 frames a
second. The live-count word itself is a poor sample: the walk zeroes it
before it counts (`sw zero,0x990(gp)` at `0x8003F398`) and adds as it goes
(`sw v0,0x990(gp)` at `0x8003F3C8`), so a vsync that lands inside the walk
reads a partial count. The spawner compares that word as the last walk left
it, so one frame's spawns can overshoot the cap by what one frame can
spawn - which is all retail does.

The emitter and the spawner draw BIOS `rand()` (`FUN_80056798` is the
`A(2Fh)` thunk), which returns the **high** half of its LCG state,
`(seed >> 16) & 0x7FFF`, and both test low bits of the result (`rand & 0xF`
for the burst gate, `& 7`, `& 0x7F`). A port that hands them a raw 32-bit
LCG state gets bits whose low nibble cycles with period 16: the burst gate
then fails on almost every frame and passes on all 24 draws of one frame,
which dropped fifty-odd records into the pool at once past the stale count -
the engine's `vell` pool read 40 to 62 against retail's 21 to 38 until the
element channel shaped its draws the BIOS way
(`engine-vm::battle_formulas::bios_rand_shape`, the one shaping every
world draw that stands in for a `jal 0x80056798` goes through -
`World::next_rand`). Shaped, the
engine's `vell` pool over 2400 ticks after a 600-tick settle reads 19 to
35, mean 26.4, at most twelve spawns in one tick
(`w1h_fog_gate_census.rs`, `vell_fog_density_tracks_the_retail_poll`).

The cap is not a debug switch. The MAN installer `FUN_8003AEB0` stores
`MAN[1] & 1` into `_DAT_8007B6A8` (`0x8003AF54`, the per-scene save-allow /
overworld flag), then at `0x8003B6BC..0x8003B6E8` writes `0x48` into
`_DAT_8007BCB0` when `MAN[1] & 4` or that flag is set, `0x18` otherwise. So
every kingdom overworld runs the pool at `0x48` - the value every
PCSX-Redux `map01` / `map03` library state holds - and a field scene
reaches it only through header bit 2. Across the 101 scene MANs on the disc
four raise it: `map01`, `map02` and `map03` (bit 0) and `opurud` (bit 2).
The engine seats the cap from the MAN at scene entry
(`fog_particles::fog_cap_for_man`); the census is
`crates/engine-core/tests/scene_host_effect_pool_underlay_disc.rs`.

### The pool on the kingdom overworld

The overworld is a game-mode-3 field-run scene, so the render pass's gate
at `0x80026EA4..0x80026EC4` (`_DAT_8007B83C == 3` and `_DAT_8007B854 != 0`)
passes there exactly as in a field, and retail draws the fog over the
continent. On `keikoku_chest_preload` (`map01`) the gate is raised and all
`0x48` records of the raised cap are alive, every one at a height in
`-0x28 - 0x7F ..= -0x28`: the spawner's overworld arm, not the field arm,
wrote them.

That arm keys on scratchpad `_DAT_1F800394` bit 0. Across the mednafen
library the bit is set on all nine non-battle kingdom-overworld states
(`map01` / `map02` / `map03`, field-run and pause menu) and clear on the
other 89, the battles fought on an overworld included - it is the overworld
flag. Read off `FUN_801D629C`'s disassembly, the bit changes three things:

- **Depth test before the pop.** After the height draw the spawner builds an
  `SVECTOR` of `(tile_x << 7, y, tile_z << 7)` on its stack and transforms
  it with `FUN_8003D344` (`0x801D6460`) - one `MVMVA` of `V0` by the
  rotation matrix plus the translation, i.e. the resident camera. A result
  depth past `0x4000` (`slti v0,v0,0x4001` at `0x801D6470`) ends the spawn
  before the slot pop, having consumed two draws.
- **A further lift.** After the record is filled, `0x801D651C..0x801D653C`
  subtract `0x28` from its height.
- **The emitter's dense profile.** `FUN_801D6058` switches its burst span
  bias and offset from `(2, 1)` to `(6, 0x0E)` on the same bit
  (`cutscene_script_elements::AmbientProfile`).

What puts the fog *ahead* of the player is the emitter's span. The burst
arm samples its tiles across the visible-tile window `0x1F8003E8..EB`,
read afresh every frame (`lb` of `0xD4..0xD7(s0)` with `s0 = 0x1F800314`,
`0x801D6158..0x801D6168`), and `map01`'s entry script (`P1[0]`) sets that
window to `(-18, -12, 18, 32)` (`46 24 EE F4 12 20`) two ops before it
raises the gate (`4C 30`, unconditional). The field default
`(-8, -6, 6, 10)` with the dense profile places every burst behind the
player; the overworld window reaches 32 tiles ahead. On
`keikoku_chest_preload` the pool spans 1075 units behind to 1337 ahead of
the player. Fed that pool, retail's camera words and retail's vertical
offset, the port's render step reproduces the frame's walked fog packets
nearly one for one - see [the sheet is a view-space
billboard](#the-sheet-is-a-view-space-billboard).

The GTE matrix at the spawner's `MVMVA` is read as the camera the frame
draws with (the port uses the last camera its render step projected
through); that attribution is an inference from the operands - the routine
never loads a matrix of its own. The engine models the arm as
`FogPool::overworld` + `FogPool::depth_view`, set from the world mode by the
element channel and from the draw path by `World::fog_render_step`, and
both hosts run the render step in either mode (`World::fog_mode`), handing
it the frame's field-frame pose
(`camera_view::FieldCameraFrame::field_view`). The live window reaches the
emitter as `FogPool::view_window`, published by `Camera::route_camera_events`
(the window is camera state); the world-map frame arm steps the scene
system script the way the field arm does, which is what runs `map01`'s
`P1[0]`. The disc-gated oracle is
`crates/engine-shell/tests/world_map_fog_oracle.rs`.

On the overworld each half-sheet is also depth-tested against the
continent the frame already drew (`FogQuad::depth`, the particle's depth -
the billboard's one depth and the one its OT bucket comes from), because retail links
the sheets into the same ordering table as the terrain; the field keeps its
composite-over-the-frame draw.

Frame-paired at `keikoku_chest_preload`'s seat, retail's haze shows mostly
as the white band above the ridges while the port's sheets read denser and
brighter across the whole frame. The **colour** is not the cause. Each walked
fog packet's modulation colour is `FUN_8003F3FC`'s `grey * tint * brightness
>> 15` of its record rolled back to the pass that built the table (two passes
of the frame step `2`, libgpu double-buffering the table): all 104 packets of
the state's walked ordering table match the engine's kernel
(`FogParticle::sheet_rgb`) that way, median `rgb` 39. What differed was the
sheet's shape and which sheets survive the culls, both corrected below; what
still differs is the draw order and the camera.

#### The sheet is a view-space billboard

`FUN_8003F3FC` transforms the particle through the field view (`0x1F8003C8`)
and stores the result as the translation of the matrix at `0x1F800334`, whose
rotation the walk `FUN_8003F348` set up: the base matrix `_DAT_8007BF10`
(`S * I`, `S = 6`) with `RotMatrixX(0x400)` folded in (`0x8003F374..94`) -
`[[S,0,0],[0,0,-S],[0,S,0]]` in the capture's scratchpad. `FUN_8003F86C` then
`RTPT`s `(dx0, 0, 0x80)` and `(dx1, 0, 0)` through it (`0x8003F86C..E8`), so
the two corners are the particle's view point plus `(dx0 * S, -0x80 * S, 0)`
and `(dx1 * S, 0, 0)`: screen-aligned, both at the particle's depth,
`H * 0x80 * S / vz` pixels tall whatever the camera pitch. The earlier reading
put the corners `0x80` world units above the particle and projected them
through the camera, which foreshortens the sheet by the pitch's cosine (about
`0.84` at `map01`'s pitch) and moves its bottom edge.

On the overworld (`_DAT_1F800394 & 1`, `0x8003F958..0x8003F9A0`) the emitter
also drops a half nearer than `SZ 0x310`, links it at `(SZ - 0x10) >> 5`, and
adds the entry at `*_DAT_8007BB04 + (SZ >> 5) * 2 + 2` to both corners' `SY`
before the row tests. That table is the overworld's screen-Y curvature
([`renderer.md`](renderer.md#frame-setup--present), `FUN_800271A8`); without
it the far sheets at the top of the frame - retail's band above the ridges -
fail the "both above row `0`" test and are never drawn.

Fed the state's pool (rolled back two passes), its camera words, vertical
offset and walk box, the engine's render step now emits 103 of the 104 walked
fog packets, every one within a pixel of retail's and none that retail did not
draw (`crates/engine-core/tests/fog_sheet_colour_retail_capture_disc.rs`, which
also pins the curvature table entry for entry). The one it misses is a
borderline NCLIP case on a sheet `363` pixels wide.

What still separates the frames:

- **Draw order.** Retail links the sheets into the one ordering table with the
  continent, so terrain in a nearer bucket overdraws the sheets behind it: an
  opaque-coverage pass over the walked table hides about a fifth of the fog's
  additive light. The port's overworld sheets are depth-tested per pixel at
  the particle's depth, which hides almost nothing (the native frame's fog
  delta moves from `26.7` to `25.3` of `255` with the test off).
- **The continent draws without the curvature table**, so the corrected
  sheets sit up to the table's entry (about `4` to `26` pixels over the
  spawner's `0x4000` depth range) lower relative to the terrain than retail's
  do.
- **The camera** (below).

The render step subtracts the camera vertical offset `_DAT_8007BCAC` from
every particle height. On `keikoku_chest_preload` it reads 252: the ease
walks toward `scene_ctrl[+0x4A] - player[+0x16]`, the control word reads
`60` and the player stands on the `-192` floor.

The `60` is `map01`'s own entry script. `P1[0]`'s per-frame park loop
(`+0x142..+0x1BD`) opens on `CD F8 00 00 7E 7E`, a whole-map box test on
the player, and its first pass after an entry (system flag `0x528` is
cleared by the prologue and set by that pass) runs `2E 18`, `4C 49 3C 00
00 00`, `2F 18` at `+0x1B0`. The prologue's `2E 19` at `+0x2E` has already
raised `_DAT_1F800394` bit 25, and op `0x4C` sub-9 tests that bit before
bit 24 (`0x801E1488`), so the op takes the delta arm: `sh s0,0x4a(v1)` at
`0x801E14BC` stores `60`, and `0x801E14D4` stores `60 - player[+0x16]`
straight into `_DAT_8007BCAC` (see `ghidra/scripts/funcs/overlay_0897_801de840.txt`).
A PCSX-Redux run of `drake_castle_to_worldmap` across the castle-to-`map01`
entry pins it
([`run_w1a_halt_and_offset_watch.sh offset`](../../scripts/pcsx-redux/run_w1a_halt_and_offset_watch.sh)): the scene reset `FUN_8003A024` zeroes the word at
`0x8003A0D4`, and the only later store is `0x801E14BC`, once, two frames
into game mode 3, from `P1[0]` `+0x1B2` (the player there stands at `-276`,
so the accumulator lands on `336`).

The port used to read `0` / `192` here. Its system-context position anchor
was re-seated on the player only in field mode, so on the overworld the
`CD F8` box test read tile `(-1, -1)`, failed every frame, and the loop
never reached the op. `World::step_field_frame_slice` now seats the anchor
in both modes, and `crates/engine-shell/tests/world_map_camera_offset_oracle.rs`
pins `60` / `252` at the `keikoku_chest_preload` seat.

The region table is MAN section 4 (`DAT_80073ED8`, count `DAT_80073EDC`):
`0xB`-byte records of `[enable][x0][z0][x1][z1][angle base][angle
spread][speed][unread][flag index u16]`; op `0x4C` nibble-C sub-1 rewrites
each record's enable byte from the inverse of its story flag.

Two earlier readings this replaces: `FUN_801D629C` is not "a per-particle
actor" of a separate template family - it allocates no actor and returns at
once - and the pass draws textured sheets, not line packets; the `0x09` in
the packet's first word is the `POLY_FT4` tag length, and the command byte is
`0x2E`. The `overlay_0896_801d629c.txt` dump at the spawner's VA is a
71-instruction fragment of another routine (no prologue, `v0` read before
any write) and is not this function.

Engine: the handler is `engine-core::cutscene_script_elements::AmbientEmitter`
on the element channel `engine-core::world::cutscene_elements`; the producer is
`World::install_field_scene_elements` and the gate sink is
`World::set_ambient_particles_enabled`. The pool is
`engine-core::fog_particles::FogPool` (spawn, walk, update, emit), installed
from section 4 at scene entry (`World::install_fog_regions`) and rendered by
`World::fog_render_step` - which both hosts call from their draw path with
the frame's camera (the field follow pose, or the overworld walk pose),
wrapping the quads through
`engine-ui::screen_prim::fog_puff_prim` into their screen-primitive pass.
Fuller spawn-site provenance is in [`cutscene.md`](cutscene.md); the
disc-wide census of the gate-raising scripts and the two oracles are
`crates/engine-shell/tests/w1h_fog_gate_census.rs` and
`crates/web-viewer/tests/w1h_fog_page_prims.rs`.

## Related

- [`cutscene.md`](cutscene.md) - the element channel the particle emitter
  shares with the position tween and the teardown.
- [`world-map.md`](world-map.md) - the kingdom walker table (ocean).
- [`move-vm.md`](move-vm.md) / [`move-vm-overlay-ext.md`](move-vm-overlay-ext.md) -
  the opcode set the ambient records run on.
- [`effect-vm.md`](effect-vm.md) - the battle-side effect pool (a different
  subsystem; the field ambience never touches it).
- [`../formats/scene-bundles.md`](../formats/scene-bundles.md) - the
  prescript bundle + consumer census.
- [`../formats/asset-type.md`](../formats/asset-type.md) - the type-6 /
  type-7 slot dispatch.
