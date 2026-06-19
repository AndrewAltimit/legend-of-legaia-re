# Per-actor motion VM

Per-actor pursue / patrol / face-target VM at `FUN_8003774C` (`SCUS_942.54`). Distinct from:

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

## Provenance

- [`ghidra/scripts/funcs/8003774c.txt`](../../ghidra/scripts/funcs/8003774c.txt) - full disassembly + decompilation.

## See also

**Reference** -
[Move-table VM](move-vm.md) ·
[Actor VM](actor-vm.md) ·
[World-map controller](world-map.md)
