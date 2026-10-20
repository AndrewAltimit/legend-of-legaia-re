//! The entry sweep, the dialogue close-up, and the glide / orbit stepping.

use super::*;

/// The battle-entry sweep against the sparring fight's two entry captures:
/// `v0_1_battle_loading_tetsu` at counter `0x84` (pitch `60`, TR
/// `(0, 1472, 6912)`) and `s5_tetsu_battle` at `0xAF` (pitch `16`, TR
/// `(0, 2010, 3552)` - taken across one dropped frame, so the port's
/// two-frame steps straddle it), then the cut to the dialogue close-up.
#[test]
fn the_entry_sweep_drifts_then_eases_onto_case_two_then_cuts_to_the_dialogue() {
    let mut cam = BattleCamera::new(BattleCamPhase::Dialogue, 0);
    cam.start_entry_sweep();
    assert_eq!(cam.entry_sweep_counter(), Some(ENTRY_COUNTER_START));
    while cam.entry_sweep_counter().is_some_and(|c| c < 0x84) {
        steps(&mut cam, 1);
    }
    let p = cam.pose();
    assert_eq!((p.pitch, p.tr), (60.0, [0.0, 1472.0, 6912.0]));
    while cam.entry_sweep_counter().is_some_and(|c| c < 0xAF) {
        steps(&mut cam, 1);
    }
    let p = cam.pose();
    assert!((p.pitch - 16.0).abs() <= 8.0, "pitch {}", p.pitch);
    assert!((p.tr[1] - 2010.0).abs() <= 120.0, "TR y {}", p.tr[1]);
    assert!((p.tr[2] - 3552.0).abs() <= 150.0, "TR z {}", p.tr[2]);
    assert_eq!(p.focus, [0.0; 3], "case 2 frames the origin");
    while cam.entry_sweep_counter().is_some() {
        steps(&mut cam, 1);
    }
    assert_eq!(cam.pose(), dialogue_pose(None), "the tutorial cuts in");
    // A fight without the sweep opens on its framing, as before.
    let cam = BattleCamera::new(BattleCamPhase::Dialogue, 0);
    assert_eq!(cam.entry_sweep_counter(), None);
}

/// Retail opens no surface and runs no action under the sweep; the port's
/// battle tick can, and the first one ends the sweep.
#[test]
fn an_action_opened_under_the_entry_sweep_ends_it() {
    let mut cam = traced_cam(BattleCamPhase::Menu);
    cam.start_entry_sweep();
    steps(&mut cam, 4);
    assert!(cam.entry_sweep_counter().is_some());
    cam.set_phase(BattleCamPhase::Action);
    steps(&mut cam, 1);
    assert_eq!(cam.entry_sweep_counter(), None);
    assert_eq!(cam.phase(), BattleCamPhase::Action);
}

/// Battle entry on tutorial dialogue: the measured held close-up, static
/// over any number of frames.
#[test]
fn dialogue_close_up_holds_static() {
    let mut cam = BattleCamera::new(BattleCamPhase::Dialogue, 0);
    steps(&mut cam, 120);
    assert_eq!(cam.pose(), dialogue_pose(None));
    assert_eq!(cam.pose().focus, [0.0, 0.0, 800.0], "Tetsu's seat");
    // With a formation the focus is the monster row's centre.
    let cam = BattleCamera::new_with_formation(
        BattleCamPhase::Dialogue,
        Some(FormationBox {
            min: [-600.0, -812.0],
            max: [600.0, 813.0],
        }),
        0.0,
        0,
    );
    assert_eq!(cam.pose().focus, [0.0, 0.0, 813.0]);
    assert_eq!(cam.pose().tr, DIALOGUE_POSE.tr);
}

/// Dialogue dismiss reproduces the traced glide: pitch +6/step to 32,
/// TR.z +864/step to 7680, yaw resuming the -4/step orbit from 0
/// (trace frames 45..57).
#[test]
fn dialogue_dismiss_glide_matches_trace() {
    let mut cam = traced_cam(BattleCamPhase::Dialogue);
    cam.set_phase(BattleCamPhase::Menu);
    // Traced (pitch, yaw, z) per step; yaw 0 on the first step (the
    // orbit decrement lands from the second entry on).
    let want = [
        (6.0, 4092.0, 2502.0),
        (12.0, 4088.0, 3366.0),
        (18.0, 4084.0, 4230.0),
        (24.0, 4080.0, 5094.0),
        (30.0, 4076.0, 5958.0),
        (32.0, 4072.0, 6822.0),
        (32.0, 4068.0, 7680.0),
    ];
    for (i, (p, y, z)) in want.into_iter().enumerate() {
        steps(&mut cam, 1);
        let pose = cam.pose();
        assert_eq!((pose.pitch, pose.tr[2]), (p, z), "step {i}");
        assert_eq!(pose.yaw, y, "yaw step {i}");
    }
    // Settled: pure idle orbit thereafter.
    steps(&mut cam, 1);
    assert_eq!(cam.pose().tr, traced_menu_tr());
    assert_eq!(cam.pose().yaw, 4064.0);
}

/// Menu idle orbit: -4 yaw units per step, framing held.
#[test]
fn menu_idle_orbit_rate() {
    let mut cam = traced_cam(BattleCamPhase::Menu);
    steps(&mut cam, 10);
    assert_eq!(cam.pose().yaw, (0.0f32 - 40.0).rem_euclid(4096.0));
    assert_eq!(cam.pose().pitch, MENU_PITCH);
    assert_eq!(cam.pose().tr, traced_menu_tr());
}

/// Submenu open glides every component to the measured close-up in 6
/// steps (shortest-arc yaw) and then holds it with the orbit paused.
#[test]
fn submenu_glide_arrives_in_six_steps_and_holds() {
    let mut cam = traced_cam(BattleCamPhase::Menu);
    // Orbit a while first (trace picks up the glide from yaw ~4024).
    steps(&mut cam, 18);
    cam.set_phase(BattleCamPhase::Submenu);
    steps(&mut cam, 5);
    assert_ne!(
        cam.pose().tr,
        BattleCamActor::default().submenu_pose().tr,
        "still mid-glide"
    );
    steps(&mut cam, 1);
    let pose = cam.pose();
    assert_eq!(pose.pitch, BattleCamActor::default().submenu_pose().pitch);
    assert_eq!(pose.tr, BattleCamActor::default().submenu_pose().tr);
    assert_eq!(
        pose.yaw.rem_euclid(4096.0),
        BattleCamActor::default().submenu_pose().yaw
    );
    // Held static while the submenu stays open.
    steps(&mut cam, 30);
    assert_eq!(cam.pose(), pose);
}

/// Submenu exit passes through the measured swing pose (6 steps), then
/// returns to the menu framing (7 steps) with the orbit running again.
#[test]
fn submenu_exit_swings_out_then_returns() {
    let mut cam = traced_cam(BattleCamPhase::Menu);
    cam.set_phase(BattleCamPhase::Submenu);
    steps(&mut cam, 6);
    cam.set_phase(BattleCamPhase::Menu);
    steps(&mut cam, 6);
    let swing = cam.pose();
    assert_eq!(swing.pitch, SWING_POSE.pitch);
    assert_eq!(swing.tr, SWING_POSE.tr);
    assert_eq!(swing.yaw, 0.0, "swing lands on yaw 4096 = 0");
    steps(&mut cam, 7);
    let back = cam.pose();
    assert_eq!(back.pitch, MENU_PITCH);
    assert_eq!(back.tr, traced_menu_tr());
    // Orbit ran through the 7 return steps: yaw 0 -> -28 (mod 4096).
    assert_eq!(back.yaw, 4096.0 - 28.0);
    // And keeps orbiting.
    steps(&mut cam, 1);
    assert_eq!(cam.pose().yaw, 4096.0 - 32.0);
}

/// The glide steps on `FUN_801D829C`'s **integer** per-frame increments,
/// not on an exact float divide. Retail's `ceil(|delta| / duration)`
/// overshoots slightly and clamps at the endpoint; a float rate would land
/// a fraction short on the same step. Pick a height delta the step count
/// does not divide and the two laws separate on the very first step.
#[test]
fn glide_rates_are_the_retail_ceiling_increments() {
    let mut cam = traced_cam(BattleCamPhase::Menu);
    // TR.y runs 1280 -> 1401: delta 121 over 6 steps. ceil = 21/step; an
    // exact divide would be 20.1667.
    cam.set_actor(BattleCamActor {
        facing: 0,
        height: Some(1401.0),
        world: [0.0, 0.0, -800.0],
    });
    cam.set_phase(BattleCamPhase::Submenu);
    steps(&mut cam, 1);
    assert_eq!(cam.pose().tr[1], 1280.0 + 21.0);
    steps(&mut cam, 4);
    assert_eq!(cam.pose().tr[1], 1280.0 + 105.0);
    // The last step clamps rather than overshooting to 1406.
    steps(&mut cam, 1);
    assert_eq!(cam.pose().tr[1], 1401.0);
}

/// The builder owns the TR.z projection prescale, so a glide handed the
/// raw world Z converges on exactly the value the framing cases publish.
#[test]
fn glide_endpoints_agree_with_the_framing_cases() {
    let mut cam = traced_cam(BattleCamPhase::Menu);
    cam.set_phase(BattleCamPhase::Submenu);
    steps(&mut cam, SUBMENU_ENTER_STEPS as u64);
    assert_eq!(
        cam.pose().tr[2],
        BattleCamActor::default().submenu_pose().tr[2]
    );
    cam.set_phase(BattleCamPhase::Menu);
    steps(&mut cam, (SUBMENU_SWING_STEPS + SWING_RETURN_STEPS) as u64);
    assert_eq!(cam.pose().tr[2], traced_menu_tr()[2]);
    // And the raw-Z helper is what `menu_framing` prescales.
    assert_eq!(
        prescale_tr_z(menu_raw_z(Some(traced_formation()))),
        traced_menu_tr()[2]
    );
}

/// The shortest-arc unwrap goes the short way in both directions.
#[test]
fn submenu_yaw_takes_shortest_arc() {
    // From yaw 800 the short way to 2288 is +1488 (forward).
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    cam.pose.yaw = 800.0;
    cam.set_phase(BattleCamPhase::Submenu);
    steps(&mut cam, 1);
    assert!(cam.pose().yaw > 800.0);
    // From yaw 3500 the short way to 2288 is -1212 (backward).
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    cam.pose.yaw = 3500.0;
    cam.set_phase(BattleCamPhase::Submenu);
    steps(&mut cam, 1);
    assert!(cam.pose().yaw < 3500.0);
    steps(&mut cam, 5);
    assert_eq!(cam.pose().yaw, BattleCamActor::default().submenu_pose().yaw);
}

/// The capture harness's orbit alignment moves the free-running azimuth
/// only where the orbit owns it, and the orbit keeps running from there.
#[test]
fn orbit_alignment_applies_only_to_the_idle_orbit() {
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    assert!(cam.align_orbit_yaw(4064.0 + 4096.0));
    assert_eq!(cam.pose().yaw, 4064.0);
    steps(&mut cam, 1);
    assert_eq!(cam.pose().yaw, 4064.0 - ORBIT_STEP);
    // The submenu close-up owns its own yaw: no alignment.
    cam.set_phase(BattleCamPhase::Submenu);
    let before = cam.pose().yaw;
    assert!(!cam.align_orbit_yaw(100.0));
    assert_eq!(cam.pose().yaw, before);
}

/// The capture harness's glide-origin alignment takes retail's live value
/// only for the components whose endpoint the engine agrees on, never in
/// the far framing's orbit phase, and lands the yaw on the representative
/// nearest the engine's own unwrap.
#[test]
fn glide_origin_alignment_takes_only_agreeing_components() {
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    let pose = cam.pose();
    assert_eq!(cam.align_glide_origin(pose, pose), 0, "the orbit phase");
    cam.set_phase(BattleCamPhase::Action);
    steps(&mut cam, 200);
    assert!(!cam.is_gliding());
    // Landed: its endpoint is its pose.
    let at = cam.pose();
    let mut end = at;
    end.focus[0] += 100.0; // a framing the engine does not head for
    let live = BattleCamPose {
        pitch: 77.0,
        yaw: (at.yaw + 4096.0 + 30.0).rem_euclid(4096.0),
        tr: [1.0, 2.0, 3.0],
        focus: [4.0, 5.0, 6.0],
    };
    let mask = cam.align_glide_origin(live, end);
    assert_eq!(mask, !0x20u8, "everything but focus.x");
    let got = cam.pose();
    assert_eq!(got.pitch, 77.0);
    assert!(
        (got.yaw - (at.yaw + 30.0)).abs() < 1e-3,
        "{} vs {}",
        got.yaw,
        at.yaw
    );
    assert_eq!(got.tr, [1.0, 2.0, 3.0]);
    assert_eq!(got.focus, [at.focus[0], 5.0, 6.0]);
}

/// The endpoint test that decides whether a Done-band capture takes the
/// displayed pose: every component of the glide's target within the
/// alignment tolerances, never in the orbit phase.
#[test]
fn glide_endpoint_match_needs_every_component() {
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    assert!(!cam.glide_endpoint_matches(cam.pose()), "the orbit phase");
    cam.set_phase(BattleCamPhase::Action);
    steps(&mut cam, 1);
    let target = cam.glide_target();
    assert!(cam.glide_endpoint_matches(target));
    let mut end = target;
    end.yaw += 1.0;
    end.focus[2] += 7.0;
    assert!(cam.glide_endpoint_matches(end), "within the tolerances");
    for k in 0..8 {
        let mut off = target;
        match k {
            0 => off.pitch += 3.0,
            1 => off.yaw += 39.0,
            2..=4 => off.tr[k - 2] += 3.0,
            _ => off.focus[k - 5] += 9.0,
        }
        assert!(!cam.glide_endpoint_matches(off), "component {k}");
    }
}

/// With the yaw on retail's endpoint, a focus endpoint a few units off
/// takes retail's point in the ease: the engine's focus stands as far short
/// of its own endpoint as retail's live focus stood from its. An endpoint
/// off the same bodies' band, or a landed glide, keeps the engine's own.
#[test]
fn glide_origin_alignment_carries_the_focus_phase_under_an_exact_yaw() {
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    cam.set_phase(BattleCamPhase::Action);
    steps(&mut cam, 1);
    assert!(cam.is_gliding());
    let target = cam.glide_target();
    let mut end = target;
    end.focus[0] -= 16.0;
    end.focus[2] -= 17.0;
    let mut live = end;
    live.yaw = (end.yaw + 881.0).rem_euclid(4096.0);
    live.focus[0] = end.focus[0] - 110.0;
    live.focus[2] = end.focus[2] - 122.0;
    let mask = cam.align_glide_origin(live, end);
    assert_eq!(mask & 0xA0, 0xA0, "focus x and z carried");
    let got = cam.pose();
    assert_eq!(got.focus[0], target.focus[0] - 110.0);
    assert_eq!(got.focus[2], target.focus[2] - 122.0);

    // A yaw endpoint off by more than the tolerance leaves the focus alone.
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    cam.set_phase(BattleCamPhase::Action);
    steps(&mut cam, 1);
    let before = cam.pose();
    let mut off = end;
    off.yaw = (cam.glide_target().yaw + 30.0).rem_euclid(4096.0);
    let mask = cam.align_glide_origin(live, off);
    assert_eq!(mask & 0xA0, 0);
    assert_eq!(cam.pose().focus, before.focus);
}

/// **The resting yaw is the free-running orbit, not a captured
/// constant.** Five retail battle save states caught at the identical
/// framing - `ctx[7] == 0x00`, pitch `32`, `TR (0, 1280, 7680)`, focus at
/// the origin, `+-800` seats - read the yaws `224`, `2632`, `3136`,
/// `3808` and `3882`. A battle inherits `_DAT_8007B792` from the field
/// camera (one shared rotation trio) and the action SM only decrements
/// it, so no single sample is "the" resting yaw.
///
/// What must not survive is `0`: at yaw `0` the eye looks straight down
/// the seat axis and both rows project to the same screen X.
#[test]
fn the_entry_yaw_is_inherited_not_zero() {
    let formation = FormationBox {
        min: [0.0, -800.0],
        max: [0.0, 800.0],
    };
    for seed in [224.0f32, 2632.0, 3136.0, 3808.0, 3882.0] {
        let cam = BattleCamera::new_with_formation(BattleCamPhase::Menu, Some(formation), seed, 0);
        assert_eq!(cam.framing_pose().yaw, seed);
        // The pinned depth law is untouched by the seed.
        assert_eq!(cam.framing_pose().tr[2], prescale_tr_z(4800));
    }
    // Out-of-range seeds wrap into the 12-bit orbit domain.
    assert_eq!(
        BattleCamera::new_with_formation(BattleCamPhase::Menu, None, 4096.0 + 100.0, 0)
            .framing_pose()
            .yaw,
        100.0
    );
}

/// **The far framing is re-derived against the LIVE formation.**
///
/// Retail re-arms case 9 from its own callers every pass, so the depth
/// and the bbox centre track the actors. A port that armed one glide and
/// kept its target froze `max(span * 3, 0x800)` at whatever the formation
/// was mid-approach - the `0x800` floor - and never recovered when the
/// attacker walked back to its seat.
#[test]
fn the_far_framing_follows_the_formation_after_it_reopens() {
    let closed = FormationBox {
        min: [0.0, -800.0],
        max: [0.0, -700.0],
    };
    let open = FormationBox {
        min: [0.0, -800.0],
        max: [0.0, 800.0],
    };
    let inputs = |f: FormationBox| BattleCamInputs {
        phase: BattleCamPhase::Menu,
        formation: Some(f),
        ..Default::default()
    };
    let mut slot = None;
    // Enter with the formation collapsed: the depth clamps at the floor.
    for i in 0..20u64 {
        drive(&mut slot, true, inputs(closed), i * 2, None);
    }
    assert_eq!(
        slot.as_ref().unwrap().framing_pose().tr[2],
        prescale_tr_z(MENU_TR_Z_MIN_RAW as i32),
    );
    // The attacker returns to its seat; the framing must re-open with it.
    for i in 20..60u64 {
        drive(&mut slot, true, inputs(open), i * 2, None);
    }
    let p = slot.as_ref().unwrap().framing_pose();
    assert_eq!(p.tr[2], prescale_tr_z(4800), "span 1600 * 3, prescaled");
    assert_eq!(
        p.focus,
        [0.0, 0.0, 0.0],
        "bbox centre of the re-opened pair"
    );
}

/// `sync_step_phase`: a tick that opens a battle frame steps the camera, the
/// tick inside the frame does not.
#[test]
fn a_synced_camera_steps_on_the_tick_that_opens_a_battle_frame() {
    let mut cam = BattleCamera::new(BattleCamPhase::Dialogue, 0);
    cam.start_entry_sweep();
    let c0 = cam.entry_sweep_counter();
    cam.sync_step_phase(1);
    cam.advance_to(1);
    assert_eq!(cam.entry_sweep_counter(), c0, "mid-frame: no step");
    cam.sync_step_phase(0);
    cam.advance_to(2);
    assert_ne!(cam.entry_sweep_counter(), c0, "frame opens: one step");
}
