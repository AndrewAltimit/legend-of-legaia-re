# Actor / sprite VM

A small fixed-width VM driving the title screen's animated sprite cluster. Distinct from the much larger [field/event VM](script-vm.md). Lives in the title-screen overlay at `FUN_801D6628`; 13-opcode dispatch table at `0x801CED70`.

## Overview

The VM walks an actor list of fixed-size structs; each actor has a small amount of per-instance state and a bytecode cursor that advances over time. Opcodes are 1 byte (no operand-byte prefix), and the operand structure is per-opcode — typically zero or one byte.

## Opcodes

The 13 opcodes cover the basics every sprite-animation system needs:

- Spawn / despawn actors.
- Set / clear a per-actor flag bit (mirrors the lower script-VM banks).
- Position writes (immediate and packed).
- Motion: linear interpolation between two endpoints.
- Trigger an animation (an ANM container indexed by id).
- Wait / yield.
- Conditional skip on a flag.
- Terminator.

Full opcode table + Rust port: `crates/engine-vm/src/lib.rs`.

## Why it's separate from the field VM

The actor VM is a fixed-width 13-opcode dispatcher tailored to the title screen's sprite-walk loop. The field VM (`FUN_801DE840`) is a 43-opcode variable-length dispatcher with cross-context targeting, halt-acquire semantics, sub-dispatcher families, and far richer ctx state. They serve different layers of the engine — actors at the rendering primitive level, scripts at the gameplay-event level — and were almost certainly written by different people on the dev team.

## Connection to ANM

Opcode "trigger animation" hands off an ANM container ID to the animation runner. The container is parsed by `crates/anm`; per-record playback is driven by the per-actor anim tick described below.

## Per-actor anim tick — `FUN_80021DF4`

The per-frame anim driver lives in `SCUS_942.54`, not in an overlay. `FUN_80021DF4` is the static-binary tick the field/battle scenes call once per frame for every active actor.

### Actor record fields

The tick reads three fixed offsets on the per-actor record:

| Offset | Type | Field | Notes |
|---|---|---|---|
| `+0x4C` | `u32` | `record_ptr` | Per-record byte pointer; written by `FUN_80024CFC` when a new animation is registered. |
| `+0x5A` | `u16` | `dispatch_byte` | Selects the per-opcode handler block (`0x01..=0x07`). |
| `+0x68` | `u16` | `frame_counter` | Initialised to `100` by `FUN_80024CFC`; advanced each tick by `actor[+0x6A]` (per-actor frame delta). |

The `crates/engine-vm` constants `ACTOR_RECORD_PTR_OFFSET`, `ACTOR_DISPATCH_BYTE_OFFSET`, and `ACTOR_FRAME_COUNTER_OFFSET` mirror those addresses.

### Dispatch byte values

`FUN_80021DF4` ladders through the dispatch byte (`actor[+0x5A]`) and routes to per-opcode handler blocks. Reading the comparison ladder at `0x80021E78..0x80022F04`:

| Byte | Mnemonic | Handler block | Notes |
|---|---|---|---|
| `0x01` | `Snap` | (TBD) | Pose-snap variant. |
| `0x02` | `KeyframeAlt` | shares with `0x06` at `0x80021E90..` | Per-bone keyframe-style. |
| `0x03` | `Path` | `0x800226DC..` | State-write logic shared with `0x05`. |
| `0x04` | `Damp` | `0x80022CBC..0x80022EE4` | Damping / spring-decay variant. |
| `0x05` | `PathAlt` | `0x800228B0..0x80022B80` | Reads geometry from `actor[+0x80]` and writes pose state. |
| `0x06` | `Keyframe` | `0x80021EA0..0x80021FA4` and `0x80022F00..0x80023040` | The dominant path. Per-bone keyframe interpolation; **fully ported in [`legaia_anm::AnimPlayer`]**. |
| `0x07` | `Spline` | `0x80022C24..0x80022CC0` | Spline / curve-driven variant. |

`crates/engine-vm`'s `DispatchByte` enum exposes those values as a typed dispatch and reports `handled_natively()` for the cases the runtime can drive without further reverse-engineering work (currently only `Keyframe`).

### Mednafen-state diff signature

Diffing the actor pool (`0x801C9594..0x801C9F7F`, 0x60-byte stride per anim slot) between a battle-intro idle save and an active-art-strike save shows the dispatch byte and the per-record pointer flipping in lockstep — the dispatch byte's lane (record `+0x0F`/`+0x10`) carries values like `0x04` (idle) and `0x06`/`0x06` (playing) across the same slot. The per-record pointer (`+0x00` of each anim slot, mirroring `actor[+0x4C]`) similarly flips between a self-reference (idle / sentinel pose) and a real RAM address that points into the scene-loaded ANM payload.
