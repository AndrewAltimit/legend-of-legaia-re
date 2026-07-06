# Per-actor motion VMs

**Two** distinct per-actor bytecode VMs live under the field actor tick
`FUN_8003BC08`: the pursue / patrol / face-target VM at `FUN_8003774C`
(dispatched when actor `+0x10 & 0x400`) and the scripted-motion / flag VM at
`FUN_80038158` (dispatched when `+0x10 & 0x80`, bytecode carried in MAN
tail-section 1 - [below](#the-second-motion-vm-fun_80038158)). Both are
distinct from:

- [Actor VM](actor-vm.md) (`FUN_801D6628`) - sprite spawn / despawn, 13 ops.
- [Move VM](move-vm.md) (`FUN_80023070`) - Tactical Arts / battle-action animation, 71 + 61 ops.

The motion VM drives **per-actor pursue / patrol / face-target** logic used for NPC movement on the field, camera follow paths, and "face the speaker" cinematic posing during dialog.

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

| byte | retail addr | name             | semantics                                |
|------|-------------|------------------|------------------------------------------|
| 0x37 | 80037894    | TranslateY       | accumulate Y axis by per-frame speed     |
| 0x38 | 80037de0    | RotateToAngle    | yaw rotates toward an absolute angle (16-entry `ANGLE_TABLE`) over a frame budget; shortest-path (`body0 & 0x80`) or forced-direction (`body1 & 0x80`), 12-bit fixed-point |
| 0x41 | 80037894    | TranslateX       | accumulate X axis by per-frame speed     |
| 0x43 | 80037f5c    | NoOp             | tick budget consumed, no actor mutation  |
| 0x47 | 80037ba8    | MoveTowardTarget | step actor XZ toward `(tx, tz)`          |
| 0x4C | 80037de0    | FaceTarget       | yaw rotates to the target's bearing over a frame budget; sub-modes 0x85 / 0x8E / 0x8F gate which component is rotated (0x8F forces clockwise) |
|      |             | (default arm)    | terminate with `Done`                    |

## Per-frame speed

`DAT_1f800393` is the per-frame speed scalar (also drives the [move VM](move-vm.md) frame-time scratchpad). The motion VM consumes it as the budget for incremental motion - engines update once per frame, the VM consumes per opcode.

## Clean-room port

[`legaia_engine_vm::motion_vm`](../../crates/engine-vm/src/motion_vm.rs) is the clean-room port. All six opcodes are implemented: `0x37` `TranslateY`, `0x38` `RotateToAngle`, `0x41` `TranslateX`, `0x43` `NoOp`, `0x47` `MoveTowardTarget`, `0x4C` `FaceTarget`. Each step returns `StepResult::Yield` (budget consumed, resume next tick) or `StepResult::Done` (terminal op / default arm); there is no fallback path.

## Engine consumers

The runtime [`Camera`](../../crates/engine-core/src/camera.rs) in `engine-core` consumes:

- The field-VM op-`0x45` event stream (`CameraConfigure` / `CameraSave` / `CameraLoad` / `CameraApply`) for the high-level camera state.
- The motion VM (optional) for cinematic pre-baked camera paths via `Camera::tick_script`.

The default mode follows a target actor slot at a configured distance + height.

### Field-NPC walking

`World::tick_field_npc_motions` (`engine-core`) drives MAN-placed field NPCs through the `0x47` `MoveTowardTarget` pursue step, one motion-VM step per field tick, writing the live position back into `World::field_npc_positions` so the moving NPC's ±40-unit collision box and its interact box follow it (retail probes the live `+0x14`/`+0x18`, not the spawn anchor). Three start paths feed it:

- **Autonomous patrol routes** (`World::field_npc_routes`, gated by `World::animate_field_npcs` / `play-window --live-npcs`): each placement's own pre-text script bytecode carries `0x4C 0x51` NPC move-to-tile ops; `man_field_scripts::placement_motion_route` decodes the local waypoints (dropping the `(127,127)` park sentinel, cross-context targets, and beyond-locality story-relocation branches) and the engine loops them as a patrol. Autonomous legs pause while a dialogue is up - retail's interaction motion-pause kick (`FUN_8003c9ac` reloading every moving-class actor's pause timer on the touch event post).
- **Interaction-prologue runs**: when the opt-in field-VM dialogue runner executes an NPC's record and the prologue hits a `0x4C 0x51` with the NPC arm, the host hook (`vm_hosts::FieldHostImpl::op4c_n5_sub1_npc_run`) starts the interacted actor's walk leg. These run through the dialogue - they are the interaction's choreography.
- **Actor-VM `start_motion`** (op `0x09` `MotionAt`, retail `FUN_800358c0`): `World::start_actor_motion` records the glide target and steps the actor's sprite position toward it through the same pursue kernel (`World::tick_actor_motions`).

The start kernel (`World::start_field_npc_motion`) mirrors the `FUN_800358c0` shape - write the target, reset the glide cursor - and the per-frame consumer is this VM. Residue: the exact retail per-NPC glide speed (the `+0x72` multiplier path) is unpinned; the engine paces NPCs at the player walking step. Per-actor field-VM channels (yield-paced, story-flag-branched script execution) are not modelled - the engine drives the decoded waypoint list directly.

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
  the `_DAT_8007C354` field actor whose `+0x50` equals the MAN partition-1
  placement index.
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
| 3 | `0x02` anim/timer, `0x03`/`0x19`/`0x20` directional step, `0x04` facing ramp, `0x07` SET flag, `0x08` CLEAR flag, `0x09` post u16 to the `DAT_8007B6D8` ring (`FUN_80035B50`), `0x0A`/`0x0B` actor-flag +/-`0x1000000`, `0x0E` model swap, `0x0F` tile teleport, `0x17` pause-table pair |
| 4 | `0x0D` facing ramp + tween channel |
| 5 | `0x06` pad-echo step, `0x14`/`0x15`/`0x16` tween installs, `0x18` AABB wander |
| 8 | `0x0C` glide-channel install |
| 13 | `0x13` `FUN_80058490` call |

No op writes `DAT_8007B7FC`-class globals - op-`9` only posts to the 4-slot
`DAT_8007B6D8` ring, so the battle-id write has no motion-VM analogue.

### Flag census

`legaia-engine man-scripts --motion-flag-census` sweeps every scene MAN
(including v12-embedded MANs) and reports all op-7/op-8 sites (sibling of
`--system-flag-census`, which covers the MAN field-VM ops `0x50/0x60/0x70`).
Disc-wide the op-7/op-8 surface is overworld walking-band choreography
(`map02`/`map03`: `0x466`/`0x467` toggles, the `0x56D..0x570` self-advancing
4-phase cycle, `0x5A2..0x5A7` one-shot latches) plus one `town0b` clear of
`0x23F`. The spine gate flags `0x142`/`0x482`/`0x1BE` and the town01 opening
one-shot `549` appear in **no** motion stream - those writers are direct
code paths, not scene bytecode (see
[`world-map.md`](world-map.md#gate-flag-setters-that-are-not-man-field-vm-ops)).
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
