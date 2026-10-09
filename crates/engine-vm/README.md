# legaia-engine-vm

From-scratch Rust ports of Legaia's runtime VMs, one module each. Every one is
written from the routine's disassembly (the decompiled C in
`ghidra/scripts/funcs/<addr>.txt` is a reading aid, not the evidence - see
[`ghidra.md`](../../docs/tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims))
plus the format notes in `docs/subsystems/`, with no static-recompiled bytes
from the original executable. The sections below cover the VMs proper; the rest of
the crate is the SCUS and overlay leaf kernels those VMs sit among; the
battle-side VMs are re-exported from `legaia-engine-battle-vm`.

The side-effect-free field-VM disassembler this crate re-exports from
`legaia-asset` (`field_disasm`) has its CLI on the `asset` binary:
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
- [`world_map` - `FUN_801DA51C`](#world_map---fun_801da51c)
- [`escape_timer` - `FUN_801D2EBC`](#escape_timer---fun_801d2ebc)
- [`actor_tick` - `FUN_80021DF4`](#actor_tick---fun_80021df4)
- [`status_effects`](#status_effects)
- [`scus_core_helpers`](#scus_core_helpers)
- [Battle-overlay leaves outside the action SM](#battle-overlay-leaves-outside-the-action-sm)
- [`field_subsystem_enter` - `FUN_801F1278`](#field_subsystem_enter---fun_801f1278)
- [Other modules](#other-modules)
- [Battle VM kernels](#battle-vm-kernels)
- [See also](#see-also)

## The window-widget VM - `FUN_801D6628`

The first script VM identified in retail Legaia, and the crate's namesake -
it lives at the **crate root** (`run` + the `Host` trait), not in a submodule.
Historically called the "actor / sprite VM"; it is really the **menu
overlay's window-widget script interpreter** (PROT 0899, slot-A base
`0x801CE818`, dispatch jump table at `0x801CED70`), and `operand_b` is a
window id into the menu window-descriptor table (`legaia_asset::menu_windows`)
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
exposed via `FieldHost::system_flag_{set,clear,test}`. Distinct from
the window-widget VM above.

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
pure draw kernel and lives in `engine-ui`.

## `move_vm` - `FUN_80023070`

71-opcode move-table VM (jump table at `0x80010778`); `actor_tick` and
`decrement_wait_timer` mirror the `FUN_80021DF4` gate (site
`0x80022B94..0x80022BBC` inside that function's body)
(skip when wait_timer ≥ 0, run VM, check HALT flag). Op `0x2F` escapes
into the overlay-resident `FUN_801D362C` extension VM (61 sub-opcodes);
the dispatch table is ported in `move_vm_overlay_ext.rs`.

## `motion_vm` - `FUN_8003774C` / `FUN_80038158`

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
slowly looks around. `motion_pause` is the sibling kick (`FUN_8003C9AC`) a
field interaction and a partition-2 record spawn both end with: it rewrites
every moving-class actor's requested move to its standing move. It is a clip
request, not a halt - the motion VM keeps running, and a walker its ops send
on to another step asks for its walk anim again before the request plays.

## `world_map` - `FUN_801DA51C`

Per-entity overworld state machine (5 states on `entity[+0x8A]`:
Idle → Activating → Transitioning → Terminal). `step` drains the shared
encounter countdown in the Idle state, fires `on_encounter` /
`on_interact` / `on_scene_transition` host callbacks, and advances the
scene-transition states. `legaia_engine_core::World` drives one
`WorldMapEntityCtx` per installed overworld entity each
`SceneMode::WorldMap` tick, bridging `on_encounter` into a real
Field-machinery battle (returning to the world map on resolution) and
`on_interact` into a `FieldInteract` event.

## `escape_timer` - `FUN_801D2EBC`

The scripted countdown the field VM arms with `0x4C 0xD3`
(`SCHEDULE_TIMED_FLAGS`) - retail's collapsing-dungeon escape clock. One
retail function does three things per frame and all three live here:
`EscapeTimer::tick` subtracts the play-clock delta from the counter and
reports the below-threshold and expiry story flags the crossing fires (the
expiry also disarms), `hud_fields` decomposes what is left into MM:SS.ff, and
`timer_ink` picks the readout colour. A "busy" frame - retail's three
short-circuit conditions - leaves the counter standing.

`legaia_engine_core::World` joins the installer and the drain: the field VM's
operand triple reaches `World::schedule_timed_flags` through
`FieldHost::op4c_n_d_sub3_party_setup`, and `World::tick_escape_timer` runs
the drain once per retail frame, raising each fired flag in the system-flag
bank and publishing the readout.

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

## `status_effects`

Per-actor status-effect tracker. `StatusKind` covers the retail
condition kinds, named with the game's in-game ailment terms (Toxic /
Numb / Venom / Rot / Curse / Stone / Faint, plus host-driven Sleep /
Confuse). The tracker maintains per-instance turn counters, drains
queued `StatusEvent`s into the engine's HUD pipeline, and bridges from
art-record `EnemyEffect` bytes through `StatusKind::from_enemy_effect` -
the byte map follows the pinned appliers (3 = Venom, 4 = Toxic, 5 = Rot,
6 = Curse). Rot carries a per-instance rolled limb (`set_rot_limb` /
`rot_limb`) whose attack command the battle session refuses.
Damage-over-time formulas (Toxic = `max_hp / 16`, Venom = `current_hp /
8`) live alongside.

## `scus_core_helpers`

Five leaf helpers in `SCUS_942.54`. `ActorNodePool` is the per-scene
actor node pool: a LIFO free-stack over 143 fixed-stride nodes
(`FUN_800203EC` init, `FUN_80020424` pop-as-list-head, `FUN_80020454`
pop-and-append, `FUN_800204A4` unlink-and-free), with the retail link
words `next` / `prev` / `owner` / `tail` at node offsets `+0x00` /
`+0x04` / `+0x08` / `+0x0C`. Allocation descends from the highest node
index and a freed node returns to the top of the stack, so the pool
reproduces retail's actor ordering. `list_append_u16` is the sprite
path's pre-increment u16 append (`FUN_8001FA68`), which indexes at the
*new* count and ignores the capacity its caller passes in `a2`.

Neither is called, and neither is owed a call: both carry a `REPLACED-BY:`
tag. The pool is replaced by the engine's `Vec`-backed actor pool with
generational slots; the append is the same routine ported a second time as
`legaia_engine_core::cutscene::sprite_stack_push`, which is live on both
hosts. The module's `copy_blocks_32` is likewise replaced, by the in-place
chunk walk in `legaia_asset::parse_streaming_with`.

## Battle-overlay leaves outside the action SM

Two more `0898` bodies whose kernels are ported here (the rest of the
battle-overlay leaves are in
[`legaia-engine-battle-vm`](../engine-battle-vm/README.md#battle-overlay-leaves-outside-the-action-sm)).
Both are reached from `engine-core` or a host - `battle_burst` through
`World::flush_battle_bursts`, which runs `run_burst` and seats each child on
the arm's actor - and
`battle_party_panel`'s label-actor open is `REPLACED-BY:` the immediate-mode
battle HUD:

| Module | Retail | What is ported |
|---|---|---|
| `battle_party_panel` | `FUN_801DBB8C`, `FUN_801DBC30`, `FUN_801DBD04`, `FUN_801DBDDC`, `FUN_801DBEC4`, `FUN_801D84C0` | The label-actor open (`FUN_801DBB8C`, handle at `0x801F4E0C`), the cross-out mark blit (`FUN_801DBC30`), the command ring's Rot stamp / Curse plate and the arts-entry Rot stamp (`RingMarks`), the per-party-size anchors, and `FUN_801D84C0`'s battle-result message buffers (victory with spoils, defeat, escaped, escape failed) - not party-name panels. |
| `battle_burst` | `FUN_801F30C4` | The two-mode radial effect burst: four compass iterations x three spawn blocks, the per-block placement / spread / tail arithmetic, and both parameter sets. |

Both are ported from a disassembly of the mapped `0898` image rather
than from the dump corpus, because four of those five VAs carry an
`overlay_0897` dump that disagrees with the battle-action image about the body's
own length (or, for `FUN_801DBB8C`, is a four-instruction label slice and not a
function). Reproduce with `scripts/ghidra-analysis/disasm-overlay-fn.py` at base
`0x801CE818`.

## `field_subsystem_enter` - `FUN_801F1278`

The installer of the field subsystem actor that op `0x49` and the menu button
both spawn: input suspend, the context-flag and pad-latch writes, the roster
resolve and three-cell roster seed, and the handler install. Its default handler
id `7` is the state pick `FUN_801F1F4C` (`field_state_pick`), which hands on to
the pause-menu session - it is not a party picker. A one-member party lands in
the middle roster cell and a two-member party takes the outer two.

## Other modules

The crate's remaining modules are leaf kernels; by family:

- **Animation + actors** - `anim_vm` (the per-actor animation runtime:
  `FUN_80024CFC` seat, `FUN_8004998C`'s `BoneFrame` unpack), `actor_alloc`
  (the allocator host traits), `move_buffer` (`FUN_800204F8` /
  `FUN_80020740`), `camera_rel_actor` (`FUN_80021248`'s parameter block),
  `menu_actor_seed`, `gte_divide` (the GTE's UNR reciprocal).
- **Menus + title** - `menu` (the engine's pause / shop / inn menu state
  machine, not a port of a single retail routine), `dev_equip_commit` (the
  per-slot equip commit), `title_overlay` (the title tick `FUN_801DD35C`) and
  `title_prim`, `gameover_banner` (`FUN_801CE844`).
- **Battle rules leaves** - `scus_battle_helpers`, `seru_side_effect`,
  `battle_party_panel`, `battle_burst`.
- **Move VM extension + render** - `move_vm_overlay_ext` / `move_ext_strip`
  (the op-`0x2F` extension VM `FUN_801D362C` and its sub-op `0x2C` scanline
  strip `FUN_801D31B0`), `prim_dispatch` / `vdf_morph` (the per-prim
  renderer dispatch `FUN_80043390`, VDF vertex-morph staging
  `FUN_8001C604`), `vram_rect_copy` (GP0 `0x80`, field-VM sub-op
  `0x43`/`0x12`).
- **Field overlay leaves** - `field_helpers` (the field dispatcher's
  helpers), `field_light` (the GTE light the light-source TMD rows shade
  through), `field_state_pick`, `field_actor_billboard`,
  `field_actor_reflect`, `field_actor_timers`, `field_ledge_hop_arc`,
  `field_passive_hud`, `field_player_clip`, `field_warp_tile`,
  `code_lock_actor`, `baka_hub_actors` (the op-`0x49` submode system-actor
  family), `panel_backread_loader` (PROT 0978's staged loader),
  `cutscene_trigger` (every retail FMV trigger site), `dance_marker`.
- **World map** - `world_map_overlay` and `world_map_panel` /
  `world_map_panel_actors` / `world_map_dev_menu` / `world_map_clut_fade` /
  `world_map_dim` / `world_map_horizon` / `world_map_sky` (the overworld sky
  band, `FUN_801F73E4`), plus `travel_art_actor` (Riremito and Rula).

## Battle VM kernels

The battle action state machine (`battle_action`), the battle formulas,
the battle camera script, `psx_camera`, the cast-module ticks and most
battle-overlay leaves live in
[`legaia-engine-battle-vm`](../engine-battle-vm/README.md), a crate below
this one. Every module there is re-exported here at its old path
(`legaia_engine_vm::battle_action`, ...), so callers may name either.

## See also

- [`docs/subsystems/script-vm.md`](../../docs/subsystems/script-vm.md)
- [`docs/subsystems/actor-vm.md`](../../docs/subsystems/actor-vm.md)
- [`docs/subsystems/effect-vm.md`](../../docs/subsystems/effect-vm.md)
- [`docs/subsystems/move-vm.md`](../../docs/subsystems/move-vm.md)
