# legaia-engine-vm

Clean-room Rust ports of Legaia's runtime VMs.

Three VMs are bundled as separate modules. Each is written from the
decompiled source in `ghidra/scripts/funcs/<addr>.txt` plus the format
notes in `docs/subsystems/`, with no static-recompiled bytes from the
original executable.

## Contents

- [`actor_vm` - `FUN_801D6628`](#actor_vm---fun_801d6628)
- [`field_vm` - `FUN_801DE840`](#field_vm---fun_801de840-the-fieldevent-script-vm)
- [`effect_vm` - `FUN_801DE914` / `FUN_801DFDF8` / `FUN_801E0088`](#effect_vm---fun_801de914--fun_801dfdf8--fun_801e0088)
- [`move_vm` - `FUN_80023070`](#move_vm---fun_80023070)
- [`world_map` - `FUN_801DA51C`](#world_map---fun_801da51c)
- [`escape_timer` - `FUN_801D2EBC`](#escape_timer---fun_801d2ebc)
- [`actor_tick` - `FUN_80021DF4`](#actor_tick---fun_80021df4)
- [`status_effects`](#status_effects)
- [`scus_core_helpers`](#scus_core_helpers)
- [`battle_cam_script` - `FUN_801D5854`](#battle_cam_script---fun_801d5854)
- [Battle-overlay leaves outside the action SM](#battle-overlay-leaves-outside-the-action-sm)
- [`field_party_cursor` - `FUN_801F1278`](#field_party_cursor---fun_801f1278)
- [`battle_formulas`](#battle_formulas)
- [See also](#see-also)

## `actor_vm` - `FUN_801D6628`

Sprite / actor script VM. The first script VM identified in retail
Legaia. Lives in the title-screen / field overlay loaded into the
`0x801C0000+` window at runtime. Small (612 bytes, 13 opcodes) and
well-bounded - the smallest target we have for a runtime-faithful port.

### Bytecode layout (4 bytes per instruction)

```text
byte 0:    opcode
byte 1:    operand_b - typically an actor id
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
| `0x07`–`0x0D` | `Nop` / reserved | Fall through to default. |
| `0x08` | `Effect` | Trigger actor effect. |
| `0x09` | `MotionAt` | Motion to packed `operand_w`. |
| `0x0A` | `EffectMotion` | Capture target, trigger effect, respawn, motion. |

### Packed-position encoding

```text
x = (operand_w >> 7) & 0x1FE
y =  operand_w       & 0xFF
```

## `field_vm` - `FUN_801DE840` (the field/event script VM)

Per-scene event script VM (traced from `FUN_801DE840`). Switch dispatch at
`0x801E00F4`; ~17.5 KB, the largest function in the corpus. All 43
opcodes ported. Default-route opcodes (`0x5x` / `0x6x` / `0x7x`) are
SET / CLEAR / TEST against a 256-bit bitfield at `DAT_80085758` and
exposed via `FieldHost::system_flag_{set,clear,test}`. Distinct from
the actor VM above.

## `effect_vm` - `FUN_801DE914` / `FUN_801DFDF8` / `FUN_801E0088`

Effect VM with a 32-master + 128-child slot pool.
`Pool::init` / `Pool::spawn` / `Pool::tick_retail` are the three API entries
(`Pool::child_billboards` is the pass-2 render snapshot); the lifecycle is
pure data (the catalog's spawn records + animation frames), so `EffectHost`
only supplies the RNG and the summon routing.

The sibling module `effect_billboard` carries the one step a *world-space*
billboard builder gets wrong. Retail's quad projector `FUN_800195A8` transforms
the sprite centre through the camera matrix and only then adds the
half-extents, in view space - so the battle camera's 4x base matrix scales the
centre and must not scale the size. `world_half_extents` divides it back out;
both hosts call it, since `engine-render` links wgpu and the browser play page
cannot depend on it.

## `move_vm` - `FUN_80023070`

71-opcode move-table VM (jump table at `0x80010778`); `actor_tick` and
`decrement_wait_timer` mirror the `FUN_80021DF4` gate (site
`0x80022B94..0x80022BBC` inside that function's body)
(skip when wait_timer ≥ 0, run VM, check HALT flag). Op `0x2F` escapes
into the overlay-resident `FUN_801D362C` extension VM (61 sub-opcodes);
the dispatch table is ported in `move_vm_overlay_ext.rs`.

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

Nothing in the engine calls either yet - both carry a `NOT WIRED` note
naming the reason.

## `battle_action` - `FUN_801E295C`

The per-actor battle action state machine (see
[`docs/subsystems/battle-action.md`](../../docs/subsystems/battle-action.md)),
split across `dispatch` / `attack` / `magic` / `summon` / `spirit` / `done` /
`run` / `enemy_budget` / `validator`. `pool_ops` collects the small
self-contained leaves over the 8-slot actor pool and the ctx target queue:
`clear_pool_flag_words` (`FUN_801DB9C4`, the `+0x8 &= 0x7CFFFFFF` scrub the
pose setter `FUN_801D5854` runs on an out-of-range slot - **not** an
action-SM state), `normalize_formation_span` (`FUN_801DB318`, the formation
span-squash + centroid recentre with camera-focus compensation),
`build_attack_target_queue` (`FUN_801D8A88`, the multi-target ring *builder* -
counts live monsters, sorts the alternates by bearing offset from the current
target) and its `cycle_attack_target` (`FUN_801D8D00`, the next/prev accessor),
`bearing_12bit` (`FUN_80019B28`, the faithful arctan-LUT atan2 the builder
sorts by), `first_live_monster_slot` (`FUN_801DB8B4`),
`first_selectable_target` / `next_selectable_actor` (`FUN_801DBA04` /
`FUN_801DB81C`, the participant scans), and `redirect_dead_target`
(`FUN_801DB124`, re-roll a queued action's target to a living same-side slot
when the chosen target has died).

`queue_applier` carries the byte-level kernels of the arts queue-builder
`FUN_801EED1C`, which operate on the raw `actor[+0x1DF..+0x1F2]` window rather
than on typed action constants: `apply_miracle_replace` (the flat 16-byte
overwrite from the resident Miracle row at `0x801F64F4`), `clear_queue_msb`
(the sweep that strips the row's on-disc `0x8C..0x8F` quirk),
`apply_super_tail_replace` (`FUN_801EF9E4`, first-matching-row tail replace
from `0x801F6524` / `0x801F65E8`), plus the still-inert `preseed_action_queue`
/ `save_action_queue` / `check_and_learn_art` / `miracle_command_position`.
`resolve_action_queue` - the entry point `engine-core` calls once per committed
arts input - runs the first three in retail's finish order, so the live path is
byte-level rather than structural; `legaia_art`'s matchers remain the *table*
source behind `miracle_row_for` / `super_rows_for`.

`overlay_rng` is the battle overlay's **own** generator (`FUN_801D0290`) - twelve
instructions over the single word at `0x801F6950`, so its draws never perturb the
SCUS `rand()` stream the determinism oracles follow. Its final `addu` of the two
shifted halves is exactly `rotate_left(16)`, which the module asserts over the
halfword boundaries rather than claiming in prose.

### The host's spell contract

`BattleActionHost` asks the engine two questions about a spell, and both are
deliberately narrow so a host cannot answer them from a second model.
`spell_class_byte` returns the record's `+0` byte and *everything* class-shaped
falls out of it - the capture route (`is_capture_spell`'s default) and the
action-seed band pick (`dispatch::magic_seed_band`). `spell_mp_cost` returns
the price, and it must be the same number the host's own cast path charges: the
SM debits MP itself at `MagicCastBegin` / `SpiritPreArm`, so a host that prices
a cast differently elsewhere is charging twice or charging nothing. See
[`docs/subsystems/battle-action.md`](../../docs/subsystems/battle-action.md)
§ Magic in the port for which half of a cast each side owns.

## `battle_cam_script` - `FUN_801D5854`

The phase-scripted battle camera, held once for every host. The module owns
retail's framing cases and the phase that selects each: the arts / spell / item
**input** close-up (case `0`), the per-action framing and its two arms (case
`6`), the post-strike **two-shot** on the attacker-target midpoint (case `7`),
the end-of-action shot on the target (case `8`), and the far Begin/Run framing
sized to the live formation (case `9`). `drive` is the single entry both hosts
call, so the create / retarget / phase-change / step ordering cannot diverge.

Three things here are easy to get wrong and are pinned in
[`docs/subsystems/battle.md`](../../docs/subsystems/battle.md#battle-camera-exact):
"a battle menu is open" does **not** select the close-up (the command chooser
keeps the far framing, only the input pickers take case `0`); case `9` is
re-derived every pass, so a depth frozen while the formation was collapsed
mid-approach never re-opens; and the resting yaw is the free-running orbit a
fight *inherits* from the field camera, not a constant - five retail states at
one framing read five different yaws.

## Battle-overlay leaves outside the action SM

Five more `0898` bodies whose kernels are ported here, each with its own
`NOT WIRED` disclosure naming the caller or the disc table it still needs:

| Module | Retail | What is ported |
|---|---|---|
| `battle_ground_grid` | `FUN_801D02C0` | The procedural battle floor's CPU side: grid origin, the three-valued per-cell depth class, the `3x3` projection lattice, the four-corner screen reject and the 2x2 sub-tile UVs. |
| `battle_scatter` | `FUN_801E0080` | The arena's emitter/particle pools: record layouts, both script advances, the countdown drain, the position integration, the brightness ramp and the mirror-bit UVs. |
| `battle_arts_auto_combo` | `FUN_801F0450` | The AI-side Arts assembler's two arms - the learned-arts auto-fill and the weighted candidate pool with its AP-gauge spend loop. |
| `battle_attack_camera` | `FUN_801D71B8` | The per-art attack camera's gate, pose seed, character / art dispatch and animation-frame push; the seventeen per-art arms need the `0x801F4E10` table parsed first. |
| `battle_value_readout` | `FUN_801E805C` | The battle value readout: the landed-hit numeral's sheet, cells and pop/rise envelope, plus the multi-cast half's decimal split, teardown pairing, slot-to-widget indirection and label quad. |
| `battle_approach` | `FUN_801DF570` | The attack-approach distance clamp: the projected attacker/target separation and the `[3d/4, d]` band a requested step is clamped into. |
| `battle_party_panel` | `FUN_801DBB8C`, `FUN_801DBC30`, `FUN_801D84C0` | The battle party-name panels - the label-actor open/teardown pair over `0x801F4E08`, the per-party-size anchors, the all-slots actor reset, and the label-strip blit. |
| `battle_burst` | `FUN_801F30C4` | The two-mode radial effect burst: four compass iterations x three spawn blocks, the per-block placement / spread / tail arithmetic, and both parameter sets. |

The last two are ported from a disassembly of the mapped `0898` image rather
than from the dump corpus, because four of those five VAs carry an
`overlay_0897` dump that disagrees with the battle-action image about the body's
own length (or, for `FUN_801DBB8C`, is a four-instruction label slice and not a
function). Reproduce with `scripts/ghidra-analysis/disasm-overlay-fn.py` at base
`0x801CE818`.

## `field_party_cursor` - `FUN_801F1278`

The field VM's op-`0x49` party-member picker, enter half: the context-flag and
pad-latch writes, the roster resolve, and the three-cell portrait seed. The one
behaviour worth knowing is that the picker is **centre-weighted** - a one-member
party lands in the middle cell and a two-member party takes the outer two.

## `battle_formulas`

Damage / MP-cost / accuracy / RNG / escape arithmetic kernels.
`art_strike_damage(attack, defense, multiplier, divisor, floor)`
applies the per-strike Tactical Art damage formula; `accuracy_roll`
mirrors selector 9 of `FUN_800402F4`; `mp_cost_after_ability_bits`
mirrors the MP-half/quarter shift-subtract in `FUN_801E295C` state
`0x28` (MP-half `0x20` wins over MP-quarter `0x10`); `escape_roll`
(with `escape_party_score` / `escape_enemy_score` over
`EscapeActor` + `EscapeFlags`) mirrors the Run-command escape check
`FUN_801E791C` - party `(SPD*3)>>1 + missingHP>>4` vs enemy
`SPD + missingHP>>5`, two rand draws, Chicken Heart / Chicken King
ability bits honoured.

`monster_escape_roll` (with `monster_escape_side_scores` over `FleeActor`) is the
enemy-side mirror `FUN_801EC0DC`: "does this monster break off and flee?" It
weighs HP and **ATK** where the party roll weighs SPD, floors the monster side at
`3/2` of the party average, and refuses outright on the same `ctx[+0x287]`
no-escape byte plus a flat `rand() & 7` gate and the **No Escape** / Chicken
Guard passive (`record[+0xF8] & 0x400000`). It takes a draw *closure* rather than
a fixed array because the third draw only happens once the score compare passes.

The retail per-slot "target valid" predicate `FUN_8003fb10` (the 18-arm
menu/UI gate documented in
[`docs/subsystems/battle-action.md`](../../docs/subsystems/battle-action.md#action-validator-fun_8003fb10))
is ported whole as `battle_action::validate_action` over the
`ActionValidatorHost` trait (per-slot HP/MP quads, record stats, party
indirection, system flags, the `FUN_80046898` inventory leaf). The older
consumption-site mirrors remain where they are used - liveness/kind gating in
`legaia-engine-core`'s `target_picker`, and the item-benefit arms in
`inventory_use::effect_benefits_target`.

## See also

- [`docs/subsystems/script-vm.md`](../../docs/subsystems/script-vm.md)
- [`docs/subsystems/actor-vm.md`](../../docs/subsystems/actor-vm.md)
- [`docs/subsystems/effect-vm.md`](../../docs/subsystems/effect-vm.md)
- [`docs/subsystems/move-vm.md`](../../docs/subsystems/move-vm.md)
