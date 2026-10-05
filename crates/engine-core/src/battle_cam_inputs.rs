//! The battle camera's **inputs** and its per-battle state, derived from the
//! world once for every host.
//!
//! The phase-scripted camera itself is `legaia_engine_vm::battle_cam_script`
//! (`drive` + `BattleCamera`). What it is fed - the retail phase, the acting
//! actor, the post-strike target, the formation box, the case-6 framing
//! context, the per-art attack channels - is a pure read of [`World`], and it
//! used to be computed twice: the native window's `battle_cam_inputs` and the
//! browser play page's `derive_battle_cam`, identical today and pinned to each
//! other by mirror tests. It lives here now, once, and so does the camera
//! state: [`World::tick_battle_camera`] steps it from [`World::tick`], so a
//! host's render resources can no longer decide whether the camera runs (the
//! browser page used to hold the state inside its battle render build and
//! skip the step while that build was absent).
//!
//! REF: FUN_801D5854 (the framing cases the script ports), FUN_801E295C

use crate::world::World;
use legaia_engine_vm::battle_cam_script as script;
use legaia_engine_vm::battle_formulas as vm_formulas;

/// Derive the shared battle-camera drive inputs from the live world state -
/// phase, acting actor, formation box.
///
/// Retail rebuilds the submenu close-up from the acting actor's own record
/// on every open (`FUN_801D5854` case 0), so the framing follows whoever
/// owns the menu rather than staying pinned to the measured solo-Vahn pose:
/// facing drives the yaw, the character id keys the per-model height table,
/// and the actor's world position is the focus the camera orbits.
pub fn battle_cam_inputs(world: &World) -> script::BattleCamInputs {
    let acting_slot = world
        .battle
        .command
        .as_ref()
        .map(|c| c.actor)
        .unwrap_or(world.battle_ctx.active_actor);
    // The per-member surfaces own the close-up: the command ring (`0x28`),
    // the item / magic windows (`0x3C` / `0x46`) and the arts input (`0x50`,
    // `world.battle.arts_input` - the saved-chain list `arts_menu` is a
    // different session). Only the round's Begin / Run prompt keeps the far
    // framing (`script::phase_for_state` carries the retail captures that
    // separate them).
    // The sparring caption is a dialogue close-up too: the side-band's
    // stage-1 arm aims the camera at the first monster seat through
    // `FUN_801D829C` with `TR (0, 0x500, 0x400)` (`0x80056324..0x80056364`) -
    // the Dialogue framing - for exactly the span its hold `ctx[+0x6B0]` is up.
    let caption_up = world.battle.stage_id == crate::battle_sideband::STAGE_SPARRING
        && world.battle.sideband.hold != 0;
    let phase = script::phase_for_state(
        world.dialog.current.is_some() || world.dialog.inline.is_some() || caption_up,
        member_surface_open(world),
        world.battle_ctx.action_state,
        battle_done_band(world, acting_slot),
    );
    let actor_at = |slot: u8, party_slot: Option<u8>| {
        let a = world.actors.get(slot as usize)?;
        // Retail's height key is `DAT_8007BD10[slot]`, the 1-based
        // party-record selector; the engine's party rows are that record
        // order, so the row index + 1 is the same id.
        let height = party_slot.and_then(|p| {
            world
                .tables
                .battle_camera_heights
                .as_ref()
                .and_then(|t| t.height_for_char_id(p + 1))
                .map(|h| h as f32)
        });
        Some(script::BattleCamActor {
            // The **battle** heading `actor[+0x46]`, not the field heading
            // `+0x26`. Both framing arms subtract `+0x46`, and the action SM
            // writes it at `FUN_801E295C` case `0x14`
            // (`0x801E32EC..0x801E3318`: `(bearing(target -> attacker) +
            // 0x800) & 0xFFF`); the port keeps it in
            // `Actor::battle.facing_angle`.
            facing: i32::from(a.battle.facing_angle & 0xFFF),
            world: [
                a.move_state.world_x as f32,
                a.move_state.world_y as f32,
                a.move_state.world_z as f32,
            ],
            height,
        })
    };
    let acting_seat = world
        .battle
        .command
        .as_ref()
        .map_or(acting_slot, |c| c.actor);
    let acting = match world.battle.command.as_ref() {
        Some(c) => actor_at(c.actor, Some(c.party_slot)),
        // A member's submenu outlives its ring session: the arts input and
        // the magic / arts windows carry the member themselves, and a party
        // seat's actor slot is its party row, so the per-character height
        // keys the same way the ring's does.
        None => match submenu_member(world) {
            Some(m) => actor_at(m, Some(m)),
            None => actor_at(acting_slot, None),
        },
    };
    // The body pair `+0x3C` / `+0x40` (`World::refresh_battle_body_pairs`).
    let acting_body = world
        .actors
        .get(usize::from(acting_seat))
        .and_then(|a| a.battle.seat)
        .map(|(x, z)| [f32::from(x), f32::from(z)]);
    // The target cursor owns the framing while it is up (cases `1` / `3`,
    // re-armed by every cursor step of the menu driver); a dialogue box
    // still takes precedence, and an action never runs under a live cursor.
    let cursor = battle_cursor_framing(world, &actor_at);
    let phase = match (phase, cursor) {
        (script::BattleCamPhase::Menu | script::BattleCamPhase::Submenu, Some(c)) => match c {
            script::CursorFraming::Enemy { .. } => script::BattleCamPhase::TargetEnemy,
            script::CursorFraming::Ally(_) => script::BattleCamPhase::TargetAlly,
        },
        (p, _) => p,
    };
    let inputs = script::BattleCamInputs {
        phase,
        acting,
        target: battle_post_action_target(world, acting_slot),
        // The far menu framing sizes its depth to - and centres on - the live
        // formation's X/Z bounding box (`FUN_801D5854` case 9).
        formation: battle_formation_box(world),
        action: battle_action_framing(world, acting_slot),
        // Retail's battle init zeroes `_DAT_8007B792`; the port opens on the
        // field camera's azimuth, through the shared on-axis guard (a port
        // judgement, `docs/subsystems/battle.md`).
        entry_yaw: script::battle_entry_yaw(world.locomotion.camera_azimuth),
        shake_amplitude: world.camera.shake_amplitude,
        attack: battle_attack_channels(world, world.battle_ctx.active_actor),
        // The yaw counter `ctx[+0x6DA]` is re-seeded on the action SM's
        // state edges (`BattleCamera::observe_action_state`).
        action_state: world.battle_ctx.action_state,
        active_commits: world.battle_ctx.active_clip_commits,
        camera_option: world.toggles.battle_camera as u8,
        swing_reseed: (
            world.battle_ctx.swing_yaw_seeds,
            world.battle_ctx.swing_yaw_coin,
        ),
        acting_body,
        cursor,
        spell_cam: world.battle.spell_cam,
        // A fight opens on the SCUS frame driver's entry sweep, before the
        // battle tick frames anything (`BattleCamera::start_entry_sweep`).
        entry_sweep: true,
    };
    battle_end_cam_inputs(world, inputs, actor_at)
}

/// What the battle target cursor rests on, as the camera frames it
/// (`script::CursorFraming`), or `None` when no single-target cursor is up.
///
/// Retail's menu driver `FUN_801D388C` re-arms `FUN_801D5854` on every
/// cursor step against the cursor's scope: one enemy takes case `1` on the
/// commanding member (`0x801D43C0..0x801D43C8`), one party member case `3`
/// on that member (`0x801D43D8..0x801D43F8`), and a whole side the no-op
/// cases `4` / `5`. The Attack command's `Auto` / `Command` prompt and the
/// cursor it opens (steps `0x0C` / `0x2D` / `0x30`) take the enemy arm on the
/// member's current `+0x1DD` before the cursor has moved
/// (`j 0x801d43c0` at `0x801D3E30`).
///
/// The ring's attack cursor and the spell picker's cursor are the two the
/// engine raises; the item window's party picker draws its own pointer
/// inside the window and is left on the close-up.
fn battle_cursor_framing(
    world: &World,
    actor_at: &dyn Fn(u8, Option<u8>) -> Option<script::BattleCamActor>,
) -> Option<script::CursorFraming> {
    use crate::battle_input::CommandPhase;
    use crate::target_picker::{CursorRow, PickerState};
    let party_count = world.party.party_count.clamp(1, 3);
    let enemy_at = |slot: u8| -> Option<script::CursorFraming> {
        let a = world.actors.get(usize::from(slot))?;
        (slot >= party_count && a.battle.liveness != 0).then_some(script::CursorFraming::Enemy {
            target: [a.move_state.world_x as f32, a.move_state.world_z as f32],
        })
    };
    let from_picker = |state: PickerState| match state {
        PickerState::Cursor {
            row: CursorRow::Enemy,
            slot,
        } => enemy_at(party_count.saturating_add(slot)),
        PickerState::Cursor {
            row: CursorRow::Ally,
            slot,
        } => actor_at(slot, Some(slot)).map(script::CursorFraming::Ally),
        PickerState::Done(_) => None,
    };
    let b = &world.battle;
    if let Some(c) = b.command.as_ref() {
        return match &c.phase {
            CommandPhase::Targeting { picker, .. } => from_picker(picker.state()),
            CommandPhase::AttackMode { .. } => {
                // The member's `+0x1DD` as the ring left it; a stale party
                // or dead slot reads as the first standing monster, which is
                // where the cursor opens.
                let own = world
                    .actors
                    .get(usize::from(c.actor))
                    .map(|a| a.battle.active_target)
                    .and_then(enemy_at);
                own.or_else(|| (party_count..world.actors.len() as u8).find_map(enemy_at))
            }
            _ => None,
        };
    }
    b.spell_menu
        .as_ref()
        .and_then(|s| s.picker())
        .and_then(|p| from_picker(p.state()))
}

/// Whether a per-member command surface is up - the surfaces retail films
/// with the case-`0` over-the-shoulder close-up (`FUN_801D5854(slot, 0)`
/// from the menu driver `FUN_801D388C`).
///
/// Every retail battle capture with the command-flow byte `ctx[+0x06]` on
/// the ring (`0x28`) or the arts input (`0x50`) reads the case-`0` pose -
/// pitch `0x20`, `TR (-0x200, height[char], prescale(0x600) = 2457)`, yaw
/// `0x8F0 - actor[+0x46]`, focus on the member - while every capture on the
/// round prompt (`0x1E`) reads case `9`'s far framing. The item and magic
/// windows (`0x3C` / `0x46`) are pickers of the same member and keep its
/// close-up.
fn member_surface_open(world: &World) -> bool {
    use crate::battle_input::CommandPhase;
    let b = &world.battle;
    b.arts_menu.is_some()
        || b.arts_input.is_some()
        || b.spell_menu.is_some()
        || b.item_menu.is_some()
        || b.command.as_ref().is_some_and(|c| {
            matches!(
                c.phase,
                CommandPhase::Menu { .. }
                    | CommandPhase::StepBack
                    | CommandPhase::OpenItemMenu
                    | CommandPhase::OpenSpellMenu
                    | CommandPhase::OpenArtsMenu
            )
        })
}

/// The party seat whose submenu is open when no ring session is, if any.
fn submenu_member(world: &World) -> Option<u8> {
    let b = &world.battle;
    let m = b
        .arts_input
        .as_ref()
        .map(|s| s.actor)
        .or_else(|| b.arts_menu.as_ref().map(|s| s.actor))
        .or_else(|| b.spell_menu.as_ref().map(|s| s.actor))
        // The item window carries no member of its own; it is the picker of
        // the member whose ring opened it, which `open_battle_command` left
        // in `active_actor`. Without this the window fell back to the
        // un-keyed height, and the close-up - which now re-arms on any change
        // of its actor - would glide off the member's own height.
        .or_else(|| b.item_menu.as_ref().map(|_| world.battle_ctx.active_actor))?;
    (m < world.party.party_count).then_some(m)
}

/// The battle-end sequence's framing (`FUN_8004E568`, which runs in place
/// of the action SM once the battle-end signal `DAT_8007BD71 == 0xFE` is
/// up), folded over the in-fight inputs. Outside the sequence, and for an
/// escape, the inputs pass through.
///
/// The sequencer frames its **pose actor** `ctx[+0x13]` on every frame it
/// runs, in one of two ways:
///
/// - **The load window** (phases `0..=4`, and the side-band stream hold at
///   its head, `0x8004E5C0..0x8004E624` / `0x8004EE10..0x8004EE98`): it
///   stores `ctx[+0xD] = 1`, forces a party seat's target `actor[+0x1DD]`
///   into the monster band `3..=6` (`3` when it is not), and calls
///   `FUN_801D5854(seat, 8)` - the end-of-action shot, half-turned by the
///   style. Every monster is down by then, so case 8 takes its dead-target
///   re-frame.
/// - **The results frame onward** (`0x8004FC80..0x8004FC90`): it stores
///   `ctx[+0xD] = 0` and calls `FUN_801D5854(seat, 6)`. With the signal up
///   and a party seat, case 6 takes its **battle-over arm** - the close-up
///   behind the posing character whose win-pose script moves the camera
///   with the pose ([`script::battle_over_script`]). The arm reads the
///   display trio `+0x3C / +0x3E / +0x40` and the latched pose `+0x1DB`.
///
/// The escape arm (`0x67`) returns before either call (`0x8004E720`), so an
/// escape keeps whatever framing the fight left.
///
/// The per-art attack channel is not armed: the win pose is not an art, and
/// the one retail capture of the results hold (`noa_levelup_banner`) reads
/// the battle-over arm's own pose.
fn battle_end_cam_inputs(
    world: &World,
    mut inputs: script::BattleCamInputs,
    actor_at: impl Fn(u8, Option<u8>) -> Option<script::BattleCamActor>,
) -> script::BattleCamInputs {
    use crate::world::VictoryPhase;
    use legaia_engine_vm::battle_action::BattleEndCause;
    let Some(seq) = world.battle.victory else {
        return inputs;
    };
    if seq.cause == BattleEndCause::Escaped {
        return inputs;
    }
    let seat = seq.pose_actor;
    let party_count = usize::from(world.party.party_count);
    let party = seat < party_count;
    let roster = party.then(|| world.party_roster_slot(seat) as u8);
    let mut actor = actor_at(seat as u8, roster);
    let display = world.battle_display_trio(seat);
    // Both arms frame the display trio `+0x3C / +0x3E / +0x40`.
    if let (Some(a), Some(d)) = (actor.as_mut(), display) {
        a.world = d;
    }
    inputs.attack = None;
    inputs.acting_body = display.map(|d| [d[0], d[2]]);
    inputs.action = script::ActionFraming {
        party_slot: party,
        battle_over: true,
        char_id: roster.map_or(0, |r| r + 1),
        anim_id: seq.pose_id.unwrap_or(0),
        ..inputs.action
    };
    match seq.phase {
        VictoryPhase::Loading { .. } => {
            inputs.action.style = 1;
            // `actor[+0x1DD]` forced into the monster band: retail slot `3`
            // is the engine's first monster seat, right behind the party.
            let target_seat = world
                .actors
                .get(seat)
                .map(|a| usize::from(a.battle.active_target))
                .filter(|t| (party_count..party_count + 4).contains(t))
                .unwrap_or(party_count);
            // The same pass turns the target to face the pose actor's back
            // (`actor[+0x46] + 0x800` stored into the target's `+0x46`,
            // `0x8004EE7C..0x8004EE9C`), which is the heading case 8's
            // dead-target yaw reads.
            let facing = actor.map_or(0, |a| (a.facing + 0x800) & 0xFFF);
            inputs.target = world.actors.get(target_seat).map(|t| {
                let live = t.active && t.battle.hp > 0;
                script::PostActionTarget {
                    world: [
                        f32::from(t.move_state.world_x),
                        f32::from(t.move_state.world_y),
                        f32::from(t.move_state.world_z),
                    ],
                    live,
                    facing,
                    // A monster that is down when the battle ends has lost
                    // its node (`noa_levelup_banner`: every dead seat's
                    // `+4` reads zero), so case 8 takes its stand-off arm.
                    node_gone: !live,
                    ..script::PostActionTarget::default()
                }
            });
            inputs.phase = script::BattleCamPhase::ActionEnd;
        }
        VictoryPhase::Results { .. } | VictoryPhase::Exit { .. } => {
            inputs.action.style = 0;
            inputs.phase = script::BattleCamPhase::Action;
        }
    }
    inputs.acting = actor;
    inputs
}

/// The live formation's X/Z bounding box - retail's case-9 min/max walk.
///
/// Presence is the **world** fact - a party seat, or a seat with a
/// `battle_monster_id` - never a render fact like a mesh binding: retail's
/// walk gates on the live-HP halfword `actor[+0x14C]` (`0x801D7000`) and
/// touches no mesh.
pub fn battle_formation_box(world: &World) -> Option<script::FormationBox> {
    let pc = world.party.party_count as usize;
    let mut bbox: Option<script::FormationBox> = None;
    for (i, a) in world.actors.iter().enumerate() {
        if !(i < pc || a.battle_monster_id.is_some()) {
            continue;
        }
        script::FormationBox::extend(
            &mut bbox,
            a.move_state.world_x as f32,
            a.move_state.world_z as f32,
        );
    }
    bbox
}

/// The acting actor's target as the post-strike framings read it: retail's
/// `actor[+0x1DD]` indexed into the 8-slot actor table `0x801C9370`.
///
/// Case 7 orbits the **midpoint** of the acting actor and this one; case 8
/// orbits this one alone and takes its actor-only arm when `actor[+0x1DD] >=
/// 8` or the target's node is dead (`live`). The engine's stand-in for the
/// node test is the live-HP halfword the rest of the camera path keys on.
pub fn battle_post_action_target(
    world: &World,
    acting_slot: u8,
) -> Option<script::PostActionTarget> {
    let acting = world.actors.get(acting_slot as usize)?;
    let slot = acting.battle.active_target;
    if usize::from(slot) >= 8 {
        return None;
    }
    let t = world.actors.get(usize::from(slot))?;
    // Both cases read the body pair `+0x3C` / `+0x40` for X / Z and the
    // live `+0x36` for Y; the live-target arm sizes TR.y off the display
    // height `+0x3E`.
    let (bx, bz) = t
        .battle
        .seat
        .unwrap_or((t.move_state.world_x, t.move_state.world_z));
    let display_y = world
        .battle_display_trio(usize::from(slot))
        .map_or(t.move_state.world_y as f32, |d| d[1]);
    let party = usize::from(slot) < world.party.party_count as usize;
    Some(script::PostActionTarget {
        world: [f32::from(bx), t.move_state.world_y as f32, f32::from(bz)],
        display_y,
        party,
        height: party
            .then(|| {
                world
                    .tables
                    .battle_camera_heights
                    .as_ref()
                    .and_then(|h| h.height_for_char_id(slot + 1))
                    .map(|h| h as f32)
            })
            .flatten(),
        monster_id: t.battle_monster_id.map_or(0, |id| id as u8),
        animating: world.battle_current_anim(usize::from(slot)) != 0,
        live: t.active && t.battle.hp > 0,
        facing: i32::from(t.battle.facing_angle & 0xFFF),
        // Retail's node test reads the low 24 bits of the colour word `+0x4`
        // (`0x801D6AA8..0x801D6AC0`): a body the defeat fade (`+0x21C = 2`)
        // has walked to black reads gone. The engine's resting word `0` is
        // the neutral colour, so only the fade's zero counts.
        node_gone: t.battle.render_flag == vm_formulas::STATE_DEFEAT_FADE
            && t.battle.render_color & 0x00FF_FFFF == 0,
        // `0x8007BD0D == 0` is folded into the latch: its one writer raises
        // it only for a lone-monster formation.
        lone_defeat: world.battle_ctx.scripted_fight != 0
            && world.battle_ctx.lone_defeat_latch != 0,
    })
}

/// The Done band's per-category fork inputs (`FUN_801E295C`'s `0x50` /
/// `0x51` arms, `script::done_band_phase`): the acting actor's committed
/// category `actor[+0x1DE]`, whether its seat is a party one
/// (`ctx[+0x13] < 3`), and whether its target's live HP has reached zero.
pub fn battle_done_band(world: &World, acting_slot: u8) -> script::DoneBandInputs {
    script::DoneBandInputs {
        category: world
            .actors
            .get(usize::from(acting_slot))
            .map_or(0, |a| a.battle.action_category),
        party_slot: usize::from(acting_slot) < world.party.party_count as usize,
        target_dead: battle_post_action_target(world, acting_slot).is_some_and(|t| !t.live),
        target_knocked: battle_target_slot(world, acting_slot)
            .is_some_and(|t| world.battle_on_knockdown(t)),
        target_death_clip: battle_target_slot(world, acting_slot)
            .is_some_and(|t| matches!(world.battle_current_anim(t), 7 | 8)),
    }
}

/// The acting actor's target seat `actor[+0x1DD]`, when it names one.
fn battle_target_slot(world: &World, acting_slot: u8) -> Option<usize> {
    let slot = world
        .actors
        .get(usize::from(acting_slot))?
        .battle
        .active_target;
    (usize::from(slot) < 8).then_some(usize::from(slot))
}

/// The per-art attack camera's track table, re-read from the battle-action
/// overlay the scene loader retains for the move-FX path
/// (`World::tables.move_power_overlay`). `None` on a host that never loaded it.
pub fn battle_attack_tracks(
    world: &World,
) -> Option<legaia_asset::battle_attack_camera_table::AttackCameraTracks> {
    let overlay = world.tables.move_power_overlay.as_ref()?;
    legaia_asset::battle_attack_camera_table::parse(overlay)
}

/// The per-art attack camera's per-actor channels for the acting slot, or
/// `None` when the channel is not armed - in the order `FUN_801D71B8` tests
/// them (`0x801D71B8..0x801D72D4`) plus its call site's outer gate
/// (`0x801D7138..0x801D7178`): a real target slot, category Attack, a party
/// seat, and a character id with a camera script.
///
/// `anim_frame` is retail `actor[+0x22C][+0x68]`, a sixteenths-of-a-keyframe
/// cursor; the engine's animation player exposes whole keyframes, so the
/// conversion is a `<< 4`.
pub fn battle_attack_channels(world: &World, acting_slot: u8) -> Option<script::AttackCamChannels> {
    use legaia_engine_vm::battle_attack_camera as cam;
    if usize::from(acting_slot) >= world.party.party_count as usize {
        return None;
    }
    let a = world.actors.get(acting_slot as usize)?;
    if a.battle.action_category != cam::CATEGORY_ATTACK {
        return None;
    }
    if !cam::outer_gate(0, a.battle.active_target) {
        return None;
    }
    // `FUN_801D71B8`'s first test: the target's live HP `+0x14C`
    // (`0x801D71E8..0x801D7208`). A swing that has killed its target hands
    // the frame back to the case `FUN_801D5854` armed - case 8's death
    // re-frame on the post-strike band.
    if world
        .actors
        .get(usize::from(a.battle.active_target))
        .is_none_or(|t| t.battle.hp == 0)
    {
        return None;
    }
    let character = cam::character_arm(acting_slot + 1)?;
    Some(script::AttackCamChannels {
        character,
        art_id: a.battle.latched_anim,
        arm_select: a.battle.hit_count_bound,
        anim_frame: a
            .battle_animation
            .as_ref()
            .map(|p| p.current_frame().saturating_mul(16))
            .unwrap_or(0),
    })
}

/// What `FUN_801DC0A0(slot, case)` reads, off the live world.
///
/// The effect slot `ctx[+0x1144]` is the move-FX block's launch point
/// (`CastFxState::move_fx_streak`); the engine does not fly it
/// (`FUN_801E09F8`'s homing is render-track), so a projectile shot frames
/// the launch rather than the flame in flight, and the caster's own pair
/// stands in before any terminator. The target's body radius is the party
/// value; the monster pick's `+0x21` first magic slot is the catalog's first
/// live one.
///
/// REF: FUN_801DC0A0, FUN_801DCEAC, FUN_801F0348
pub fn spell_cam_inputs(world: &World, slot: u8, case: u8) -> script::SpellCamInputs {
    use legaia_engine_vm::battle_target_group::{GroupSlot, RENDER_FLAG_HIDDEN, target_group_aim};
    let pc = world.party.party_count;
    let Some(a) = world.actors.get(usize::from(slot)) else {
        return script::SpellCamInputs {
            case,
            ..Default::default()
        };
    };
    let world_of = |x: i16, y: i16, z: i16| [f32::from(x), f32::from(y), f32::from(z)];
    let actor = script::BattleCamActor {
        facing: i32::from(a.battle.facing_angle & 0xFFF),
        world: world_of(
            a.move_state.world_x,
            a.move_state.world_y,
            a.move_state.world_z,
        ),
        height: None,
    };
    let code = a.battle.active_target;
    let target = if usize::from(code) < 8 {
        world
            .actors
            .get(usize::from(code))
            .map_or(script::SpellCamTarget::None, |t| {
                script::SpellCamTarget::Slot {
                    world: [
                        f32::from(t.move_state.world_x),
                        f32::from(t.move_state.world_z),
                    ],
                    radius: script::PARTY_BODY_RADIUS,
                }
            })
    } else {
        // `FUN_801DCEAC` walks retail numbering (party `0..3`, monsters
        // `3..7`); the engine seats monsters straight after the party.
        let mut slots = [GroupSlot {
            live: false,
            x: 0,
            z: 0,
        }; 8];
        for (retail, out) in slots.iter_mut().enumerate() {
            let retail = retail as u8;
            let engine = if retail < 3 {
                if retail >= pc {
                    continue;
                }
                retail
            } else {
                pc + (retail - 3)
            };
            let Some(t) = world.actors.get(usize::from(engine)) else {
                continue;
            };
            let live = retail < 3
                || (t.battle_monster_id.is_some() && t.battle.render_flag != RENDER_FLAG_HIDDEN);
            *out = GroupSlot {
                live,
                x: t.move_state.world_x,
                z: t.move_state.world_z,
            };
        }
        target_group_aim(code, &slots).map_or(script::SpellCamTarget::None, |g| {
            script::SpellCamTarget::Group {
                centroid: [-f32::from(g.centroid_x), -f32::from(g.centroid_z)],
                extent: i32::from(g.extent),
            }
        })
    };
    let streak = &world.casting.move_fx_streak;
    // `ctx[+0x1144]` - homing slot `0`, which the flight moves off the
    // launch point and lands on the target.
    let fx_position = world
        .casting
        .homing
        .lead()
        .map(|[x, _, z]| (x, 0, z))
        .or(streak.launch)
        .map_or([actor.world[0], actor.world[2]], |(x, _, z)| {
            [x as f32, z as f32]
        });
    let monster = slot >= pc;
    let first_magic_3a = monster
        && a.battle_monster_id
            .and_then(|id| world.tables.monster_catalog.get(id))
            .and_then(|d| d.magic_attacks.first().copied())
            == Some(0x3A);
    script::SpellCamInputs {
        case,
        actor,
        char_id: if monster {
            0
        } else {
            world.party_roster_slot(usize::from(slot)) as u8 + 1
        },
        monster_seat: monster,
        first_magic_3a,
        group_target: code >= 8,
        target,
        fx_position,
        live_yaw: 0.0,
        frame_step: script::SPELL_CAM_FRAME_STEP,
        fx_timer: streak.counter_word as i16,
        // `ctx[+0x24D]`: the census's count, which the engine's own census
        // leaves at zero for want of the slots; the flown slots supply it.
        fx_children: world
            .battle_ctx
            .magic_recovery_gate
            .max(world.casting.homing.children()),
        fx_phase: streak.phase,
        accum: 0,
        depth_raw: world.battle.camera_frame_height as i32,
        anim_cursor: a
            .battle_animation
            .as_ref()
            .map_or(0, |p| p.current_frame().saturating_mul(16)),
        hit_bound: a.battle.hit_count_bound,
        current_anim: world.battle_current_anim(usize::from(slot)),
    }
}

/// The `FUN_801D5854` case-6 context inputs for the acting slot: `party_slot`
/// is retail's `ctx[+0x13] < 3`, `char_id` its `DAT_8007BD10[slot]`,
/// `depth_raw` `ctx[+0x6D0]` (what `camera_height_for_frame` last computed),
/// `style` the live `ctx[+0xD]` framing variant. `battle_over` is
/// `DAT_8007BD71 == 0xFE`, which reads `0xFF` for the whole of a running
/// fight, so it is `false` here.
pub fn battle_action_framing(world: &World, acting_slot: u8) -> script::ActionFraming {
    let party = usize::from(acting_slot) < world.party.party_count as usize;
    script::ActionFraming {
        party_slot: party,
        battle_over: false,
        depth_raw: world.battle.camera_frame_height as i32,
        yaw_base: 0,
        style: world.battle_ctx.camera_variant,
        char_id: if party { acting_slot + 1 } else { 0 },
        // Read only by the battle-over arm ([`battle_end_cam_inputs`]); the
        // counters are the camera's own.
        anim_id: 0,
        accum: 0,
        ramp: 0,
        body_radius: script::PARTY_BODY_RADIUS,
    }
}

impl World {
    /// Step the phase-scripted battle camera one tick through the shared
    /// `battle_cam_script::drive`: created on battle entry (snapped to the
    /// entry phase's framing), dropped outside battle so the next fight
    /// re-snaps, stepped on the retail display-frame clock
    /// (`clock.display_frames`, one camera step per 2 vsyncs). Run from
    /// [`World::tick`], so the gate is the world's mode and nothing a host
    /// renders.
    pub fn tick_battle_camera(&mut self) {
        let active = self.mode == crate::world::SceneMode::Battle;
        let inputs = battle_cam_inputs(self);
        let tracks = battle_attack_tracks(self);
        let frames = self.clock.display_frames;
        // The camera's draws (shake pair, strike-loop yaw coin, per-art
        // track column) are retail `rand()` calls on the one process-wide
        // seed, so they draw on the world stream in tick order.
        script::drive_on_stream(
            &mut self.battle.camera,
            active,
            inputs,
            frames,
            tracks.as_ref(),
            &mut self.rng_state,
        );
        self.battle.spell_cam = None;
    }

    /// The battle camera's current pose, or the shared boot pose before the
    /// first tick armed it. While a battle-stage module owns the camera
    /// globals (the Cort arrival / form transition,
    /// [`crate::battle_stage_module`]) this is the module's camera instead,
    /// so both hosts draw what it frames.
    pub fn battle_cam_pose(&self) -> script::BattleCamPose {
        if let Some(c) = self.battle.stage_camera.as_ref() {
            return script::BattleCamPose {
                pitch: c.pitch as f32,
                yaw: c.yaw as f32,
                tr: c.tr.map(|v| v as f32),
                focus: c.focus.map(|v| v as f32),
            };
        }
        self.battle
            .camera
            .as_ref()
            .map(|c| c.pose())
            .unwrap_or(script::BOOT_POSE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle_input::BattleCommandSession;
    use crate::world::SceneMode;
    use script::BattleCamPhase;

    /// Run a fresh fight's battle-entry sweep out and let the far framing
    /// that takes over from it land, so a test of a framing starts from a
    /// fight already running.
    fn run_entry_sweep_out(world: &mut World) {
        let mut over = None;
        for step in 0..64 {
            world.clock.display_frames += 2;
            world.tick_battle_camera();
            if world
                .battle
                .camera
                .as_ref()
                .is_some_and(|c| c.entry_sweep_counter().is_none())
            {
                over = Some(step);
                break;
            }
        }
        assert!(over.is_some(), "the entry sweep never ended");
        for _ in 0..16 {
            world.clock.display_frames += 2;
            world.tick_battle_camera();
        }
    }

    /// Every retail capture on a member's command ring (`ctx[+0x06] = 0x28`)
    /// or arts input (`0x50`) reads case 0's close-up; every capture on the
    /// round's Begin / Run prompt (`0x1E`) reads case 9's far framing.
    #[test]
    fn the_ring_and_the_arts_input_take_the_close_up_the_round_prompt_does_not() {
        let mut world = World {
            mode: SceneMode::Battle,
            ..World::default()
        };
        world.party.party_count = 3;
        world.battle.command = Some(BattleCommandSession::new_round_open(1, 1, false));
        assert_eq!(battle_cam_inputs(&world).phase, BattleCamPhase::Menu);

        world.battle.command = Some(BattleCommandSession::new(1, 1));
        assert_eq!(battle_cam_inputs(&world).phase, BattleCamPhase::Submenu);

        world.battle.command = None;
        world.battle.arts_input = Some(crate::arts_command_input::ArtsCommandInputSession::new(
            1, 1, 100, [0; 4], 1,
        ));
        assert_eq!(battle_cam_inputs(&world).phase, BattleCamPhase::Submenu);
        assert_eq!(submenu_member(&world), Some(1), "framed on the member");
    }

    /// The command phase hands the ring from member to member with no far
    /// framing between; the live camera has to end on the member now
    /// commanding, not stay on the first one.
    #[test]
    fn the_command_camera_frames_each_commanding_member() {
        let mut world = World {
            mode: SceneMode::Battle,
            ..World::default()
        };
        world.party.party_count = 3;
        if world.actors.len() < 8 {
            world.actors.resize_with(8, Default::default);
        }
        for (slot, x) in [(0usize, 0i16), (1, -700), (2, 700)] {
            let a = &mut world.actors[slot];
            a.move_state.world_x = x;
            a.move_state.world_z = -800;
        }
        run_entry_sweep_out(&mut world);
        for member in 0u8..3 {
            world.battle_ctx.active_actor = member;
            world.battle.command = Some(BattleCommandSession::new(member, member));
            for _ in 0..16 {
                world.clock.display_frames += 2;
                world.tick_battle_camera();
            }
            let want = f32::from(world.actors[usize::from(member)].move_state.world_x);
            assert_eq!(
                world.battle_cam_pose().focus[0],
                want,
                "member {member}'s ring frames member {member}"
            );
        }
    }

    /// The attack cursor (`ctx[+0x06]` `0x78` / `0x5A`) frames with case 1
    /// on the commanding member, turned toward the monster under the cursor
    /// (`FUN_801D388C` steps `0x0C` / `0x2D` / `0x30` -> `0x801D43C0`); the
    /// shot follows the cursor across monsters and hands back to the ring's
    /// close-up when the cursor closes. The engine used to read the cursor
    /// as the far Begin / Run framing.
    #[test]
    fn the_attack_cursor_frames_the_member_toward_its_target() {
        use crate::battle_input::{BattleCommand, CommandPhase};
        use crate::target_picker::{SlotState, TargetKind, TargetPickerSession};
        let mut world = World {
            mode: SceneMode::Battle,
            ..World::default()
        };
        world.party.party_count = 1;
        if world.actors.len() < 3 {
            world.actors.resize_with(3, Default::default);
        }
        world.actors[0].move_state.world_z = -800;
        for (slot, x) in [(1usize, -600i16), (2, 600)] {
            let a = &mut world.actors[slot];
            a.move_state.world_x = x;
            a.move_state.world_z = 800;
            a.battle.liveness = 1;
        }
        let picker = |first: bool| {
            let mut monsters = [SlotState::alive(false, false); 5];
            monsters[0] = SlotState::alive(first, first);
            monsters[1] = SlotState::alive(true, true);
            TargetPickerSession::new(
                TargetKind::SingleEnemy,
                0,
                [
                    SlotState::alive(true, true),
                    SlotState::alive(false, false),
                    SlotState::alive(false, false),
                ],
                monsters,
            )
        };
        let mut session = BattleCommandSession::new(0, 0);
        session.phase = CommandPhase::Targeting {
            command: BattleCommand::Attack,
            picker: picker(true),
        };
        run_entry_sweep_out(&mut world);
        world.battle.command = Some(session);
        let inputs = battle_cam_inputs(&world);
        assert_eq!(inputs.phase, BattleCamPhase::TargetEnemy);
        assert_eq!(
            inputs.cursor,
            Some(script::CursorFraming::Enemy {
                target: [-600.0, 800.0]
            })
        );
        let settle = |world: &mut World| {
            for _ in 0..16 {
                world.clock.display_frames += 2;
                world.tick_battle_camera();
            }
            world.battle_cam_pose()
        };
        let left = settle(&mut world);
        assert_eq!(left.pitch, 256.0, "case 1 pitch 0x100");
        assert_eq!(left.tr[1], 1536.0, "case 1 TR.y 0x600");
        assert_eq!(left.focus[2], -800.0, "focused on the member");
        // The cursor moves to the other monster: the shot re-arms toward it,
        // mirrored about the seat axis.
        if let Some(c) = world.battle.command.as_mut() {
            c.phase = CommandPhase::Targeting {
                command: BattleCommand::Attack,
                picker: picker(false),
            };
        }
        let right = settle(&mut world);
        assert_ne!(left.yaw, right.yaw, "the shot follows the cursor");
        assert_eq!(
            (left.yaw + right.yaw).rem_euclid(4096.0),
            0.0,
            "mirror targets, mirror yaws: {} / {}",
            left.yaw,
            right.yaw
        );
        // Cursor closed back onto the ring: the member's close-up again.
        world.battle.command = Some(BattleCommandSession::new(0, 0));
        assert_eq!(battle_cam_inputs(&world).phase, BattleCamPhase::Submenu);
    }
}
