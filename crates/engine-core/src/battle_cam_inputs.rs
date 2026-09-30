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
    // The **input** pickers own the close-up; the top-level command chooser
    // keeps the far framing (`script::phase_for_state` carries the two retail
    // framebuffers that separate them).
    // The sparring caption is a dialogue close-up too: the side-band's
    // stage-1 arm aims the camera at the first monster seat through
    // `FUN_801D829C` with `TR (0, 0x500, 0x400)` (`0x80056324..0x80056364`) -
    // the Dialogue framing - for exactly the span its hold `ctx[+0x6B0]` is up.
    let caption_up = world.battle.stage_id == crate::battle_sideband::STAGE_SPARRING
        && world.battle.sideband.hold != 0;
    let phase = script::phase_for_state(
        world.dialog.current.is_some() || world.dialog.inline.is_some() || caption_up,
        world.battle.arts_menu.is_some()
            || world.battle.spell_menu.is_some()
            || world.battle.item_menu.is_some(),
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
        None => actor_at(acting_slot, None),
    };
    // The body pair `+0x3C` / `+0x40` (`World::refresh_battle_body_pairs`).
    let acting_body = world
        .actors
        .get(usize::from(acting_seat))
        .and_then(|a| a.battle.seat)
        .map(|(x, z)| [f32::from(x), f32::from(z)]);
    script::BattleCamInputs {
        phase,
        acting,
        target: battle_post_action_target(world, acting_slot),
        // The far menu framing sizes its depth to - and centres on - the live
        // formation's X/Z bounding box (`FUN_801D5854` case 9).
        formation: battle_formation_box(world),
        action: battle_action_framing(world, acting_slot),
        // `_DAT_8007B792` is one global shared with the field camera, and
        // nothing on the battle-entry path zeroes it - a fight inherits the
        // live azimuth, through the shared on-axis guard.
        entry_yaw: script::battle_entry_yaw(world.locomotion.camera_azimuth),
        shake_amplitude: world.camera.shake_amplitude,
        attack: battle_attack_channels(world, world.battle_ctx.active_actor),
        // The yaw counter `ctx[+0x6DA]` is re-seeded on the action SM's
        // state edges (`BattleCamera::observe_action_state`).
        action_state: world.battle_ctx.action_state,
        active_commits: world.battle_ctx.active_clip_commits,
        acting_body,
    }
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
    Some(script::PostActionTarget {
        world: [
            t.move_state.world_x as f32,
            t.move_state.world_y as f32,
            t.move_state.world_z as f32,
        ],
        live: t.active && t.battle.hp > 0,
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
    }
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
        script::drive(
            &mut self.battle.camera,
            active,
            inputs,
            frames,
            tracks.as_ref(),
        );
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
