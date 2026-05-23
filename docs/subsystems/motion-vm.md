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
| 0x38 | 80037de0    | RotateToAngle    | yaw rotates toward absolute angle (12-bit fixed-point quadrant logic; not yet ported - treated as `Pending`) |
| 0x41 | 80037894    | TranslateX       | accumulate X axis by per-frame speed     |
| 0x43 | 80037f5c    | NoOp             | tick budget consumed, no actor mutation  |
| 0x47 | 80037ba8    | MoveTowardTarget | step actor XZ toward `(tx, tz)`          |
| 0x4C | 80037de0    | FaceTarget       | yaw rotates to bearing; sub-modes 0x85 / 0x8E / 0x8F gate which component is rotated. Not yet ported - treated as `Pending` |
|      |             | (default arm)    | terminate with `Done`                    |

## Per-frame speed

`DAT_1f800393` is the per-frame speed scalar (also drives the [move VM](move-vm.md) frame-time scratchpad). The motion VM consumes it as the budget for incremental motion - engines update once per frame, the VM consumes per opcode.

## Clean-room port

[`legaia_engine_vm::motion_vm`](../../crates/engine-vm/src/motion_vm.rs) is the clean-room port. Implemented opcodes: `0x37` `TranslateY`, `0x41` `TranslateX`, `0x43` `NoOp`, `0x47` `MoveTowardTarget`. `0x38` and `0x4C` are documented but not yet ported - the VM reports `StepResult::Pending { op }` so engines can route them to a fallback.

## Camera integration

The runtime [`Camera`](../../crates/engine-core/src/camera.rs) in `engine-core` consumes:

- The field-VM op-`0x45` event stream (`CameraConfigure` / `CameraSave` / `CameraLoad` / `CameraApply`) for the high-level camera state.
- The motion VM (optional) for cinematic pre-baked camera paths via `Camera::tick_script`.

The default mode follows a target actor slot at a configured distance + height.

## Provenance

- [`ghidra/scripts/funcs/8003774c.txt`](../../ghidra/scripts/funcs/8003774c.txt) - full disassembly + decompilation.

## See also

**Reference** —
[Move-table VM](move-vm.md) ·
[Actor VM](actor-vm.md) ·
[World-map controller](world-map.md)
