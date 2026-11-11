//! The battle bodies' draw reads (`legaia_engine_world_views::battle_draws`)
//! over a world driven through the battle animation tick: the after-image
//! ghosts, the weapon trail sweep, the ground shadow and the floor clip. The
//! animation state these read is pinned in engine-core's
//! `world::tests::battle_anim`.

use legaia_actor_formats::monster_archive::MonsterAnimation;
use legaia_engine_camera::projection::BattleCamPose;
use legaia_engine_core::world::{PARTY_BODY_RADIUS, World};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_world_views::battle_draws::BATTLE_SHADOW_DEPTH_LIFT;

/// Synthetic one-part action clip: `frames` keyframes translating from `tx`.
fn pose_test_clip(action_id: u8, frames: usize, tx: i16) -> MonsterAnimation {
    use legaia_actor_formats::monster_archive::PartPose;
    MonsterAnimation {
        variant_track: Default::default(),
        action_id,
        rate: 2,
        attach_key: 0,
        solo_flag: 0,
        impact_class: 0,
        effect_script: Vec::new(),
        part_count: 1,
        frame_count: frames,
        frames: (0..frames)
            .map(|f| {
                vec![PartPose {
                    tx: tx + f as i16,
                    ty: 0,
                    tz: 0,
                    rx: 0,
                    ry: 0,
                    rz: 0,
                }]
            })
            .collect(),
    }
}

fn pose_test_world() -> World {
    let mut world = World::new();
    world.actors[0].active = true;
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[8] = Some(pose_test_clip(8, 2, 100));
    clips[9] = Some(pose_test_clip(9, 2, 200));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world
}

/// A clip with enough parts to carry Vahn's weapon-bone chain
/// (`base_part 0x0C..0x0F`) and the given `+0x77` identity byte. Part `p`'s
/// translation at frame `f` is `(p, f, 0)` so the sweep's ring samples are
/// distinguishable per point and per step.
fn trail_test_clip(attach_key: u8, frames: usize) -> MonsterAnimation {
    use legaia_actor_formats::monster_archive::PartPose;
    let parts = 16usize;
    MonsterAnimation {
        variant_track: Default::default(),
        action_id: 0xC,
        rate: 2,
        attach_key,
        solo_flag: 0,
        impact_class: 0,
        part_count: parts,
        frame_count: frames,
        frames: (0..frames)
            .map(|f| {
                (0..parts)
                    .map(|p| PartPose {
                        tx: p as i16,
                        ty: f as i16,
                        tz: 0,
                        ..Default::default()
                    })
                    .collect()
            })
            .collect(),
        effect_script: Vec::new(),
    }
}

fn battle_world() -> World {
    let mut world = World::default();
    world.enter_battle(3, 1);
    world.actors[3].battle_monster_id = Some(4);
    world
}

fn pose(tr_z: f32) -> BattleCamPose {
    BattleCamPose {
        pitch: 0.0,
        yaw: 0.0,
        tr: [0.0, 0.0, tr_z],
        focus: [0.0; 3],
    }
}

/// The SpecialStarter (`0x1A`) commit fires the retail freeze arm
/// (`FUN_8004AD80` `0x8004B728..0x8004B750`): every slot frozen, the acting
/// actor at quarter speed - and its dash frames are ghost-eligible (the
/// dynamic art slot `0x11` gate of `FUN_80049348`).
#[test]
fn special_starter_commit_freezes_the_battle_and_ghosts_the_dash() {
    use legaia_engine_battle_vm::battle_anim_rate::{RATE_FROZEN, RATE_QUARTER};
    let mut world = pose_test_world();
    // The ring's slot-per-vsync reading below is the per-vsync cadence's.
    world.toggles.battle_sm_frame_cadence = false;
    world.actors[1].active = true;
    let mut bank: Vec<Option<MonsterAnimation>> = vec![None; 16];
    bank[0xA] = Some(pose_test_clip(0x1A, 60, 0));
    world.set_actor_battle_art_bank(0, std::sync::Arc::new(bank));
    world.actors[0].battle.queued_anim = 0x1A;
    world.commit_staged_battle_anim(0);
    assert_eq!(world.actors[0].battle.anim_rate.get(), RATE_QUARTER);
    assert_eq!(world.actors[1].battle.anim_rate.get(), RATE_FROZEN);
    // The commit materialised the starter at dynamic slot B (0x11).
    assert_eq!(
        world.actors[0].battle.current_anim,
        legaia_engine_vm::anim_vm::DYNAMIC_ART_SLOT_B
    );
    // The per-vsync cadence pushes a ring slot a vsync; the walk's depths
    // are battle frames, `frame_step` vsyncs each.
    world.clock.frame_step = 2;
    for _ in 0..20 {
        world.tick_battle_animations();
    }
    let ghosts = legaia_engine_world_views::battle_draws::battle_ghost_draws(&world);
    assert_eq!(ghosts.len(), 2, "two ghosts per retail walk");
    // Quarter speed: step = 8/2 = 4, ghosts trail 4 and 8 battle frames -
    // ring slots 8 and 16, slot 0 the live frame.
    let hist = &world.actors[0].battle_pose_history;
    assert_eq!(ghosts[0].pose, hist[8].pose);
    assert_eq!(ghosts[1].pose, hist[16].pose);
    assert_ne!(hist[8].pose, hist[7].pose, "the clip moves a slot a vsync");
    // Colour: Vahn's red base, the second ghost one decay dimmer.
    assert_eq!(ghosts[0].color, [0x60, 0x30, 0x30]);
    assert_eq!(ghosts[1].color, [0x50, 0x20, 0x20]);

    // A cast module's hide zeroes the word the draw tick tests ahead of the
    // walk (`0x800481CC`): the ring stays, the ghosts go with the body.
    world.actors[0].battle.render_color = 0;
    world.actors[0].battle.render_flag = 0xFF;
    assert!(legaia_engine_world_views::battle_draws::battle_ghost_draws(&world).is_empty());
}

/// A seated ghost is drawn from the slot the walk reads, re-posed from the
/// playing clip at the ring's cursor and stood on the ring's pair - the
/// capture's ghosts rather than the replay's.
#[test]
fn a_seated_ghost_ring_slot_draws_the_captured_keyframe_and_ground() {
    let mut world = pose_test_world();
    world.actors[1].active = true;
    let mut bank: Vec<Option<MonsterAnimation>> = vec![None; 16];
    bank[0xA] = Some(pose_test_clip(0x1A, 60, 0));
    world.set_actor_battle_art_bank(0, std::sync::Arc::new(bank));
    world.actors[0].battle.queued_anim = 0x1A;
    world.commit_staged_battle_anim(0);
    world.clock.frame_step = 2;
    for _ in 0..20 {
        world.tick_battle_animations();
    }
    // Quarter speed: the walk's depths are 4 and 8 battle frames.
    assert!(world.seat_battle_ghost_ring(0, 4, 0x50, [-21, 116]));
    assert!(world.seat_battle_ghost_ring(0, 8, 0x38, [-21, 106]));
    let mut at = world.actors[0].battle_animation.clone().unwrap();
    let want_near = at.seat_cursor(0x50);
    let want_far = at.seat_cursor(0x38);
    let ghosts = legaia_engine_world_views::battle_draws::battle_ghost_draws(&world);
    assert_eq!(ghosts.len(), 2);
    assert_eq!(ghosts[0].pose, want_near);
    assert_eq!([ghosts[0].pos[0], ghosts[0].pos[2]], [-21, 116]);
    assert_eq!(ghosts[1].pose, want_far);
    assert_eq!([ghosts[1].pos[0], ghosts[1].pos[2]], [-21, 106]);
    assert_eq!(ghosts[0].color, [0x60, 0x30, 0x30]);
    // Single-sided, as the live body: both shells additive read twice
    // retail's ghost.
    assert!(
        ghosts
            .iter()
            .all(|g| g.nclip_mode() == legaia_engine_core::world::BATTLE_BODY_NCLIP)
    );
    // A closed depth draws no ghost: the walk keeps the other.
    world.close_battle_ghost_ring(0, 8);
    let ghosts = legaia_engine_world_views::battle_draws::battle_ghost_draws(&world);
    assert_eq!(ghosts.len(), 1);
    assert_eq!(ghosts[0].pose, want_near);
    // A body with no playing clip has nothing to re-pose from.
    world.actors[1].battle_animation = None;
    assert!(!world.seat_battle_ghost_ring(1, 4, 0x50, [0, 0]));
}

/// Basic-attack swings (`0x0C..=0x0F`) fire no rate arm: the battle stays
/// at normal speed and no frame is ghost-eligible.
#[test]
fn direction_swing_commit_keeps_normal_speed_and_no_ghosts() {
    use legaia_engine_battle_vm::battle_anim_rate::RATE_NORMAL;
    let mut world = pose_test_world();
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 22];
    clips[0] = Some(pose_test_clip(0, 2, 0));
    clips[0x0C] = Some(pose_test_clip(0x0C, 60, 0));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.queued_anim = 0x0C;
    world.commit_staged_battle_anim(0);
    assert_eq!(world.actors[0].battle.anim_rate.get(), RATE_NORMAL);
    for _ in 0..10 {
        world.tick_battle_animations();
    }
    assert!(legaia_engine_world_views::battle_draws::battle_ghost_draws(&world).is_empty());
}

/// The trigger fires only on the matching (character, `+0x77`) pair, and
/// the sweep is the driver's cursor walk: the playing clip decoded at the
/// live cursor and then `2 * record[+0x78]` back a step down to the clip's
/// start, sixteen steps at most (`FUN_80048310`).
#[test]
fn weapon_trail_rewinds_the_cursor_on_the_trigger_clip() {
    let mut world = World::new();
    world.enter_battle(1, 0);
    // Slot 0 = battle ordinal 0 = roster slot 0 = Vahn (retail char id 1);
    // trigger clip id 0x29.
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 16];
    clips[0] = Some(trail_test_clip(0, 8));
    clips[0xC] = Some(trail_test_clip(0x29, 30));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.queued_anim = 0xC;
    world.commit_staged_battle_anim(0);
    // The commit zeroes the cursor: nothing is captured at or below zero.
    assert!(legaia_engine_world_views::battle_draws::battle_weapon_trail_draws(&world).is_empty());
    let mut seen_short = false;
    let mut seen_full = false;
    for _ in 0..12 {
        world.tick_battle_animations();
        let player = world.actors[0].battle_animation.as_ref().unwrap();
        let cursor = player.cursor_sixteenths() as i16;
        // The fixture's rate byte is 2: four sixteenths a step.
        let want = legaia_render_kernels::battle_trail::sweep_cursors(cursor, 2);
        assert_eq!(want.len(), ((cursor as usize).div_ceil(4)).min(16));
        let draws = legaia_engine_world_views::battle_draws::battle_weapon_trail_draws(&world);
        if want.len() < 2 {
            assert!(draws.is_empty(), "below the two-step floor at {cursor}");
            continue;
        }
        assert_eq!(draws.len(), 1, "one trailing actor");
        let d = &draws[0];
        assert_eq!(d.actor_slot, 0);
        assert_eq!(d.rgb, [0x80, 0x20, 0x40], "Vahn's trigger tint");
        assert_eq!(d.steps.len(), want.len(), "cursor {cursor}");
        seen_short |= want.len() < 16;
        seen_full |= want.len() == 16;
        for (k, step) in d.steps.iter().enumerate() {
            let pose = player.pose_at_sixteenths(want[k] as u16);
            for (i, pt) in step.iter().enumerate() {
                // Control points are the pose translations of parts
                // 0x0C..0x0F at the rewound cursor.
                assert_eq!(*pt, pose.bone_outputs[0x0C + i].0, "step {k} point {i}");
                assert_eq!(pt[0], (0x0C + i) as i16);
            }
        }
        // Step 0 is the live pose; deeper steps are earlier in the clip
        // (the fixture's `ty` is the keyframe index).
        assert!(d.steps[0][0][1] >= d.steps[d.steps.len() - 1][0][1]);
    }
    assert!(seen_short && seen_full, "the walk grows to the budget");
    // A hidden body (`+0x04 = 0` under `+0x21C = 0xFF`) never reaches the
    // trigger: the draw tick's colour-word test skips it with the ghosts.
    world.actors[0].battle.render_color = 0;
    world.actors[0].battle.render_flag = 0xFF;
    assert!(legaia_engine_world_views::battle_draws::battle_weapon_trail_draws(&world).is_empty());
}

/// No trail off the trigger: a different clip id on the same seat, or a
/// monster actor playing a trigger-keyed clip, draw nothing.
#[test]
fn weapon_trail_stays_dark_off_the_trigger() {
    let mut world = World::new();
    world.enter_battle(1, 0);
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 16];
    clips[0] = Some(trail_test_clip(0, 8));
    // Noa's 0x1E id on Vahn's seat: no trigger row matches.
    clips[0xC] = Some(trail_test_clip(0x1E, 30));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.queued_anim = 0xC;
    world.commit_staged_battle_anim(0);
    for _ in 0..8 {
        world.tick_battle_animations();
    }
    assert!(legaia_engine_world_views::battle_draws::battle_weapon_trail_draws(&world).is_empty());
    // A monster playing a 0x29-keyed clip: party-only trigger.
    world.actors[1].active = true;
    world.actors[1].battle_monster_id = Some(5);
    let mut mclips: Vec<Option<MonsterAnimation>> = vec![None; 16];
    mclips[0] = Some(trail_test_clip(0, 8));
    mclips[2] = Some(trail_test_clip(0x29, 30));
    world.set_actor_battle_action_clips(1, std::sync::Arc::new(mclips));
    world.actors[1].battle.queued_anim = 2;
    world.commit_staged_battle_anim(1);
    for _ in 0..8 {
        world.tick_battle_animations();
    }
    assert!(legaia_engine_world_views::battle_draws::battle_weapon_trail_draws(&world).is_empty());
}

/// The sweep ends at the clip's start whatever played before it: a second
/// commit of the same trigger clip (an art's next strike lands in the same
/// dynamic slot) starts its trail from nothing, and a rate the arts' slow
/// motion dropped leaves the walk in clip time.
#[test]
fn weapon_trail_sweep_ends_at_the_clips_start() {
    let mut world = World::new();
    world.enter_battle(1, 0);
    let mut clips: Vec<Option<MonsterAnimation>> = vec![None; 16];
    clips[0] = Some(trail_test_clip(0, 8));
    clips[0xC] = Some(trail_test_clip(0x29, 30));
    clips[0xD] = Some(trail_test_clip(0x29, 30));
    world.set_actor_battle_action_clips(0, std::sync::Arc::new(clips));
    world.actors[0].battle.queued_anim = 0xC;
    world.commit_staged_battle_anim(0);
    for _ in 0..12 {
        world.tick_battle_animations();
    }
    let full = legaia_engine_world_views::battle_draws::battle_weapon_trail_draws(&world);
    assert_eq!(full[0].steps.len(), 16);
    // A second clip of the same identity byte, staged behind the first:
    // on the tick it commits, the walk reaches back to its own start only.
    world.actors[0].battle.queued_anim = 0xD;
    let mut ticks = 0;
    while world.actors[0].battle.current_anim != 0xD {
        world.tick_battle_animations();
        ticks += 1;
        assert!(ticks < 200, "the staged clip commits at the natural end");
    }
    let cursor = world.actors[0]
        .battle_animation
        .as_ref()
        .unwrap()
        .cursor_sixteenths() as usize;
    assert!(cursor < 16, "cursor {cursor}");
    let steps = legaia_engine_world_views::battle_draws::battle_weapon_trail_draws(&world)
        .first()
        .map_or(0, |d| d.steps.len());
    assert_eq!(steps, if cursor > 4 { cursor.div_ceil(4) } else { 0 });
    // Under a dropped rate the cursor moves less a tick; the steps are
    // still four sixteenths apart, so the trail is as long as the cursor
    // allows and no longer.
    world.actors[0].battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(2);
    for _ in 0..3 {
        world.tick_battle_animations();
    }
    let cursor = world.actors[0]
        .battle_animation
        .as_ref()
        .unwrap()
        .cursor_sixteenths() as usize;
    assert!(cursor > 4 && cursor < 64, "cursor {cursor}");
    let draws = legaia_engine_world_views::battle_draws::battle_weapon_trail_draws(&world);
    assert_eq!(draws[0].steps.len(), cursor.div_ceil(4));
}

/// A drawn party body casts the 24-column subtractive disc of radius
/// `640 * 4 / 10`, under it on the floor; a pitched body casts none.
#[test]
fn a_drawn_party_body_casts_the_ground_shadow_disc() {
    let mut world = battle_world();
    world.actors[0].move_state.world_z = 0;
    world.actors[0].move_state.world_x = 300;
    let plan = world
        .battle_actor_draw_plan(0, Some(&pose(4000.0)), 4.0, false)
        .expect("party seat");
    assert_eq!(plan.radius, PARTY_BODY_RADIUS);
    let d = legaia_engine_world_views::battle_draws::battle_ground_shadow(&world, 0, &plan)
        .expect("a shadow");
    assert_eq!(d.world_pos, [300.0, -BATTLE_SHADOW_DEPTH_LIFT, 0.0]);
    assert_eq!(d.mesh.indices.len(), 24 * 6);
    let r = d
        .mesh
        .positions
        .iter()
        .map(|p| p[0].hypot(p[2]))
        .fold(0.0f32, f32::max);
    assert!((r - 256.0).abs() < 1.5, "radius {r}");
    let all = legaia_engine_world_views::battle_draws::battle_ground_shadows(&world, |i| {
        world.battle_actor_draw_plan(i, Some(&pose(4000.0)), 4.0, false)
    });
    assert!(!all.is_empty());
    world.actors[0].move_state.render_24 = 0x100;
    assert!(
        legaia_engine_world_views::battle_draws::battle_ground_shadow(&world, 0, &plan).is_none()
    );
}

/// The disc sits under the drawn pose's centroid, turned by the facing,
/// not under the node: a clip whose parts stand 400 units forward of the
/// node moves the shadow with them (`FUN_80048A08`'s `MVMVA` of the pose
/// pool's `+0x120 / +0x124`).
#[test]
fn the_ground_shadow_follows_the_pose_centroid() {
    use legaia_actor_formats::monster_archive::{MonsterAnimation, PartPose};
    let mut world = battle_world();
    world.actors[0].move_state.world_x = 100;
    world.actors[0].move_state.world_z = -700;
    let plan = world
        .battle_actor_draw_plan(0, Some(&pose(4000.0)), 4.0, false)
        .expect("party seat");
    let anim = MonsterAnimation {
        variant_track: Default::default(),
        action_id: 0,
        rate: 1,
        attach_key: 0,
        solo_flag: 0,
        impact_class: 0,
        effect_script: Vec::new(),
        part_count: 2,
        frame_count: 1,
        frames: vec![vec![
            PartPose {
                tz: 400,
                ..PartPose::default()
            };
            2
        ]],
    };
    let a = &mut world.actors[0];
    a.battle_animation = legaia_engine_battle::battle_anim::MonsterAnimPlayer::new(&anim);
    a.pose_frame = Some(a.battle_animation.as_mut().unwrap().seat_cursor(0));
    for (facing, want) in [(0u16, [100.0, -300.0]), (0x800, [100.0, -1100.0])] {
        world.actors[0].battle.facing_angle = facing;
        let d = legaia_engine_world_views::battle_draws::battle_ground_shadow(&world, 0, &plan)
            .expect("a shadow");
        assert!(
            (d.world_pos[0] - want[0]).abs() <= 1.0 && (d.world_pos[2] - want[1]).abs() <= 1.0,
            "facing {facing:#x}: {:?}",
            d.world_pos
        );
    }
}

/// A body the defeat sink raised `+0x42` on keeps only what stands at
/// or above the floor: over its posed mesh the clip is the mesh's own
/// `Y` against the body's height `+0x36`, so a body `183` down (Y-down)
/// keeps nothing above mesh `y = -183`.
#[test]
fn a_sunk_body_is_clipped_at_the_stage_floor() {
    let mut world = World::default();
    world.enter_battle(1, 1);
    assert!(
        legaia_engine_world_views::battle_draws::battle_actor_floor_clip(&world, 1).is_none(),
        "gate down"
    );
    world.actors[1].battle.floor_clip = true;
    world.actors[1].battle.facing_angle = 0x800;
    world.actors[1].move_state.world_x = 675;
    world.actors[1].move_state.world_y = 183;
    world.actors[1].move_state.world_z = 623;
    let c = legaia_engine_world_views::battle_draws::battle_actor_floor_clip(&world, 1)
        .expect("raised");
    assert!(
        c.keeps([30.0, -200.0, -40.0]),
        "a vertex still above the floor"
    );
    assert!(!c.keeps([30.0, -100.0, -40.0]), "a vertex sunk under it");
    assert!(!c.keeps([0.0, 0.0, 0.0]), "the body's origin is under it");
    world.mode = SceneMode::Field;
    assert!(
        legaia_engine_world_views::battle_draws::battle_actor_floor_clip(&world, 1).is_none(),
        "battle only"
    );
}

/// A monster draws the playing stream's channels over its identity table
/// (`FUN_80048A08`'s loop bound is the stream's part count): a 16-object
/// mesh on a 15-part clip leaves its last object undrawn (Koru's staff on
/// his idle), and a clip with as many parts as objects draws them all.
#[test]
fn a_monster_draws_only_the_playing_streams_channels() {
    let mut world = World::default();
    world.enter_battle(1, 1);
    let monster = 1;
    world.actors[monster].battle_monster_id = Some(182);
    let mut clip = pose_test_clip(0, 2, 0);
    clip.variant_track.stream_parts = 15;
    world.actors[monster].battle_animation =
        legaia_engine_battle::battle_anim::MonsterAnimPlayer::new(&clip);
    let drawn = legaia_engine_world_views::battle_draws::battle_drawn_objects(&world, monster, 16)
        .expect("bounded");
    assert_eq!(drawn.len(), 16);
    assert!(drawn[..15].iter().all(|&d| d), "the stream's channels draw");
    assert!(!drawn[15], "the object past them does not");
    assert!(
        legaia_engine_world_views::battle_draws::battle_drawn_objects(&world, monster, 15)
            .is_none(),
        "every object has a channel"
    );
    world.actors[monster].battle_monster_id = None;
    assert!(
        legaia_engine_world_views::battle_draws::battle_drawn_objects(&world, monster, 16)
            .is_none(),
        "only a monster's table is the bare identity"
    );
}

/// A monster ghost is bounded by its historical stream's part count like
/// the live body; a ghost carrying a model-table answer keeps it.
#[test]
fn a_monster_ghost_draws_only_its_streams_channels() {
    use legaia_engine_core::world::BattleGhostDraw;
    let mut clip = pose_test_clip(0, 2, 0);
    clip.variant_track.stream_parts = 15;
    let mut player = legaia_engine_battle::battle_anim::MonsterAnimPlayer::new(&clip).unwrap();
    let pose = player.seat_cursor(0);
    let ghost = |drawn, monster_parts| BattleGhostDraw {
        actor_slot: 1,
        pos: [0; 3],
        pose: pose.clone(),
        color: [0x50, 0x20, 0x20],
        drawn,
        monster_parts,
    };
    let d = ghost(None, Some(15)).drawn_objects(16).expect("bounded");
    assert!(d[..15].iter().all(|&o| o) && !d[15]);
    assert!(ghost(None, Some(15)).drawn_objects(15).is_none());
    assert!(ghost(None, None).drawn_objects(16).is_none());
    assert_eq!(
        ghost(Some(vec![true, false]), None).drawn_objects(16),
        Some(vec![true, false])
    );
}
