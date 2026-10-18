# legaia-engine-battle-vm

The battle-side VM kernels: the per-actor battle action state machine
(`FUN_801E295C`), the battle formulas, the phase-scripted battle camera, and
the battle-overlay leaves around them. The slot-B cast / summon module tick
bodies the action SM pages in sit one crate up, in
[`legaia-engine-cast`](../engine-cast/README.md), which names this crate and
is never named by it. Ported from the routines' disassembly like
the VMs in [`legaia-engine-vm`](../engine-vm/README.md), with no bytes from
the original executable.

This crate does not depend on `legaia-engine-vm`: every module's whole
dependency closure inside the engine is in this crate (plus the format
crates, `legaia-art`, `legaia-engine-system` for the BIOS `rand` and the fade
template the summon flash converts into, and
[`legaia-engine-camera`](../engine-camera/README.md) for the shared GTE camera
the battle camera script builds its poses over). Consumers name the modules here
directly (`legaia_engine_battle_vm::battle_action`). Free of wgpu, winit and
cpal, so it builds for native and `wasm32` alike.

## What belongs here

A battle-side module moves here when nothing in its closure reaches back into
`legaia-engine-vm`. `battle_burst` (named by `move_vm`'s host) stays in
`legaia-engine-vm` for that reason. The GTE camera build, the projection
constants and `BattleCamPose` are one crate down in `legaia-engine-camera`,
because the field camera, the Baka Fighter duel and the draw kernels run on
them too.
Doc links that point at `legaia-engine-vm` modules are plain code
spans, since rustdoc cannot resolve a link into a crate this one does not
depend on.

## Contents

- [`battle_action` - `FUN_801E295C`](#battle_action---fun_801e295c)
- [`battle_cam_script` - `FUN_801D5854`](#battle_cam_script---fun_801d5854)
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
`check_and_learn_art` (called from `engine-battle::tactical_arts`). (`learned_seru_position`, the
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
| `battle_attack_camera` | `FUN_801D71B8` | The per-art attack camera: gate, pose seed, character / art dispatch and animation-frame push. Dispatch is three per-character jump tables (17 / 20 / 17 slots) reaching 13 distinct arms; the row folds come from `legaia_game_tables::battle_attack_camera_table`. |
| `battle_value_readout` | `FUN_801E805C` | The battle value readout: the landed-hit numeral's sheet, cells and pop/rise envelope, plus the multi-cast half's decimal split, teardown pairing, slot-to-widget indirection and label quad. |
| `battle_approach` | `FUN_801DF570` | The attack-approach distance clamp: the projected attacker/target separation and the `[3d/4, d]` band a requested step is clamped into. |
| `battle_party_panel` | `FUN_801DBB8C`, `FUN_801DBC30`, `FUN_801DBD04`, `FUN_801DBDDC`, `FUN_801DBEC4`, `FUN_801D84C0` | The label-actor open (`FUN_801DBB8C`, handle at `0x801F4E0C`), the cross-out mark blit (`FUN_801DBC30`), the command ring's Rot stamp / Curse plate and the arts-entry Rot stamp (`RingMarks`), the per-party-size anchors, and `FUN_801D84C0`'s battle-result message buffers (victory with spoils, defeat, escaped, escape failed) - not party-name panels. |

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

- **Battle presentation** - `battle_actor_draw` (`FUN_80048A08`'s per-object decisions),
  `battle_pose_blend`, `battle_anim_rate` (arts slow-motion),
  `battle_impact_fx`,
  `battle_hp_bar` / `battle_gauge` (bar ramp, gauge colour),
  `battle_gauge_rearm`, `battle_commit_log`, `battle_cursor_pose`,
  `battle_record_writer`, `battle_cue_group`, `move_no_effect_guard` (the
  "No effect." banner).
- **Casts** - `battle_cast_census`, `battle_cast_cue`,
  `battle_cast_dispatch` (the action SM's side of a cast); the module tick
  bodies and their camera are
  [`legaia-engine-cast`](../engine-cast/README.md)'s. See
  [`cast-module.md`](../../docs/subsystems/cast-module.md).
- **Battle rules leaves** - `seru_side_effect` (the Seru-magic side-effect
  stager), `nested_record_copy`, `rgb_wash_scale` and `bgr555_grey` (the nested
  record copy, the RGB modulate and the petrify grey), `mes_escape_append` (the caption's item-name token
  splice), `panel_backread_loader` (PROT 0978's staged loader),
  `battle_damage_wrappers` (`FUN_801DD4B0` / `FUN_801DD6B4`),
  `battle_target_group` (`FUN_801DCEAC`), `battle_separation`,
  `battle_stream_slot` (the `summon.dat` / `readef.DAT`
  streaming transfer), `battle_slot` (retail's fixed actor-pool numbering -
  monster `k` at pool slot `3 + k` - against the engine's compact seating).
