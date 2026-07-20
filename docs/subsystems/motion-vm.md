# Per-actor motion VMs

**Two** distinct per-actor bytecode VMs live under the field actor tick
`FUN_8003BC08`: the pursue / patrol / face-target VM at `FUN_8003774C`
(dispatched when actor `+0x10 & 0x400`) and the scripted-motion / flag VM at
`FUN_80038158` (dispatched when `+0x10 & 0x80`, bytecode carried in MAN
tail-section 1 - [below](#the-second-motion-vm---fun_80038158)). Both live in
`SCUS_942.54`.

The first drives **per-actor pursue / patrol / face-target** logic - NPC movement on
the field, camera follow paths, and "face the speaker" cinematic posing during
dialog. The second drives scripted actor choreography and writes story flags.

Both are distinct from the other three members of
[the runtime VM family](move-vm.md#the-runtime-vm-family) - the
[actor VM](actor-vm.md), the [move VM](move-vm.md), and the
[field VM](script-vm.md).

**What catches people out: the two motion VMs are ported in three places.**
`FUN_8003774C` is [`legaia_engine_vm::motion_vm`](../../crates/engine-vm/src/motion_vm.rs).
`FUN_80038158` splits: its *static* decode (which stream binds to which
placement, wander pace, default-move harvest) is
[`legaia_engine_core::man_field_scripts::npc_motion`](../../crates/engine-core/src/man_field_scripts/npc_motion.rs),
because its bytecode arrives as MAN tail-section data rather than through the actor
tick's own buffer; its *runtime facing channel* - the ambient idle turns of ops
`0x04` and `0x0D`, plus the ramp scheduler they drive - is
[`legaia_engine_vm::ambient_motion`](../../crates/engine-vm/src/ambient_motion.rs).

## Bytecode layout

Each script entry is `1 + N` bytes:

```text
+0  u8 op_byte         ; bit 0x7F = opcode, bit 0x80 = "select target"
+1  u8 target_id       ; only present if bit 0x80 set in op_byte;
                       ;   special ids: 0xF8 (self), 0xFB (linked)
+N  u8 operand[...]    ; opcode-specific operands
```

When the high bit is set, the VM resolves a target actor before applying the body. `0xF8` resolves to "this actor" (the retail engine reads `_DAT_8007c364` - current player ptr), `0xFB` follows a linked list at `_DAT_8007c34c` looking for a matching record-class signature, and any other id linearly scans the actor list at `_DAT_8007c354` matching against the actor's id field at `+0x14`.

## Opcodes

Dispatch is a 22-entry jump table at `0x80010EE0` indexed by `(op & 0x7F) -
0x37`; opcodes outside `0x37..=0x4C`, and the fifteen table slots pointing at
`0x80037FEC`, all take the default arm.

| byte | case body  | name             | semantics                                |
|------|------------|------------------|------------------------------------------|
| 0x37 | 0x8003789C | TranslateY       | accumulate Y axis by per-frame speed     |
| 0x38 | 0x800379FC | RotateToAngle    | yaw ramps to a compass-LUT entry over a frame budget; shortest-path (`body0 & 0x80`) or forced-direction (`body1 & 0x80`), 12-bit fixed-point |
| 0x41 | 0x8003789C | TranslateX       | accumulate X axis by per-frame speed     |
| 0x43 | 0x80037FF0 | NoOp             | tick budget consumed, no actor mutation  |
| 0x47 | 0x80037B84 | MoveTowardTarget | step actor XZ toward `(tx, tz)`, snapping facing to the compass once per leg / step-direction change |
| 0x4C | 0x80037DE0 | FaceTarget       | yaw ramps to the target's live bearing over a frame budget; sub-mode bytes `0x85` / `0x8E` / `0x8F` are the three retail accepts, and `0x8F` alone forces the decreasing direction instead of the shortest arc |
|      | 0x80037FEC | (default arm)    | terminate with `Done`                    |

The return value carries the outcome: the yield arm at `0x80037FF0` returns
`0` (leg still running), the default arm at `0x80037FEC` increments the flag
first and returns `1`, clearing the actor's HALT bit `0x400` and zeroing the
`+0x54` progress cursor on the way out.

## How an actor's facing changes

Three opcodes write the actor's 12-bit heading at `+0x26`, under **two
distinct laws** - the distinction that decides whether a turn is instant or
animated.

**Snap - the walk-direction-implied facing.** The tail of the `0x47` case
(`0x80037D4C..0x80037DDC`) reduces the step to its axis signs, maps them to
an index in the heading LUT at `0x80073F04`, and writes the entry outright:

| index | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 |
|---|---|---|---|---|---|---|---|---|
| direction | -Z | -X -Z | -X | -X +Z | +Z | +X +Z | +X | +X -Z |

Entry `i` is `i * 0x200` in the retail heading space (`0` = -Z), so a walking
actor's facing is always one of eight compass points and **never an
in-between angle** - there is no walk-turn interpolation anywhere in retail.
The written heading holds **one value per walk leg**: a frame-exact trace of
the town01 Mei dinner walk-on (per-frame `+0x26` off the static recomp)
shows every leg - the straight `-Z` runs and the `+X +Z` diagonal walk-off
alike - carrying a single heading for its whole run, so the port issues the
write at the leg's first moving frame and on a step-direction change (the
dominant-axis → diagonal cut) rather than re-writing per frame. The ops
index the LUT `& 0xF`, but only the first eight slots are direction entries;
the upper half is unrelated SCUS data.

**Ramp - the two dedicated rotate ops.** `0x38` and `0x4C` interpolate toward
a target angle over a frame budget carried in their own operands. Both run the
same arithmetic each tick:

```text
remaining = budget - cursor                  ; cursor at actor +0x54
if remaining - speed <= 0:                   ; terminal frame
    heading = target                         ; exact snap, leg is Done
else:
    cursor += speed
    arc  = (target - heading) mod 0x1000     ; or (heading - target) when decreasing
    heading += arc * speed / remaining       ; or -=
```

`speed` is `_DAT_1F800393`, the per-frame scalar. Because the arc is measured
from the *live* heading every tick, the step is a linear ease that lands
exactly on the target, and the terminal frame snaps rather than steps so
rounding can never leave the actor short. Three properties of that loop are
runtime-pinned frame-exact (the Mei dinner beat's seven authored `0x38`
turns, replayed tick-for-tick by the port including the floor-divide
increment pattern - oracle
`crates/engine-core/tests/recomp_facing_trace.rs`, gated on
`LEGAIA_RECOMP_TRACE_DIR`):

- **The ramp is linear at `arc / budget`** - the per-op turn *rate* is op
  data, not an engine constant. The authored spectrum in one beat alone
  spans ~16 to ~85 units/frame (a deliberate slow 32-frame quarter-turn sits
  beside 18-frame near-half-turns), so a port that hardcodes one turn speed
  is wrong.
- **The heading write-back is raw u16 wrapping - never normalised into
  `0..0xFFF` per tick.** Only the *arc* is measured mod `0x1000`; a
  decreasing ramp through zero holds `0xFFxx` raw headings frame after
  frame, and only the terminal snap lands back in range. Renderers consume
  the angle mod `0x1000`, so the raw hold is invisible on screen, but a port
  that masks every tick diverges from the traced `+0x26` on any
  wrap-crossing turn.
- **The `0x38` endpoint is always a compass entry** (the LUT snap); the
  `0x4C` endpoint is the live arctan bearing and is generally NOT
  compass-aligned - the interact face-the-player write lands on values like
  `1075`, and nothing re-snaps it.

The two ops differ only in what they aim at and how they choose a direction:

| | `0x38` RotateToAngle | `0x4C` FaceTarget |
|---|---|---|
| target | heading-LUT index `body0 & 0xF` | live bearing to the resolved target actor, `FUN_80019B28(self.z, self.x, tgt.z, tgt.x) + 0x800` |
| budget | `body1 & 0x7F` (7-bit) | `body1 \| body2 << 8` (16-bit), target id in `body3` |
| direction | `body1 & 0x80` forces it, unless `body0 & 0x80` opts into shortest-path (decrease when the increasing arc exceeds `0x800`) | always the shortest arc, unless sub-mode `0x8F` forces decreasing |

Because `0x4C` re-reads the bearing each tick, a FaceTarget leg tracks a
target that is itself moving.

The `+0x800` on the bearing is the convention shift, not a fudge:
`FUN_80019B28` returns `0` = +Z (quadrant-decomposed against the arctangent
table at `0x8006F4C8`, indexed by `min << 11 / max`), while the actor heading
space has `0` = -Z. The engine's `render_26` space matches the bearing's, so
the port carries the half-turn on the LUT instead.

## Per-frame speed

`DAT_1f800393` is the per-frame speed scalar (also drives the [move VM](move-vm.md) frame-time scratchpad). The motion VM consumes it as the budget for incremental motion - engines update once per frame, the VM consumes per opcode.

## Clean-room port

[`legaia_engine_vm::motion_vm`](../../crates/engine-vm/src/motion_vm.rs) is the clean-room port. All six opcodes are implemented: `0x37` `TranslateY`, `0x38` `RotateToAngle`, `0x41` `TranslateX`, `0x43` `NoOp`, `0x47` `MoveTowardTarget`, `0x4C` `FaceTarget`. Each step returns `StepResult::Yield` (budget consumed, resume next tick) or `StepResult::Done` (terminal op / default arm); there is no fallback path.

The facing law above is `heading_lut_engine` (the eight compass entries, carried in the engine's `0` = +Z space), `walk_facing_index` / `walk_facing_yaw` (the `0x47` sign-to-index table), and `rotate_step` (the shared ramp arithmetic, widened to 32-bit so a large speed cannot overflow the increment, raw wrapping write-back). `engine-core`'s `facing_index_to_engine_heading` delegates to the same LUT, so the spawn-prologue facings and the runtime ones cannot drift apart. `MotionState` carries the once-per-leg walk-facing latch (`walk_facing`) and a per-step `yaw_written` signal; engine hosts gate their render-heading mirror on the latter, so a heading another writer posed (the interact bearing) is not clobbered by an idle leg's stale VM yaw.

Two deliberate departures: LUT indices `8..=15` are treated as no-ops rather than reproducing retail's overread into adjacent SCUS data, and an unrecognised `0x4C` sub-mode terminates the leg instead of yielding forever the way retail's inert arm does.

## Engine consumers

The runtime [`Camera`](../../crates/engine-core/src/camera.rs) in `engine-core` consumes:

- The field-VM op-`0x45` event stream (`CameraConfigure` / `CameraSave` / `CameraLoad` / `CameraApply`) for the high-level camera state.
- The motion VM (optional) for cinematic pre-baked camera paths via `Camera::tick_script`.

The default mode follows a target actor slot at a configured distance + height.

### Field-NPC walking

**Initial seats come from the spawn-prologue pre-run, not the placement header.**
Retail's placement installer (`FUN_8003A1E4`) runs each partition-1 record's
story-flag-tested opening ops through the field VM at scene load: a `0x23 MoveTo`
to the parked-sentinel tile `(0x7F,0x7F)` despawns the actor for the current story
state, and cross-scene `MoveTo`s seat an actor away from its MAN header tile.
Runtime-pinned against the retail `town01` field-actor list (`_DAT_8007C354`
class; a large share of the placements stand parked or relocated from frame one).
The engine mirror is `World::pre_run_field_channel_prologues` (scene entry, after
the carrier/channel install): one field-VM frame slice per placement channel, run
unconditionally - load-time behaviour, not the opt-in free-roam liveliness - with
position write-through, patrol-route invalidation when the executed branch
contradicts the flag-blind route decode, and no heading derivation (facings come
from `seed_field_npc_facings`). Disc + save-library oracle:
`crates/engine-core/tests/field_npc_entry_positions_disc.rs`.

`World::tick_field_npc_motions` (`engine-core`) drives MAN-placed field NPCs through the `0x47` `MoveTowardTarget` pursue step, one motion-VM step per field tick, writing the live position back into `World::field_npc_positions` so the moving NPC's ±40-unit collision box and its interact box follow it (retail probes the live `+0x14`/`+0x18`, not the spawn anchor). Four start paths feed it:

- **Autonomous patrol routes** (`World::field_npc_routes`, gated by `World::animate_field_npcs` / `play-window --live-npcs`): each placement's own pre-text script bytecode carries `0x4C 0x51` NPC move-to-tile ops; `man_field_scripts::placement_motion_route` decodes the local waypoints (dropping the `(127,127)` park sentinel, cross-context targets, and beyond-locality story-relocation branches) and the engine loops them as a patrol. Autonomous legs pause while a dialogue is up - retail's interaction motion-pause kick (`FUN_8003c9ac` reloading every moving-class actor's pause timer on the touch event post).
- **Interaction-prologue runs**: when the opt-in field-VM dialogue runner executes an NPC's record and the prologue hits a `0x4C 0x51` with the NPC arm, the host hook (`vm_hosts::FieldHostImpl::op4c_n5_sub1_npc_run`) starts the interacted actor's walk leg. These run through the dialogue - they are the interaction's choreography.
- **Actor-VM `start_motion`** (op `0x09` `MotionAt`, retail `FUN_800358c0`): `World::start_actor_motion` records the glide target and steps the actor's sprite position toward it through the same pursue kernel (`World::tick_actor_motions`).
- **Cutscene-timeline cross-context walks** (`C7 <id> <tx> <tz> <mode>`, the targeted `0x47` yield): a spawned partition-2 record walking a cast member. The timeline arms the leg with the op's own speed (`0x80 >> (2 + (mode & 7))`), PARKS on `CutsceneTimeline::walk_wait`, and resumes past the yield when the leg arrives - the retail shape where the yield-op pointer lands in the target's `+0x94` and the walk kernel moves it in place. The player-anchor form (`C7 F8 …`) steps the player actor directly in the same park. Scripted legs keep stepping while a timeline is active; only the autonomous patrol kicks stand down.
- **Cutscene-timeline cross-context turns** (`B8 <id> <dir|flags> <budget|dir>`, the targeted `0x38` yield - the field-VM CAM_CFG halt-acquire arm): same park shape for the rotate op. The timeline arms a `0x38` RotateToAngle leg on the NPC (`CutsceneTimeline::facing_wait`), steps it once per tick writing the raw yaw into `World::field_npc_headings`, and resumes past the yield on the terminal compass snap - one parked tick per budget frame, the 1:1 retail duration. The simple path (`op1 & 0x7F == 0`) is an instant compass write, no park.

The start kernel (`World::start_field_npc_motion`) mirrors the `FUN_800358c0` shape - write the target, reset the glide cursor - and the per-frame consumer is this VM.

On interaction start the host also poses the spoken-to NPC toward the player,
the live driver for the "face the speaker" posing named above:
`World::face_field_npc_toward` runs the `0x4C` FaceTarget op for one step,
rotating the NPC's heading to the player's bearing and settling it into
`field_npc_headings` (which the renderer reads). Separately, the interacted
`0x4C 0x51` op's byte-+4 move-anim id (retail actor `+0x5C`, consumed by the
anim-stream stepper `FUN_800204F8`) is carried onto the started glide leg as a
`field_npc_anim_cues` entry (`carry_npc_run_anim`) instead of being dropped.
The retail speed encoding is pinned: the ops carry their own base-step selector (`(op0>>5 & 4)|(op1>>6)` for 0x37/0x41, `b2 & 7` for 0x47; numerator `0x80`, or `0x40` for 0x41)
and are the field-VM record's yield-class bytes interpreted in place from the pointer parked at actor `+0x94` -
see [field-locomotion.md](field-locomotion.md#npc-glide-speed) + [script-vm.md](script-vm.md) § 0x37-0x42.
Per-actor field-VM channels are not modelled - the engine drives the decoded waypoint list, pacing each placement by its **real decoded walk-kernel step**
(`man_field_scripts::placement_glide_speed`: the bound tail-section-1 stream's wander/step ops first, then the record's own yield ops,
then the facing-nibble heuristic only for placements with no walk-kernel op at all).

## The second motion VM - `FUN_80038158`

A separate per-actor bytecode interpreter (dispatched by `FUN_8003BC08` when
actor `+0x10 & 0x80`), reading its stream at `*(u32*)(actor+0x80) +
*(u16*)(actor+0x84)`. It choreographs scripted actor motion (directional
steps, facing ramps, tweens, teleports, waits) AND writes the system
story-flag bank `DAT_80085758` directly: op-`7` `[07, lo, hi]` **sets** flag
`lo | hi << 8`, op-`8` **clears** (byte `= idx >> 3`, bit `= 0x80 >> (idx &
7)`, matching `FUN_8003CE08`).

### Disc carrier - MAN tail-section 1

The only disc source of this bytecode is **MAN tail-section 1** (parser
`legaia_asset::man_motion`):

- `FUN_8003AEB0` (scene-entry map-init) computes `tail_base = MAN + 0x2B +
  3*(N0+N1+N2) + u24_at_0x28`, walks the u24-length-prefixed section chain,
  and installs section 1's body at the field control block
  (`*_DAT_801C6EA4` `+0x00`).
- `FUN_8003A9D4` walks the body as a record chain `[u8 count][s16
  next_delta][count x (u8 actor_id, u8 enable)][motion stream]`, terminated
  by `count == 0`. Per binding it installs `actor+0x80 = record + 3 +
  2*count` and the enable byte at `+0x8A` (bit 0 gates the tick). Actor
  resolution: `0xF8` = player (`_DAT_8007C364`), `0xFB` = first
  `_DAT_8007C34C` node ticking the world-map entity SM `0x801DA51C`, else
  the `_DAT_8007C354` field actor whose `+0x50` equals the binding byte.
  The placement spawner `FUN_8003A1E4` writes `+0x50 = N0 +
  placement_index` (`N0` = the MAN's partition-0 record count, passed down
  from the `FUN_8003AEB0` header decode), so a binding resolves to
  partition-1 placement `actor_id - N0` - the id space is offset by the
  object records, NOT the raw partition-1 index (town01: `N0 = 36`, binding
  `0x30` = placement 12). Engine decoders:
  `man_field_scripts::placement_wander_step` /
  `motion_default_move_writes`.
- The stream opens with a **variant header table** `[u16 selector][s16
  delta]...`: the interpreter preamble picks the first variant whose
  `DAT_80085758` flag is set (`0xFFFF` = default/terminator); bytecode starts
  at `header + 4` and is re-selected every tick (live flag-driven variant
  swap). The flag test uses the full u16 selector; the change-detect compares
  `& 0xFFF`.
- All other `actor+0x80` writers are eliminated: `FUN_8003A55C` /
  `FUN_8003A1E4` / `FUN_8003AB2C` / `FUN_8003BDE0` zero it; `FUN_8003C6A4`
  and `overlay_0896_801cd520` write `_DAT_8007C34C`-list nodes with different
  layouts; the field VM `FUN_801DE840` never writes it.

### Op widths

| width | ops |
|---|---|
| 1 | `0x01` end/loop-back |
| 2 | `0x05` wait, `0x10`/`0x11`/`0x12` bit set/clear/wait |
| 3 | `0x02` anim/timer, `0x03`/`0x19`/`0x20` directional step, `0x04` facing ramp, `0x07` SET flag, `0x08` CLEAR flag, `0x09` post u16 to the `DAT_8007B6D8` ring (`FUN_80035B50`), `0x0A`/`0x0B` actor-flag +/-`0x1000000`, `0x0E` model swap, `0x0F` tile teleport, `0x17` default-move pair write |
| 4 | `0x0D` facing ramp + tween channel |
| 5 | `0x06` pad-echo step, `0x14`/`0x15`/`0x16` tween installs, `0x18` AABB wander |
| 8 | `0x0C` glide-channel install |
| 13 | `0x13` `FUN_80058490` call |

### Walk-op speed encoding

Every walk op steps on the same `0x80 >> (2 + bits)` per-tick ladder as the
`FUN_8003774C` yield ops, with the base-step selector carried in its own
operands. The directional steps `0x03`/`0x19`/`0x20` hold `bits` in operand
byte 1's low nibble; the pad-echo step `0x06` and the AABB wander `0x18`
scatter the same 4-bit selector over their four operand bytes' high bits
(`(b1&0x80)>>4 | (b2&0x80)>>5 | (b3&0x80)>>6 | b4>>7`), leaving the low seven
bits for the pad-echo offsets / wander-box tiles.

This is the disc source of a town NPC's ambient wander pace. The static
decode is `man_field_scripts::placement_wander_step` →
`World::field_npc_glide_speeds` (default variant first, then the gated
variants in table order); the runtime semantics are [below](#the-walk-half---the-directional-steps-and-the-aabb-wander).

### The walk half - the directional steps and the AABB wander

These are what a fresh town villager actually runs, and the reason "why does
nobody move" is a walk question rather than a facing one. Dispatch for all of
them is the same `0x80010FE8` table: `0x03`, `0x19` and `0x20` share one case
body at `0x800383F8`, and `0x18` has its own at `0x80038B90`.

Two tables sit back to back at `0x80073F04` and drive every walk op. The
first is the eight-entry compass LUT the facing ops also use; the second,
starting sixteen bytes later at `0x80073F14`, is the **axis bitmask** table
(`1` = `+Z`, `2` = `-Z`, `4` = `+X`, `8` = `-X`) that every walk op reduces
its heading index through before touching a coordinate. Entry `i` of the
bitmask table is exactly entry `i` of the compass in bit form, which is why
diagonal indices move both axes by the same per-tick step. The two tables
being adjacent is also what a `& 0xF` index overreads into.

#### Ops `0x03` / `0x19` / `0x20` `[op, b1, b2]` - the directional step

```text
lut    = b1 >> 4                  ; heading-LUT index
bits   = b1 & 0x0F                ; pace selector
shift  = bits + 2
budget = (b2 & 0x3F) << shift     ; ticks - 0x20 halves it
step   = 0x80 >> shift            ; units per tick
```

The product `budget * step` is `(b2 & 0x3F) * 0x80` whatever `bits` is, so
**`b2 & 0x3F` is the leg's length in 128-unit tiles and `bits` only sets the
pace**. The three ops differ in exactly two flags: `0x03` additionally snaps
the heading to `LUT[lut]` on every tick, and `0x20` halves the budget (so it
walks half the authored distance). `0x19` moves without touching the facing.
Only `0x03` and `0x19` appear in the disc corpus; `0x20` is authored nowhere.

The cursor at `+0x8B` is `+1` per tick, **not** `_DAT_1F800393`-scaled - the
same asymmetry op `0x04` has against the `FUN_8003774C` ramps, so a leg is
the same number of ticks and covers the same distance at any frame scalar.
The cursor is a `u8` compared `& 0xFF`, so a budget of 256 or more could
never retire; no authored operand reaches it.

#### Op `0x18` `[18, b1, b2, b3, b4]` - the AABB wander

The four operand bytes carry a tile-space box in their low seven bits
(`min_x`, `min_z`, `max_x`, `max_z`, each read as `tile << 7 | 0x40` - a tile
*centre*), and the 4-bit pace selector scattered over their high bits. On the
op's first tick an actor seated outside its own box retires the op
untouched, so a story-relocated placement simply never wanders.

Inside the box a three-phase machine runs, its phase byte living in the
actor's default-move record (`0x801C6470[slot]` byte 2) rather than on the
actor - which is why a `0x17` write and a wander share a record:

1. **Pick.** Draw `rand() & 6` - a cardinal only, never a diagonal - and
   reject it if a half-tile probe that way would leave the box. A rejected
   draw **retires the op** rather than redrawing.
2. **Turn.** Rotate to the picked compass point at `0x1000 >> (bits + 2)`
   per tick. Skipped outright when the actor already faces that way.
3. **Walk.** Step `0x80 >> (bits + 2)` units for `2 << bits` ticks - 64
   units, half a tile, whatever the pace - then flip a coin: half the time
   the op retires, half the time it drops back to phase 1 and keeps going.

Two things separate the wander's turn from the ambient facing ops. It takes
the **shortest arc** (measuring the increasing arc against `0x800` and going
the other way when it is wider), where `0x04` and `0x0D` take the authored
direction with no override. And it **masks the heading into `0..0xFFF` on
every tick**, where those two deliberately hold raw out-of-range values
mid-ramp. A port that generalises either property from the facing ops to the
wander is wrong.

#### What the walk ops collide with

Both walk ops probe through `FUN_801cf8ac`, and its only subject is the
**player actor** - not the walkability grid, and not other NPCs. The
directional steps probe the single `DAT_801F2254` compass point for their
heading index; the wander probes the three-point fan of `DAT_801F21B4` row
`dir` through `FUN_801d5a68`, which ORs three `FUN_801cf8ac` calls. Both
apply the shared `(x + dx, z - dz)` convention around the walking actor and
accept a hit inside the ±40 moving-actor box. These are the same two tables
the player's own locomotion reads (see
[field-locomotion.md](field-locomotion.md)).

So an ambient walker's containment is the AABB its op authored, and the only
thing that can stop a step is the player standing in it. A blocked
directional step re-runs its op next tick without advancing the cursor or the
PC; a blocked wander drops back to the pick phase.

#### Clean-room port + wiring

[`legaia_engine_vm::ambient_motion`](../../crates/engine-vm/src/ambient_motion.rs)
executes all four walk ops alongside the facing ones, plus `0x17`. The
collision service is the `AmbientBlocking` trait the host supplies, and
`engine-core`'s implementation is the player box test above.

The engine's opt-in liveliness (`World::animate_field_npcs` /
`play-window --live-npcs`) gates the **mirror**, not the interpreter: with it
off the stream still runs its walk ops at their authored cadence, but neither
the position nor the walk-implied facing is published, so nothing on screen
moves. Suppressing the ops instead would stall a stream on its first walking
op - a blocked directional step re-runs forever without advancing its PC -
and a `0x07`/`0x08` story-flag write further down it would then never fire.

That gate distinguishes the two facings this VM writes, which is why the VM
reports them separately (`AmbientMotion::walk_yaw`). The `0x04` / `0x0D`
ramps are ambient turning in their own right and mirror either way; a walk
op's heading write is *walk-direction-implied* facing and means nothing apart
from the step it accompanies. Publishing it while suppressing the step makes
an NPC pivot on the spot through a motion it never performs - and in the
scene where it matters most the fresh-game variants author no turning at all,
so that pivot would be pure artefact rather than retail behaviour.

`World::tick_field_npc_ambient` writes the VM's live position back into
`World::field_npc_positions` on any tick that moved it, so the wandering
NPC's own collision box and its interact box follow it. Because retail
dispatches the two motion VMs off *different* actor flag bits (`+0x10 & 0x80`
here, `& 0x400` for `FUN_8003774C`), no actor is ever walked by both: the
engine stands its autonomous patrol substitute down for any placement bound
to a stream that carries a walk op. Scripted legs still outrank both.

One ordering trap the wander's absolute box makes fatal: the ambient
channels install with the scene carriers, *before* the spawn-prologue pre-run
relocates the story-parked and story-moved placements. `World::pre_run_field_channel_prologues`
therefore re-seats every not-yet-started channel
(`resync_ambient_start_positions`, the position sibling of
`resync_ambient_start_headings`) - a channel left holding the raw MAN header
tile retires its wander on the first tick and the villager silently never
moves.

Disc oracle:
[`crates/engine-core/tests/field_npc_ambient_wander_disc.rs`](../../crates/engine-core/tests/field_npc_ambient_wander_disc.rs).

### The ambient VM's own facing ops

`FUN_80038158` writes `+0x26` from the same `0x80073F04` compass, under the
same two laws. Dispatch for both ramp ops is the 32-entry jump table at
`0x80010FE8` indexed by `op - 1`.

- **Snap.** The directional steps `0x03`/`0x19`/`0x20` and the wander/pad-echo
  ops set the heading from their operand's LUT index as they move.
- **Ramp.** Ops `0x04` and `0x0D`, below. Both aim at `LUT[b1 & 7]` and both
  read the direction from `b1 & 0x80` (set = decreasing). Neither has a
  shortest-arc override, so an authored turn can deliberately take the long
  way round - unlike the `FUN_8003774C` `0x38` sibling, which opts into
  shortest-path on `body0 & 0x80`. Because the LUT index is masked `& 7`,
  these ops cannot reach the adjacent SCUS data that the sibling VM's
  `& 0xF` index can.

#### Op `0x04` `[04, b1, b2]` - the in-VM ramp

Case body `0x8003859C`, arithmetic at `0x800385D0..0x800386A0`. Per tick:

```text
frames    = b2 & 0x7F                        ; frame budget
remaining = frames - cursor                  ; cursor at actor +0x8B, u8
target    = LUT[b1 & 7]
cursor   += 1                                ; unit-per-tick
if remaining == 0:                           ; terminal
    heading = target                         ; exact snap
    cursor  = 0 ; pc += 3 ; next op runs THIS tick
else:
    arc      = (target - heading) mod 0x1000 ; or (heading - target)
    heading += arc / remaining               ; or -=, raw u16 wrapping
    yield
```

The **cursor is unit-per-tick** (`addiu a0, a0, 1`), *not*
`_DAT_1F800393`-scaled the way the `FUN_8003774C` ramps are, so a budget of
24 is 24 stepping ticks at any frame scalar. The leg therefore always runs
`(b2 & 0x7F) + 1` ticks: the budget in stepping ticks plus one terminal
tick, and that terminal tick does **not** consume the frame (retail never
increments the did-work counter `s8` on that arm), so the snap and the
following op execute together.

Each tick also reloads the actor's requested-move / anim pair
(`+0x88`/`+0x5C`) from the per-actor default-move record while it is set -
the same `0x801C6470` table op `0x17` writes.

#### Op `0x0D` `[0D, b1, b2, b3]` - pre-unwrap + tween

Case body `0x800386A4..0x80038828`. This op never moves the heading itself.
On its first tick it pre-unwraps the live heading past the `0x1000` boundary
so that a plain linear interpolation travels the authored way, then hands
`&actor+0x26` to the generic ramp scheduler (an inlined `FUN_8003C5F0`):

```text
cursor16 = actor+0x8B | actor+0xB7 << 8
if cursor16 == 0:                                     ; install tick, once
    target = LUT[b1 & 7]
    if b1 & 0x80:  if heading < target: heading += 0x1000   ; go decreasing
    else:          if target < heading: heading -= 0x1000   ; go increasing
    install ramp { dest=&heading, start=heading, end=target,
                   total=(s16)(b2 | b3 << 8), kind=2 }
if cursor16 >= (s16)(b2 | b3 << 8):                   ; terminal
    cursor16 = 0 ; pc += 4 ; next op runs THIS tick
else:
    cursor16 += _DAT_1F800393 ; yield
```

`0x0D`'s wait cursor **is** frame-scalar-driven - the opposite of `0x04`'s -
and that is what keeps it in lockstep with the scheduler, which decrements
its own countdown by the same scalar. Op and ramp retire together; the op
outlives its ramp by exactly one tick, so the leg runs
`ceil(duration / speed) + 1` ticks. A `duration` of `0` is an instant
compass write.

#### Masking

Neither op normalises the heading per tick. `0x04`'s write-back is raw `u16`
wrapping, so a decreasing ramp through zero holds `0xFFxx` for its whole run.
`0x0D` goes further: the pre-unwrap deliberately parks the heading *outside*
`0..0xFFF` (up to `0x1FFF`, or negative stored as `0xFxxx`) and the scheduler
interpolates on that raw value, so raw headings above `0x1000` are observable
live mid-turn. Only the endpoint lands back in range, written as the LUT
entry verbatim. Renderers consume `+0x26 & 0xFFF`, so none of this shows on
screen - but a port that masks per tick diverges from the traced `+0x26` on
every wrap-crossing turn. The arc measurement inside `0x04` *is* taken mod
`0x1000`, which is why a raw out-of-range heading still feeds back correctly.

#### The generic ramp scheduler

`FUN_80036D80` walks the 64-slot pool at `0x801C66A0` (stride `0x20`; slot 0
is the intrusive list header, leaving 63 allocatable) once per frame:

```text
remaining -= _DAT_1F800393
if remaining <= 0:  value = end ; free the slot
else:               value = end + (start - end) * remaining / total
store value to *dest, width per kind (1=u8, 2=u16, 3=packed RGB, 4=u32)
```

The divide truncates toward zero (MIPS `div`) and `total` is never
rewritten, so this is a straight lerp off the install-time endpoints rather
than an incremental accumulation. The heading channel is `kind == 2` (`sh`).
A slot whose owning actor has `+0x10 & 8` set is freed unticked; an install
with no free slot bumps the `DAT_80073ED0` overflow counter and is dropped.
Installers are `FUN_8003C5F0` and this op's inlined copy; the pool is reset
by `FUN_8003CDA8` at scene entry.

#### Clean-room port

[`legaia_engine_vm::ambient_motion`](../../crates/engine-vm/src/ambient_motion.rs)
executes both ops plus the `0x05` wait, the `0x01` restart, the `0x17`
default-move write and the four [walk ops](#the-walk-half---the-directional-steps-and-the-aabb-wander),
and carries the scheduler as `RampScheduler`. Ops it still does not model are
stepped over by `legaia_asset::man_motion::op_width` without consuming the
tick; a per-tick op budget stops a stream whose real yield op is one of those
from spinning. The pool is ticked in slot order rather
than through retail's linked list - observable only if two live ramps shared
a destination, which one actor's heading channel cannot do.

#### Engine wiring

The host keeps one channel per placement slot (`World::field_npc_ambient`),
seeded at scene load by `World::seed_field_npc_ambient` from the MAN's
tail-section-1 streams through the installer's binding law
(`actor_id = N0 + placement_index`). The whole variant table is carried, not
just the fresh-game one, because the interpreter preamble re-selects the live
variant **every tick** against `DAT_80085758`; a swap reseeds the cursor
rather than resuming the old variant's offset into new bytecode.

`World::tick_field_npc_ambient` steps each channel on the **actor game tick**
(see [`actor-vm.md`](actor-vm.md#tick-cadence-dat_1f800393)), not per rendered
frame - that is what keeps `0x0D` in lockstep with the ramp scheduler, since
both advance by the same `DAT_1F800393`. Op `0x04`'s unit-per-tick cursor is
unaffected by the same scalar, which is the retail asymmetry, not a port
artefact.

Where the facing ops sit in the corpus is worth knowing before debugging a
"nobody turns" report. In `town01` the **default** (fresh-game) variants carry
no facing op at all - they are `0x17` default-move, `0x05` wait, `0x18` AABB
wander - so a fresh Rim Elm villager's ambient behaviour is *wandering*, and
the `0x04` ramps live in **flag-gated** variants, i.e. later story states. A
fresh save showing no turning NPCs is therefore the authored behaviour, not
broken wiring: the thing to check on that report is the
[walk half](#the-walk-half---the-directional-steps-and-the-aabb-wander)
instead. The gated turning path is exercised in
[`crates/engine-core/tests/field_npc_ambient_idle_disc.rs`](../../crates/engine-core/tests/field_npc_ambient_idle_disc.rs).

The channel's `render_heading()` is mirrored into `World::field_npc_headings`
- converted out of retail heading space into the engine's `render_26` space
(`+0x800`) - **only on a tick that actually moved the heading**. That gate is
the ambient sibling of `MotionState::yaw_written`: an NPC parked in a `0x05`
wait must not re-stamp its stale heading over a pose another writer set (the
interact face-the-speaker bearing being the case that regressed before).

Disc oracle:
[`crates/engine-vm/tests/ambient_motion_disc_oracle.rs`](../../crates/engine-vm/tests/ambient_motion_disc_oracle.rs)
re-derives the disc-wide census (every scene bundle's MAN → tail-section 1 →
every record → every gated variant → every `0x04`/`0x0D` site) and replays
each site from four start headings at three frame scalars, asserting the
compass endpoint, the scalar-invariant `0x04` cadence, the scalar-driven
`0x0D` cadence, the swept arc equalling the authored (not complementary)
arc, and the raw out-of-range hold on every wrap-crossing leg.

### Op `0x17` - the per-actor default-move table

`[0x17, move_id, anim_id]` writes `0x801C6470[actor(+0x50) * 4] = move_id`
/ `+1 = anim_id`, guarded `+0x50 < 0x8C` (the table's 0x8C-record arena;
`0x8C` is also the "unset" sentinel the variant-swap preamble reseeds a
record to). The walk/anim ops (`0x02`, `0x03`/`0x19`/`0x20`, `0x18` phase
1/3) reload the actor's requested-move pair `+0x88`/`+0x5C` from the
record while it is set, and the interaction motion-pause kick
`FUN_8003C9AC` (ported at `legaia_engine_vm::motion_pause`) sweeps the same
table on the touch-event post. The engine statically harvests each
stream's first `0x17` per bound placement
(`man_field_scripts::motion_default_move_writes` →
`World::field_npc_default_moves`), keyed by placement slot (`actor_id -
N0`).

No op writes `DAT_8007B7FC`-class globals - op-`9` only posts to the 4-slot
`DAT_8007B6D8` ring, so the battle-id write has no motion-VM analogue.

### The touch post - `FUN_8003D038`

The field collision probe `FUN_801CFC40` calls `func_0x8003d038` with the
touched actor's bind-record index (`other[+0x50]`) whenever two actors
overlap. The body is one guarded store into `DAT_80073F1C`, the one-slot
mailbox the motion VM's wait-for-touch arm (`0x8003882C`, inside
`FUN_80038158`) consumes and resets.

The guard reads the record's **class byte** - the first byte of the
4-byte record at `DAT_801C6470 + index*4`, the same table op `0x17`
writes - and drops the post when it is `0x8C`. Since `0x8C` is the
"unset" sentinel the variant-swap preamble reseeds a record to, the
filter means a placement with **no move assigned** can be walked into
without ever waking a script waiting on a touch. The mailbox keeps its
previous contents in that case; the post is dropped, not cleared.

Retail does no bounds check on the index. The port
(`legaia_engine_vm::motion_vm::post_touch`) returns "no post" for an
out-of-range index rather than reading past the table, which is the safe
reading of the same outcome.

That port is **not wired**: the engine's field collision path resolves
per-axis walls and stops without identifying which actor it hit, so
nothing calls `post_touch` and no script can currently wake on a touch.
Wiring it needs an actor-vs-actor overlap test standing in for
`FUN_801CFC40`.

Provenance: `ghidra/scripts/funcs/8003d038.txt`; the consumer at
`0x8003882C` is inside `ghidra/scripts/funcs/80038158.txt`.

### Flag census

`legaia-engine man-scripts --motion-flag-census` sweeps every scene MAN
(including v12-embedded MANs) and reports all op-7/op-8 sites (sibling of
`--system-flag-census`, which covers the MAN field-VM ops `0x50/0x60/0x70`).
Disc-wide the op-7/op-8 surface is overworld walking-band choreography
(`map02`/`map03`: `0x466`/`0x467` toggles, the `0x56D..0x570` self-advancing
4-phase cycle, `0x5A2..0x5A7` one-shot latches) plus one `town0b` clear of
`0x23F`. The spine gate flags appear in **no** motion stream - their writers
are field-VM script bytes in the streaming variant MAN carriers
(`0x142`/`0x482`/`0x1BE`; see
[`script-vm.md`](script-vm.md#a-second-script-byte-carrier-the-streaming-variant-man))
or, for the town01 opening one-shot `549`, a direct code path (see
[`world-map.md`](world-map.md), "Gate-flag setters beyond a scene's bundle MAN").
Disc-gated anchor test: `crates/engine-core/tests/motion_flag_census_disc.rs`.

## Provenance

- `ghidra/scripts/funcs/8003774c.txt` - full disassembly + decompilation.
- `ghidra/scripts/funcs/80038158.txt` - the second VM's interpreter.
- `ghidra/scripts/funcs/8003a9d4.txt` - motion-script installer (record chain + actor binding).
- `ghidra/scripts/funcs/8003c5f0.txt` - the ramp-scheduler installer op `0x0D` inlines.
- `ghidra/scripts/funcs/overlay_cutscene_dialogue_801cf8ac.txt` - the walk
  ops' player box test (field overlay; the `801cf8ac.txt` SCUS-space dump is
  a different function at an aliased address).
- `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d5a68.txt` - the
  wander's three-point fan over the same test.
- The scheduler tick `FUN_80036D80` and the pool reset `FUN_8003CDA8` have no
  standalone dump; both are plain `SCUS_942.54` bodies at those addresses.
  The two walk LUTs at `0x80073F04` / `0x80073F14` are plain `SCUS_942.54`
  rodata.

## See also

**Reference** -
[Move-table VM](move-vm.md) ·
[Actor VM](actor-vm.md) ·
[World-map controller](world-map.md)
