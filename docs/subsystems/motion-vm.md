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

**What catches people out: the two motion VMs are ported in two different crates.**
`FUN_8003774C` is [`legaia_engine_vm::motion_vm`](../../crates/engine-vm/src/motion_vm.rs);
`FUN_80038158` is
[`legaia_engine_core::man_field_scripts::npc_motion`](../../crates/engine-core/src/man_field_scripts/npc_motion.rs),
because its bytecode arrives as MAN tail-section data rather than through the actor
tick's own buffer.

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
| 0x47 | 0x80037B84 | MoveTowardTarget | step actor XZ toward `(tx, tz)`, snapping facing to the compass each moving frame |
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
(`0x80037D4C..0x80037DDC`) runs on every frame the actor moved. It reduces the
step to its axis signs, maps them to an index in the heading LUT at
`0x80073F04`, and writes the entry outright:

| index | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 |
|---|---|---|---|---|---|---|---|---|
| direction | -Z | -X -Z | -X | -X +Z | +Z | +X +Z | +X | +X -Z |

Entry `i` is `i * 0x200` in the retail heading space (`0` = -Z), so a walking
actor's facing is always one of eight compass points and **never an
in-between angle** - there is no walk-turn interpolation anywhere in retail.
The ops index the LUT `& 0xF`, but only the first eight slots are direction
entries; the upper half is unrelated SCUS data.

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
rounding can never leave the actor short. The two differ only in what they
aim at and how they choose a direction:

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

The facing law above is `heading_lut_engine` (the eight compass entries, carried in the engine's `0` = +Z space), `walk_facing_index` / `walk_facing_yaw` (the `0x47` sign-to-index table), and `rotate_step` (the shared ramp arithmetic, widened to 32-bit so a large speed cannot overflow the increment). `engine-core`'s `facing_index_to_engine_heading` delegates to the same LUT, so the spawn-prologue facings and the runtime ones cannot drift apart.

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

The walk ops step on the same `0x80 >> (2 + bits)` per-frame ladder as the
`FUN_8003774C` yield ops, with the base-step selector in their own
operands: the directional steps `0x03`/`0x19`/`0x20` carry `bits` in
operand byte 1's low nibble (byte 1's high nibble = the heading-LUT index,
byte 2 & 0x3F × `4 << bits` = the frame budget; `0x20` halves the budget),
while the pad-echo step `0x06` and the AABB wander `0x18` scatter a 4-bit
selector over their four operand bytes' high bits (`(b1&0x80)>>4 |
(b2&0x80)>>5 | (b3&0x80)>>6 | b4>>7`; the low 7 bits are the pad-echo
offsets / wander-box tiles). This is the disc source of a town NPC's
ambient wander pace; the engine decode is
`man_field_scripts::placement_wander_step` →
`World::field_npc_glide_speeds` (default variant first, then the gated
variants in table order).

### The ambient VM's own facing ops

`FUN_80038158` writes `+0x26` from the same `0x80073F04` compass, under the
same two laws:

- **Snap.** The directional steps `0x03`/`0x19`/`0x20` and the wander/pad-echo
  ops set the heading from their operand's LUT index as they move.
- **Ramp.** Op `0x04` `[04, b1, b2]` (body at `0x800385D0`) rotates to
  `LUT[b1 & 7]` over `b2 & 0x7F` frames, stepping `arc / frames_remaining`
  and snapping on the terminal frame - the `FUN_8003774C` ramp with a
  **unit-per-tick cursor** (`+0x8B`) instead of the `_DAT_1F800393` scalar,
  and with no shortest-arc choice: `b1 & 0x80` alone picks the direction. Op
  `0x0D` `[0D, b1, b2, b3]` (body at `0x800386A4`) pre-unwraps the current
  heading past the `0x1000` boundary in the chosen direction and then hands
  `&actor+0x26` to the generic 16-bit tween channel for `b2 | b3 << 8`
  frames, so the turn is a plain linear interpolation that crosses the wrap
  correctly.

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

- [`ghidra/scripts/funcs/8003774c.txt`](../../ghidra/scripts/funcs/8003774c.txt) - full disassembly + decompilation.
- [`ghidra/scripts/funcs/80038158.txt`](../../ghidra/scripts/funcs/80038158.txt) - the second VM's interpreter.
- [`ghidra/scripts/funcs/8003a9d4.txt`](../../ghidra/scripts/funcs/8003a9d4.txt) - motion-script installer (record chain + actor binding).

## See also

**Reference** -
[Move-table VM](move-vm.md) ·
[Actor VM](actor-vm.md) ·
[World-map controller](world-map.md)
