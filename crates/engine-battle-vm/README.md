# legaia-engine-battle-vm

The battle-side VM kernels: the per-actor battle action state machine
(`FUN_801E295C`), the battle formulas, the phase-scripted battle camera and
the shared retail GTE camera, the cast-module tick bodies, and the
battle-overlay leaves around them. Ported from the routines' disassembly like
the rest of [`legaia-engine-vm`](../engine-vm/README.md), with no bytes from
the original executable.

This crate sits strictly **below** `legaia-engine-vm`: every module's whole
dependency closure inside the engine is in this crate (plus `legaia-asset` /
`legaia-art`), and `legaia-engine-vm` re-exports each module at its old path,
so `legaia_engine_vm::battle_action` and `legaia_engine_battle_vm::battle_action`
name the same module. Free of wgpu, winit and cpal, so it builds for native
and `wasm32` alike.

## What belongs here

A battle-side module moves here when nothing in its closure reaches back into
`legaia-engine-vm`. `battle_burst` (named by `move_vm`'s host) and
`battle_party_panel` (whose test reads the field subsystem cursor) stay in
`legaia-engine-vm` for that reason; `psx_camera` and `camera_mover` are here
because the battle camera script and the GTE camera build call each other.
Doc links that point back up at `legaia-engine-vm` modules are plain code
spans, since rustdoc cannot resolve a link into a dependent crate.

## Contents

- [`battle_action` - `FUN_801E295C`](#battle_action---fun_801e295c)
- [`battle_cam_script` - `FUN_801D5854`](#battle_cam_script---fun_801d5854)
- [`psx_camera` - the shared retail GTE camera](#psx_camera---the-shared-retail-gte-camera)
- [`battle_actor_tick` / `battle_actor_tint` - `FUN_800480D8` / `FUN_8004A908`](#battle_actor_tick--battle_actor_tint---fun_800480d8--fun_8004a908)
- [Battle-overlay leaves outside the action SM](#battle-overlay-leaves-outside-the-action-sm)
- [`battle_formulas`](#battle_formulas)
- [Other modules](#other-modules)

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
from `0x801F6524` / `0x801F65E8`), plus `preseed_action_queue` /
`save_action_queue` (called from `engine-core`'s auto-command path) and
`check_and_learn_art` (called from `engine-core::tactical_arts`). (`learned_seru_position`, the
`FUN_801E91E8` port beside them, is not a queue routine: it is the
already-learned check of the killing-blow Seru absorb, and it is live.)
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

## `psx_camera` - the shared retail GTE camera

The projection every camera in the port runs through, held once below both
hosts. `psx_camera_vp` is retail's
`screen = H * (R * (v - focus) + tr_eye) / Ez` with `R = Rx * Ry * Rz`
(the order `FUN_80026988` composes the camera's Euler angles in) and the GTE control file's `(OFX, OFY)`,
written as one column-major 4x4; `psx_camera_eye` is its analytic inverse, the
world-space lens. `battle_vp` above is this kernel with the battle pose's
constants, so the field camera and the battle camera cannot diverge in their
arithmetic.

The frame convention is the part worth reading before using it: **every matrix
this module returns is for the Y-up render frame** - the caller's model
matrices carry the PSX `scale(1,-1,1)` and the projection's trailing flip
cancels it. A host that instead keeps raw retail Y-down world state (the
native window's field frame) post-multiplies one more `scale(1,-1,1)` and
lands on the same net transform.

`CutsceneCameraInterp` - the between-beat glide for op-`0x45` Camera Configure
(`PORT: FUN_801DC0BC`, the `f32` rendition of `camera_mover`'s integer law) -
lives here rather than in the wgpu-linked renderer crate, which is what lets
the browser play page ease a scripted shot instead of snapping every
`apply > 0` beat. `engine-render::window` re-exports it at its old path.

Which camera owns a given frame, and what its inputs are, is one layer up:
`engine-core::camera_view`.

## `battle_actor_tick` / `battle_actor_tint` - `FUN_800480D8` / `FUN_8004A908`

The per-body battle draw decision: the draw tick (whether the body draws, the
lone-monster grey stamp, the order of its tint / trail / draw calls) and the
whole tint pass it runs first (the colour word and blend weight the draw hands
the GTE depth cue - lanes, the distance fade past half the body radius, the
outdoor-stage invert, status colours, the cursor-dim arm). The view depth the
tint reads is `battle_cam_script::battle_view_depth`. Both play hosts reach the
pair per body per frame through `engine-core`'s
`World::battle_actor_draw_plan`; the decode and its capture match are in
[`battle.md`](../../docs/subsystems/battle.md#the-distance-fade).

## Battle-overlay leaves outside the action SM

More `0898` bodies whose kernels are ported here, each reached from
`engine-core` or a host:

| Module | Retail | What is ported |
|---|---|---|
| `battle_ground_grid` | `FUN_801D02C0` | The procedural battle floor's CPU side: grid origin, the three-valued per-cell depth class, the `3x3` projection lattice, the four-corner screen reject and the 2x2 sub-tile UVs. |
| `battle_arts_auto_combo` | `FUN_801F0450` | The AI-side Arts assembler's two arms - the learned-arts auto-fill and the weighted candidate pool with its AP-gauge spend loop. |
| `battle_attack_camera` | `FUN_801D71B8` | The per-art attack camera: gate, pose seed, character / art dispatch and animation-frame push. Dispatch is three per-character jump tables (17 / 20 / 17 slots) reaching 13 distinct arms; the row folds come from `legaia_asset::battle_attack_camera_table`. |
| `battle_value_readout` | `FUN_801E805C` | The battle value readout: the landed-hit numeral's sheet, cells and pop/rise envelope, plus the multi-cast half's decimal split, teardown pairing, slot-to-widget indirection and label quad. |
| `battle_approach` | `FUN_801DF570` | The attack-approach distance clamp: the projected attacker/target separation and the `[3d/4, d]` band a requested step is clamped into. |

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

## Other modules

The crate's remaining modules are leaf kernels; by family:

- **Cameras** - `camera_mover` (the op-`0x45` cutscene glide,
  `FUN_801DC0BC`), `battle_camera` (tween step table and shake jitter).
- **Battle transition** - `battle_intro_transition` (+ `battle_intro_styles`
  / `battle_intro_tiles` / `battle_intro_swirl` / `battle_intro_particles`):
  the field-to-battle overlay's state machine and per-style kernels;
  `engine-ui::battle_intro` is its draw half.
- **Battle presentation** - `battle_actor_draw` (`FUN_80048A08`'s per-object decisions),
  `battle_pose_blend`, `battle_anim_rate` (arts slow-motion),
  `battle_impact_fx`, `battle_trail` (the weapon trail's schedule),
  `battle_hp_bar` / `battle_gauge` (bar ramp, gauge colour),
  `battle_gauge_rearm`, `battle_commit_log`, `battle_cursor_pose`,
  `battle_record_writer`, `battle_cue_group`, `move_no_effect_guard` (the
  "No effect." banner).
- **Casts** - `cast_module_ticks` / `cast_arm_ticks` / `cast_seru_ticks_a` /
  `cast_seru_ticks_b` (the slot-B cast-module tick bodies, PROT 0903..0966),
  `cast_module_camera` (the summon modules' own camera and countdown),
  `cast_fatal_decision` (PROT 0954), `battle_cast_census`,
  `battle_cast_cue`, `battle_cast_dispatch`; see
  [`cast-module.md`](../../docs/subsystems/cast-module.md).
- **Battle rules leaves** - `battle_helpers`,
  `battle_damage_wrappers` (`FUN_801DD4B0` / `FUN_801DD6B4`),
  `battle_target_group` (`FUN_801DCEAC`), `battle_separation`,
  `battle_stream_slot` (the `summon.dat` / `readef.DAT`
  streaming transfer).
