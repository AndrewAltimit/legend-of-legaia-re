# legaia-engine-vm

From-scratch Rust ports of Legaia's runtime VMs, one module each. Every one is
written from the routine's disassembly (the decompiled C in
`ghidra/scripts/funcs/<addr>.txt` is a reading aid, not the evidence - see
[`ghidra.md`](../../docs/tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims))
plus the format notes in `docs/subsystems/`, with no static-recompiled bytes
from the original executable. The crate is the VMs and what they import: the
window-widget VM at the root, `field`, `effect_vm`, `move_vm` with its buffers
and extension, `anim_vm`, the actor allocator and tick, and the `World`'s frame clock
(`frame_clock`). The battle-side VMs
are `legaia-engine-battle-vm` and the two motion VMs are
`legaia-engine-motion-vm`. Kernels that are not a VM live in the crate whose
role they fit - field-overlay kernels in `legaia-engine-field`, the title and
menu state machines in `legaia-engine-menus`, draw-prep kernels in
`legaia-render-kernels`, pad input and the BIOS `rand` in
`legaia-engine-system`.

The side-effect-free field-VM disassembler
(`legaia_scene_formats::field_disasm`) has its CLI on the `asset` binary:
`asset field-disasm file` walks a raw script body,
`scene-event-scripts` walks every record of a prescript container, and
`scan-prot` sweeps a whole extracted `PROT.DAT` for FMV triggers. For a
scene's genuine per-scene scripts - which live LZS-compressed inside the
scene's MAN - use `legaia-engine man-scripts` instead.

## Contents

- [The window-widget VM - `FUN_801D6628`](#the-window-widget-vm---fun_801d6628)
- [`field` - `FUN_801DE840`](#field---fun_801de840-the-fieldevent-script-vm)
- [`effect_vm` - `FUN_801DE914` / `FUN_801DFDF0` / `FUN_801E0080`](#effect_vm---fun_801de914--fun_801dfdf0--fun_801e0080)
- [`move_vm` - `FUN_80023070`](#move_vm---fun_80023070)
- [`motion_vm` - `FUN_8003774C` / `FUN_80038158`](#motion_vm---fun_8003774c--fun_80038158)
- [`actor_tick` - `FUN_80021DF4`](#actor_tick---fun_80021df4)
- [`actor_node_pool`](#actor_node_pool)
- [Battle-overlay leaves outside the action SM](#battle-overlay-leaves-outside-the-action-sm)
- [Other modules](#other-modules)
- [Battle VM kernels](#battle-vm-kernels)
- [See also](#see-also)

## The window-widget VM - `FUN_801D6628`

The first script VM identified in retail Legaia, and the crate's namesake -
it lives at the **crate root** (`run` + the `Host` trait), not in a submodule.
Historically called the "actor / sprite VM"; it is really the **menu
overlay's window-widget script interpreter** (PROT 0899, slot-A base
`0x801CE818`, dispatch jump table at `0x801CED70`), and `operand_b` is a
window id into the menu window-descriptor table (`legaia_game_tables::menu_windows`)
rather than a field actor. Small (612 bytes, 13 opcodes) and well-bounded -
the smallest target we have for a runtime-faithful port. Its programs are data
resident in the menu overlay itself; see
[`docs/formats/window-script.md`](../../docs/formats/window-script.md).

### Bytecode layout (4 bytes per instruction)

```text
byte 0:    opcode
byte 1:    operand_b - a window id
bytes 2-3: operand_w - little-endian u16, typically packed (x, y)
```

Execution stops on opcode `0x00`. Opcodes outside `1..=0xD` are no-ops.

### Opcodes

| op | name | semantics |
|----|------|-----------|
| `0x00` | `End` | Terminate the program. |
| `0x01` | `SpawnDefault` | Ensure actor exists, snap to default position, conditional clear of `field20`. |
| `0x02` | `SpawnAt` | Ensure actor exists, snap to packed `operand_w`. |
| `0x03` | `SetField1d` | Write low byte of `operand_w` to actor `field1d`. |
| `0x04` | `DeleteSprite` | Delete the sprite for `operand_b`. |
| `0x05` | `GlobalUpdate` | Tick the global sprite system. |
| `0x06` | `ClearField20` | Clear actor `field20` if actor exists. |
| `0x07` | `Nop` | Falls through to default. |
| `0x08` | `Effect` | Trigger actor effect. |
| `0x09` | `MotionAt` | Motion to packed `operand_w`. |
| `0x0A` | `EffectMotion` | Capture target, trigger effect, respawn, motion. |
| `0x0B`–`0x0D` | reserved `Nop` | Fall through to default. |

### Packed-position encoding

```text
x = (operand_w >> 7) & 0x1FE
y =  operand_w       & 0xFF
```

## `field` - `FUN_801DE840` (the field/event script VM)

Per-scene event script VM (traced from `FUN_801DE840`). Switch dispatch at
`0x801E00F4`; ~17.5 KB, the largest function in the corpus. All 43
opcodes ported. Default-route opcodes (`0x5x` / `0x6x` / `0x7x`) are
SET / CLEAR / TEST against a 256-bit bitfield at `DAT_80085758` and
exposed via `FieldFlagsHost::system_flag_{set,clear,test}`. Distinct from
the window-widget VM above.

The VM is generic over `FieldHost`, the sum of fourteen per-owner supertraits
(`FieldFlagsHost`, `FieldSceneHost`, `FieldCameraHost`, ...), one file each
under `src/field/host/`. The table of what each one speaks for is in
[`script-vm.md`](../../docs/subsystems/script-vm.md#host-surface).

## `effect_vm` - `FUN_801DE914` / `FUN_801DFDF0` / `FUN_801E0080`

Effect VM with a 32-master + 128-child slot pool. Both retail entries sit two
words ahead of their prologues (`0x801DFDF8`, `0x801E0088`), where the
pool-ready byte `0x8007BD58` is loaded; the walker is called once per battle
frame from the draw tick `FUN_800480D8`.
`Pool::init_head` / `Pool::spawn` / `Pool::tick_retail` are the three API entries
(`Pool::child_billboards` is the pass-2 render snapshot); the lifecycle is
pure data (the catalog's spawn records + animation frames), so `EffectHost`
only supplies the RNG and the summon routing.

The world-space billboard step that pairs with it (`effect_billboard`) is a
pure draw kernel and lives in `legaia-render-kernels`.

## `move_vm` - `FUN_80023070`

71-opcode move-table VM (jump table at `0x80010778`); `actor_tick` and
`decrement_wait_timer` mirror the `FUN_80021DF4` gate (site
`0x80022B94..0x80022BBC` inside that function's body)
(skip when wait_timer ≥ 0, run VM, check HALT flag). Op `0x2F` escapes
into the overlay-resident `FUN_801D362C` extension VM (61 sub-opcodes);
the dispatch table is ported in `move_vm_overlay_ext.rs`.

## `motion_vm` - `FUN_8003774C` / `FUN_80038158`

Retail carries **two** per-actor motion VMs and both are ported: `motion_vm`
(`FUN_8003774C`, pursue / patrol / face-target) and `ambient_motion`
(`FUN_80038158`, the scripted-motion VM over MAN tail-section 1). They live in
[`legaia-engine-motion-vm`](../engine-motion-vm/README.md)
(`legaia_engine_motion_vm::motion_vm`, `legaia_engine_motion_vm::ambient_motion`,
`legaia_engine_motion_vm::ambient_motion_ops`). This crate does not depend on
it, and the oracles that pin both VMs are that crate's own `tests/`.

## `actor_tick` - `FUN_80021DF4`

Per-actor physics tick - the `FUN_8002519C`-driven per-frame loop calls
this on every active actor. The dispatch byte at `actor[+0x5A]` selects
which subset of side-effects fires:

| Stage | Runs for | Behaviour |
|---|---|---|
| Common pre-update | every byte | Drain timer at `+0x54`, advance rotation accumulator at `+0x22`. |
| Keyframe accel | `0x02` / `0x06` | Fold `+0xC0..+0xCA` into shake envelopes at `+0xB4..+0xC8`. |
| Positional SFX emitter | `0x05` | Distance-based pan / volume engine; ramp interpolation between target / source pairs over `+0xBC` frames; `key-on` / `vol-update` / `release` SsAPI calls surface as `TickEvent::Sfx*`. |
| Path interpolation | `0x03` | Three-axis velocity into `+0x90..+0x94`, zoom envelope advance, path state machine at `+0x9C`. |
| Default movement | every byte except `0x05` | Velocity / accel into `motion_x..motion_z`, trig-LUT-driven world rotation, shake / focal envelopes. |
| Common late-update | every byte | Cap envelopes, optional move-VM kick, render submissions for `0x04` / `0x07`, keyframe pose write for `0x06`. |

`ActorPhysics` mirrors the retail actor record's tick-relevant fields
(`+0x10` through `+0xD0`, with offset annotations on every field).
Cross-cutting effects surface as `TickEvent` entries; engines drain
them into their own audio mixer / scene graph / move-VM driver.

## `actor_node_pool`

`ActorNodePool` is the per-scene
actor node pool: a LIFO free-stack over 143 fixed-stride nodes
(`FUN_800203EC` init, `FUN_80020424` pop-as-list-head, `FUN_80020454`
pop-and-append, `FUN_800204A4` unlink-and-free), with the retail link
words `next` / `prev` / `owner` / `tail` at node offsets `+0x00` /
`+0x04` / `+0x08` / `+0x0C`. Allocation descends from the highest node
index and a freed node returns to the top of the stack, so the pool
reproduces retail's actor ordering.

Two unrelated `SCUS_942.54` leaves sit beside it, a module each:
`u16_list_append::list_append_u16` is the sprite path's pre-increment u16
append (`FUN_8001FA68`), which indexes at the *new* count and ignores the
capacity its caller passes in `a2`; `block_copy_32::copy_blocks_32` is the
32-byte-block copy (`FUN_8003D26C`).

None of the three is called, and none is owed a call: each carries a
`REPLACED-BY:` tag. The pool is replaced by the engine's `Vec`-backed actor
pool with generational slots; the append is the same routine ported a
second time as `legaia_engine_system::movie::cutscene::sprite_stack_push`,
which is live on both hosts; the block copy is replaced by the in-place
chunk walk in `legaia_container_formats::parse_streaming_with`.

## Battle-overlay leaves outside the action SM

One `0898` body whose kernel is ported here because the move VM's host names
it (the rest of the battle-overlay leaves are in
[`legaia-engine-battle-vm`](../engine-battle-vm/README.md#battle-overlay-leaves-outside-the-action-sm)).
`battle_burst` is reached from `engine-core` through
`World::flush_battle_bursts`, which runs `run_burst` and seats each child on
the arm's actor:

| Module | Retail | What is ported |
|---|---|---|
| `battle_burst` | `FUN_801F30C4` | The two-mode radial effect burst: four compass iterations x three spawn blocks, the per-block placement / spread / tail arithmetic, and both parameter sets. |

It is ported from a disassembly of the mapped `0898` image rather than from
the dump corpus; reproduce with `scripts/ghidra-analysis/disasm-overlay-fn.py`
at base `0x801CE818`.

## Other modules

The crate's remaining modules are what the VMs import; by family:

- **Animation + actors** - `anim_vm` (the per-actor animation runtime:
  `FUN_80024CFC` seat, `FUN_8004998C`'s `BoneFrame` unpack), `actor_alloc`
  (the allocator host traits), `move_buffer` (`FUN_800204F8` /
  `FUN_80020740`).
- **Move VM extension** - `move_vm_overlay_ext` / `move_ext_strip` (the
  op-`0x2F` extension VM `FUN_801D362C` and its sub-op `0x2C` scanline strip
  `FUN_801D31B0`) and `vdf_morph` (the move-VM actor's morph lanes and
  their ramp envelope; the record walk and weighted-delta blend they feed
  are `legaia_scene_formats::vdf_morph`).
- **Positional sound emitter** - `sfx_emitter` (the part tick
  `FUN_80021DF4`'s render-mode-5 arm: level ramp, box falloff against the
  stored camera focus, raw voice volume, key / volume / release calls).
- **Field VM leaves** - `field::dispatch_leaves` (the dispatcher's operand
  and table helpers), `field_actor_billboard`, `field_ledge_hop_arc` and
  `vram_rect_copy` (GP0 `0x80`, field-VM sub-op `0x43`/`0x12`): each is a
  body the field VM's own step calls.

## Battle VM kernels

The battle action state machine (`battle_action`), the battle formulas,
the battle camera script and most battle-overlay leaves live in
[`legaia-engine-battle-vm`](../engine-battle-vm/README.md)
(`legaia_engine_battle_vm::battle_action`, ...), the cast-module ticks one
crate above it in [`legaia-engine-cast`](../engine-cast/README.md), and
`psx_camera` in [`legaia-engine-camera`](../engine-camera/README.md). This crate does not depend
on it; neither crate sits below the other.

## See also

- [`docs/subsystems/script-vm.md`](../../docs/subsystems/script-vm.md)
- [`docs/subsystems/actor-vm.md`](../../docs/subsystems/actor-vm.md)
- [`docs/subsystems/effect-vm.md`](../../docs/subsystems/effect-vm.md)
- [`docs/subsystems/move-vm.md`](../../docs/subsystems/move-vm.md)
