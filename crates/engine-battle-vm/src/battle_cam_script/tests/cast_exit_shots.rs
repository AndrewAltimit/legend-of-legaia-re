//! The summon close-up, the battle-over hold, the win pose, the stand-off and the escape shots.

use super::*;

/// `FUN_801DC0A0` case `0x12` against the `puera_summon_mid_cast` capture:
/// the ramp saturated at `0xC8` gives retail's pitch `3696` (`-400`), and an
/// accumulator of `649` lands the eye height on the captured `2066`.
#[test]
fn the_summon_cast_close_up_matches_a_captured_frame() {
    let actor = BattleCamActor {
        facing: 0,
        world: [300.0, -40.0, -800.0],
        height: None,
    };
    let (pose, raw_z) = summon_cast_framing(actor, None, 649, 0xC8);
    // The focus is the body pair `+0x3C` / `+0x40` when one is known.
    let (bodied, _) = summon_cast_framing(actor, Some([123.0, -456.0]), 649, 0xC8);
    assert_eq!(bodied.focus, [123.0, 0.0, -456.0]);
    assert_eq!(pose.pitch, -400.0);
    assert_eq!((pose.pitch as i32).rem_euclid(4096), 3696);
    assert_eq!(pose.tr[1], 2066.0);
    assert_eq!(raw_z, 0x680 - 3 * 649);
    assert_eq!(pose.tr[2], prescale_tr_z(raw_z));
    assert_eq!(pose.yaw, (649 * 2 + 0x500) as f32);
    // The focus is the caster's X/Z at floor height.
    assert_eq!(pose.focus, [300.0, 0.0, -800.0]);
}

/// While the action SM sits in the summon band's `0x33` / `0x34`, the
/// action camera walks to the cast close-up, not case 6's framing.
#[test]
fn the_summon_band_frames_the_cast_close_up() {
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    cam.set_phase(BattleCamPhase::Action);
    cam.observe_action_state(0x32);
    cam.observe_action_state(0x34);
    for f in 1..=200u64 {
        cam.advance_to(f * 2);
    }
    let c = cam.attack.ctx;
    let (want, _) = summon_cast_framing(cam.actor, cam.acting_body, c.accum, c.ramp);
    let got = cam.framing_pose();
    assert_eq!(got.pitch, want.pitch);
    assert!(
        (got.tr[1] - want.tr[1]).abs() <= 32.0,
        "{got:?} vs {want:?}"
    );
}

/// The capture harness's accumulator alignment: the close-up is carried from
/// the accumulator its last camera step ran on to the capture's, a pass
/// restarts there, and a gap wider than a pass is left for the channel to
/// read.
#[test]
fn the_close_up_accumulator_aligns_within_a_pass() {
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    cam.set_phase(BattleCamPhase::Action);
    cam.observe_action_state(0x32);
    cam.observe_action_state(0x34);
    // An odd frame: one display frame past the last camera step.
    for f in 1..=81u64 {
        cam.advance_to(f);
    }
    assert_eq!(cam.frame_accum, 1);
    let have = cam.close_up_accum();
    let before = cam.framing_pose();
    // Outside the tolerance nothing moves.
    assert!(!cam.align_close_up_accum(have + 32));
    assert_eq!(cam.framing_pose(), before);
    assert_eq!(cam.close_up_accum(), have);
    // Two vsyncs back: the pose was stepped on `have - 8`, so it is carried
    // `-8` of accumulator - `2` a unit on the yaw and the eye height.
    assert!(cam.align_close_up_accum(have - 16));
    let after = cam.framing_pose();
    assert_eq!(cam.close_up_accum(), have - 16);
    assert_eq!(cam.frame_accum, 0);
    assert_eq!(after.tr[1], before.tr[1] - 16.0);
    assert_eq!(after.yaw, (before.yaw - 16.0).rem_euclid(4096.0));
    assert_eq!(after.pitch, before.pitch);
    assert_eq!(after.focus, before.focus);
    // Already there: no move.
    assert!(!cam.align_close_up_accum(have - 16));
    // The same accumulator between two camera steps still moves the pose up
    // to the capture's pass boundary.
    cam.advance_to(82);
    let (acc, mid) = (cam.close_up_accum(), cam.framing_pose());
    assert_eq!(cam.frame_accum, 1);
    assert!(cam.align_close_up_accum(acc));
    assert_eq!(cam.framing_pose().tr[1], mid.tr[1] + 16.0);
    // Outside the summon close-up it is inert.
    cam.observe_action_state(0x35);
    assert!(!cam.align_close_up_accum(acc + 8));

    // An early capture, the ramp still under its cap: the pitch is `-2` a
    // unit of ramp and is carried with the rest.
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    cam.set_phase(BattleCamPhase::Action);
    cam.observe_action_state(0x33);
    cam.attack.ctx.accum = 0;
    cam.attack.ctx.ramp = 0;
    cam.advance_to(2);
    cam.advance_to(3);
    let (acc, before) = (cam.close_up_accum(), cam.framing_pose());
    assert_eq!((acc, cam.attack.ctx.ramp, cam.frame_accum), (24, 24, 1));
    assert!(cam.align_close_up_accum(acc));
    assert_eq!(cam.framing_pose().pitch, before.pitch - 16.0);
    assert_eq!(cam.attack.ctx.ramp, 24);
}

/// **The battle-over arm's win-pose script, pinned against retail RAM.**
/// `noa_levelup_banner` is parked in the results hold with Vahn (character
/// `1`, seat 0) posing `0x14`: display trio `(78, -183, -15)`, heading
/// `562`, `ctx[+0x87C] = 616`, `ctx[+0x26E] = 0xC8`. The rotation trio reads
/// pitch `-0x20` and yaw `0x800 - 562 = 1486` exactly, the focus is the
/// display X / Z on the floor, and TR is one tween step short of the arm's
/// target (`(0, 941, 1830)` against `(0, 928, prescale(1126))`), walking
/// down from the stand-off pose the load window left.
#[test]
fn the_battle_over_arm_reproduces_the_results_hold_capture() {
    let vahn = BattleCamActor {
        facing: 562,
        world: [78.0, -183.0, -15.0],
        height: None,
    };
    let f = ActionFraming {
        battle_over: true,
        char_id: 1,
        anim_id: 0x14,
        accum: 616,
        ramp: 0xC8,
        ..Default::default()
    };
    let p = action_framing(vahn, f);
    assert_eq!(p.pitch, -32.0);
    assert_eq!(p.yaw, 1486.0);
    // TR.y = -5 * -183 + (616 >> 3) - 0x40; TR.z = 0x500 - (616 >> 2).
    assert_eq!(p.tr, [0.0, 928.0, prescale_tr_z(1126)]);
    assert_eq!(f.raw_z(), 1126);
    assert_eq!(p.focus, [78.0, 0.0, -15.0], "display X / Z on the floor");
    // The capture is mid-chase toward that target, from above on both.
    assert!(941.0 > p.tr[1] && 1830.0 > p.tr[2]);
}

/// The script's dispatch edges: character `4`'s fixed arm, an unknown
/// character or a pose outside `0x11..=0x18` editing nothing, and the one
/// pose that skips the height floor.
#[test]
fn the_win_pose_script_dispatch() {
    let ground = BattleCamActor {
        facing: 0,
        world: [0.0, 0.0, 0.0],
        height: None,
    };
    let over = ActionFraming {
        battle_over: true,
        accum: 800,
        ramp: 0x40,
        ..Default::default()
    };
    // Character 4: pitch 0x80 then the floor's quarter-shortfall on top.
    let terra = battle_over_slots(ground, ActionFraming { char_id: 4, ..over });
    assert_eq!(terra.tr_z, 100 + 0x680);
    assert_eq!(terra.pitch, 0x80 + 0x280 / 4);
    assert_eq!(terra.tr_y, 0x280);
    // Character 0 / out-of-range pose: the base and the floor only.
    for f in [
        ActionFraming { char_id: 0, ..over },
        ActionFraming {
            char_id: 1,
            anim_id: 0x19,
            ..over
        },
    ] {
        let s = battle_over_slots(ground, f);
        assert_eq!((s.tr_y, s.tr_z, s.pitch), (0x280, 0x500, 0xA0), "{f:?}");
    }
    // Character 2 posing 0x16 keeps a TR.y under the floor.
    let noa = battle_over_slots(
        ground,
        ActionFraming {
            char_id: 2,
            anim_id: 0x16,
            ..over
        },
    );
    assert_eq!(noa.tr_y, (800 >> 3) - 0xC0, "no floor");
    assert_eq!(noa.pitch, 0x100);
    // Tables 2 and 3 alias their last entry to arm 1; table 1 aliases 6/7
    // to 0/1.
    let at = |c: u8, a: u8| {
        battle_over_slots(
            ground,
            ActionFraming {
                char_id: c,
                anim_id: a,
                ..over
            },
        )
    };
    assert_eq!(at(1, 0x17), at(1, 0x11));
    assert_eq!(at(1, 0x18), at(1, 0x12));
    assert_eq!(at(2, 0x18), at(2, 0x12));
    assert_eq!(at(3, 0x18), at(3, 0x12));
}

/// Case 8's **stand-off** arm: a dead target whose node is gone frames the
/// acting actor alone at `TR (0, 0x400, radius * 5 / 2)`, level, yaw from
/// the target's heading plus the live ladder.
#[test]
fn a_gone_target_takes_the_stand_off_arm() {
    let mut cam = BattleCamera::new(BattleCamPhase::ActionEnd, 0);
    cam.set_actor(BattleCamActor {
        facing: 562,
        world: [78.0, -183.0, -15.0],
        height: None,
    });
    cam.action_yaw = 0x10;
    cam.attack.ctx.phase_cursor = 1;
    cam.target = Some(PostActionTarget {
        world: [600.0, 0.0, 800.0],
        live: false,
        facing: (562 + 0x800) & 0xFFF,
        node_gone: true,
        ..PostActionTarget::default()
    });
    let (pose, raw_z) = cam.action_end_pose();
    assert_eq!(raw_z, PARTY_BODY_RADIUS * 5 / 2);
    assert_eq!(pose.tr, [0.0, 1024.0, prescale_tr_z(1600)]);
    assert_eq!(pose.pitch, 0.0);
    assert_eq!(pose.focus, [78.0, 0.0, -15.0], "the actor, not the corpse");
    let yaw: i32 = -((562 + 0x800) & 0xFFF) - ((1 << 9) - 0x100) + 0x10;
    assert_eq!(pose.yaw, yaw.rem_euclid(4096) as f32);
    assert_eq!(cam.action_yaw, 0x10, "the stand-off arm keeps the ladder");
}

/// The lone-monster defeat bypass (`0x801D6AC8..0x801D6AF0`) takes the same
/// stand-off arm with the node still drawn.
#[test]
fn a_lone_scripted_monster_dying_in_place_takes_the_stand_off_arm() {
    let mk = |node_gone: bool, lone_defeat: bool| {
        let mut cam = BattleCamera::new(BattleCamPhase::ActionEnd, 0);
        cam.set_actor(BattleCamActor {
            facing: 562,
            world: [78.0, -183.0, -15.0],
            height: None,
        });
        cam.target = Some(PostActionTarget {
            world: [600.0, 0.0, 800.0],
            live: false,
            facing: 7,
            node_gone,
            lone_defeat,
            ..PostActionTarget::default()
        });
        cam.action_end_pose()
    };
    assert_eq!(mk(false, true), mk(true, false));
    assert_ne!(mk(false, true), mk(false, false));
}

#[test]
fn escape_shot_cuts_to_the_reverse_angle_and_holds_it() {
    let mut cam = traced_cam(BattleCamPhase::Menu);
    cam.arm_escape_shot();
    assert_eq!(cam.pose().yaw, ESCAPE_SNAP_YAW, "the snap lands at once");
    // 0x30 display frames of tween, then the landed pose holds through any
    // state the run band and the teardown pass through.
    for f in 1..=0x30u64 {
        cam.advance_to(f);
    }
    let landed = cam.pose();
    assert_eq!(landed.yaw, 2048.0);
    assert_eq!(landed.tr, [0.0, 1536.0, 0.0]);
    assert_eq!(landed.focus, [0.0, 0.0, 1024.0]);
    for (i, state) in [0x65u8, 0x66, 0x50, 0x00].into_iter().enumerate() {
        cam.observe_action_state(state);
        cam.advance_to(0x30 + 2 * (i as u64 + 1));
    }
    assert_eq!(
        cam.pose().yaw,
        2048.0,
        "no framing case takes the camera back"
    );
    assert_eq!(cam.pose().focus, landed.focus);
}

/// The magic exit's camera write (`0x801E4958..0x801E497C`): on the
/// `0x2E -> 0x50` edge a pitch at or above `0x191` snaps to pitch `0`, eye Y
/// `0x500`; a lower pitch, or any other edge, leaves the framing alone.
#[test]
fn magic_exit_edge_snaps_a_steep_pitch() {
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    cam.observe_action_state(0x2E);
    cam.pose.pitch = 0x191 as f32;
    cam.pose.tr[1] = 300.0;
    cam.observe_action_state(0x50);
    assert_eq!((cam.pose.pitch, cam.pose.tr[1]), (0.0, 1280.0));

    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    cam.observe_action_state(0x2E);
    cam.pose.pitch = 0x190 as f32;
    cam.pose.tr[1] = 300.0;
    cam.observe_action_state(0x50);
    assert_eq!((cam.pose.pitch, cam.pose.tr[1]), (400.0, 300.0));

    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    cam.observe_action_state(0x51);
    cam.pose.pitch = 0x400 as f32;
    cam.observe_action_state(0x50);
    assert_eq!(cam.pose.pitch, 1024.0);
}

/// A granted flee's wait (`0x65`) backs the eye off `32 * step` a pass
/// (`0x801E59A4..0x801E59BC`) once the escape shot owns the camera.
#[test]
fn escape_wait_backs_the_eye_away() {
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    for f in 1..=40u64 {
        cam.advance_to(f * 2);
    }
    cam.arm_escape_shot();
    cam.observe_action_state(0x64);
    for f in 41..=200u64 {
        cam.advance_to(f * 2);
    }
    let settled = cam.pose.tr[2];
    cam.observe_action_state(0x65);
    cam.advance_to(402);
    assert_eq!(cam.pose.tr[2], settled - 64.0);
}

/// A module drift under a shot in flight, against the walker `FUN_8002149C`:
/// a component the shot moves lands on the shot's endpoint whatever drifted
/// into it meanwhile (the walk stores the endpoint once reached), while a
/// component the shot leaves alone (increment `0`, skipped by the walker)
/// keeps every drift. Once the shot has landed every drift sticks.
#[test]
fn a_drift_under_a_moving_shot_is_spent_and_kept_on_a_still_one() {
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    cam.pose = BattleCamPose {
        pitch: 0.0,
        yaw: 0.0,
        tr: [0.0, 1536.0, 5000.0],
        focus: [0.0, 0.0, 0.0],
    };
    let shot = ModuleShot {
        angles: [0, 0, 0],
        tr: [0, 0x600, 0x800],
        focus: [0, 0, 0],
        frames: 0x20,
    };
    let (target, raw_z) = shot.pose();
    cam.arm_module_shot(target, raw_z, u32::from(shot.frames));
    let mut drifted = 0.0;
    while !cam.module_glide.is_some_and(|g| g.landed()) {
        cam.drift_module(0, 0, 4, 4);
        drifted += 4.0;
        cam.step_module_shot();
    }
    assert_eq!(cam.pose.tr[2], target.tr[2], "TR z lands on the endpoint");
    assert_eq!(cam.pose.tr[1], 1536.0 + drifted, "TR y keeps its drift");
    cam.drift_module(0, 0, 0, 4);
    cam.step_module_shot();
    assert_eq!(
        cam.pose.tr[2],
        target.tr[2] + 4.0,
        "a landed shot keeps a drift"
    );
}

/// A module shot walks retail's builder increments: `ceil(|gap| / frames)`
/// a vsync, times the pass's frame step, each component landing on its own
/// (`FUN_801D829C` / `FUN_8002149C`). PROT 0905's arm-1 shot from the arm-0
/// cut (pitch `0`, yaw `0x60C`, TR `(0, 0x5E0, 0x800)`) onto pitch `-0x60`,
/// yaw `0x9F4`, TR `(0, 0x580, 0x80)` over `0x60` frames: a PCSX-Redux cast
/// steps yaw `22`, pitch `2`, TR y `2` and TR z `64` a step-`2` pass, so
/// the yaw (`1000` units) lands on the `46`th pass, ahead of the rest.
#[test]
fn a_module_shot_walks_the_builders_per_vsync_increments() {
    let mut cam = BattleCamera::new(BattleCamPhase::Action, 0);
    let cut = ModuleShot {
        angles: [0, 0x60C, 0],
        tr: [0, 0x5E0, 0x800],
        focus: [0, 0, 0],
        frames: 1,
    };
    let (pose, raw_z) = cut.pose();
    cam.arm_module_shot(pose, raw_z, u32::from(cut.frames));
    assert_eq!(cam.pose.yaw, 1548.0);
    let shot = ModuleShot {
        angles: [-0x60, 0x9F4, 0],
        tr: [0, 0x580, 0x80],
        focus: [0, 0, 0],
        frames: 0x60,
    };
    let (target, raw_z) = shot.pose();
    cam.arm_module_shot(target, raw_z, u32::from(shot.frames));
    let start = cam.pose;
    cam.step_module_shot();
    assert_eq!(cam.pose.yaw - start.yaw, 22.0);
    assert_eq!(cam.pose.pitch - start.pitch, -2.0);
    assert_eq!(cam.pose.tr[1] - start.tr[1], -2.0);
    assert_eq!(cam.pose.tr[2] - start.tr[2], -64.0);
    for _ in 1..45 {
        cam.step_module_shot();
    }
    assert_eq!(
        cam.pose.yaw,
        1548.0 + 45.0 * 22.0,
        "45 passes in, still walking"
    );
    cam.step_module_shot();
    assert_eq!(cam.pose.yaw, 2548.0, "the yaw lands on pass 46");
    assert!(cam.pose.pitch > -96.0, "the pitch has not");
    assert!(!cam.module_glide.is_some_and(|g| g.landed()));
    for _ in 0..2 {
        cam.step_module_shot();
    }
    assert_eq!(cam.pose.pitch, -96.0);
    assert!(
        cam.module_glide.is_some_and(|g| g.landed()),
        "every component home"
    );
}
