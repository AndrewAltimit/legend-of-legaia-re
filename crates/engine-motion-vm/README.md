# legaia-engine-motion-vm

The two per-actor motion VMs, ported from the routines' disassembly like the
rest of [`legaia-engine-vm`](../engine-vm/README.md), with no bytes from the
original executable.

This crate sits strictly **below** `legaia-engine-vm`: every module's whole
dependency closure inside the engine is in this crate, `legaia-asset` (the MAN
motion-script opcode widths) and
[`legaia-engine-battle-vm`](../engine-battle-vm/README.md) (the PsyQ `rand`
step and the bearing LUT). `legaia-engine-vm` re-exports each module at its
old path, so `legaia_engine_vm::ambient_motion` and
`legaia_engine_motion_vm::ambient_motion` name the same module. Free of wgpu,
winit and cpal, so it builds for native and `wasm32` alike.

## `motion_vm` / `ambient_motion` - `FUN_8003774C` / `FUN_80038158`

Retail carries **two** per-actor motion VMs and both are ported.
`motion_vm` is `FUN_8003774C`: pursue / patrol / face-target, the NPC
movement, camera follow paths and "face the speaker" cinematic posing.
Each script entry is `1 + N` bytes, with bit `0x80` of the op byte selecting a
target actor first (`0xF8` = self, `0xFB` = linked); dispatch is a 22-entry
jump table at `0x80010EE0` indexed by `(op & 0x7F) - 0x37`.

`ambient_motion` is the second one, `FUN_80038158` - the scripted-motion VM
whose bytecode arrives as MAN tail-section 1 (`legaia_asset::man_motion`). It
runs the whole 32-slot table: the idle facing ramps, the walk ops, the waits,
the story-flag writes, the bit ops, the teleport, the model swap and the three
scalar tweens. The op bodies split across two files for length only - the
walks, waits, ramps and the ramp scheduler here, the rest in
`ambient_motion_ops` as further `impl AmbientMotion` blocks - and nothing is
stepped over by width. Its two rotate ops both aim at the same eight-point
compass LUT the walk ops snap to, so every ambient turn ends on a compass
point. Without it an engine NPC holds one heading forever where a retail one
slowly looks around.

`motion_pause`, the sibling kick (`FUN_8003C9AC`) that rewrites every
moving-class actor's requested move to its standing move, stays in
`legaia-engine-vm`: it writes the field VM context's requested move.

## What belongs here

| Module | Covers |
|---|---|
| `motion_vm` | `FUN_8003774C`: the opcode decoder and per-op steps, the compass and heading LUTs, the rotate ramp, and the touch post both VMs share. |
| `ambient_motion` | `FUN_80038158`: the dispatch loop, the variant cursor, the walks, waits, facing ramps and the ramp scheduler. |
| `ambient_motion_ops` | The rest of the op bodies as further `impl AmbientMotion` blocks: story-flag writes, bit ops, teleport, model swap, scalar tweens, and the effects they report to the host. |

The disc oracles that pin both VMs against every scene MAN
(`ambient_motion_disc_oracle`, `ambient_motion_ops`) stay in
`crates/engine-vm/tests`, which name the re-exported paths.

## See also

- [`docs/subsystems/motion-vm.md`](../../docs/subsystems/motion-vm.md) - the
  opcode tables and the bytecode's home in the MAN.
- [`crates/engine-vm`](../engine-vm/README.md) - the crate this was split
  from, and the field host that ticks both VMs.
