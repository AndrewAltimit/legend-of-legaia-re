use super::*;

#[test]
fn view_depth_reproduces_a_captured_battle_frame() {
    // party_battle_gobu_gobu: camera (32, 2828), tr (0, 1280, 7800),
    // focus origin; two bodies' stored view depths (the other two sit
    // about 20 units off, read as moved after the frame drew them).
    let pose = BattleCamPose {
        pitch: 32.0,
        yaw: 2828.0,
        tr: [0.0, 1280.0, 7800.0],
        focus: [0.0; 3],
    };
    for (pos, z) in [
        ([0.0, 0.0, -812.0], 8986.0),
        ([600.0, 0.0, -762.0], 11143.0),
    ] {
        let got = battle_view_depth(&pose, 4.0, pos);
        assert!((got - z).abs() <= 2.0, "{pos:?}: {got} vs {z}");
    }
}

/// The formation behind the traced Tetsu fight. The trace pins the far
/// framing's TR.z at `7680` = `prescale(0x12C0)`, and case 9 builds that
/// raw `0x12C0` as `span * 3`, so the traced formation spanned `1600`
/// world units. Every trace-pinned menu assertion below is stated
/// against this formation - which is the point: the law reproduces the
/// measurement instead of hardcoding it.
fn traced_formation() -> FormationBox {
    FormationBox {
        min: [-800.0, -800.0],
        max: [800.0, 800.0],
    }
}

/// The traced far framing, reproduced by the case-9 law.
fn traced_menu_tr() -> [f32; 3] {
    menu_framing(Some(traced_formation()), 0.0).tr
}

/// A camera armed on the traced formation.
fn traced_cam(phase: BattleCamPhase) -> BattleCamera {
    let mut cam = BattleCamera::new(phase, 0);
    cam.set_formation(Some(traced_formation()));
    // Re-snap: `new` built the entry pose before the formation landed.
    if phase == BattleCamPhase::Menu {
        cam.pose = cam.menu_pose();
    }
    cam
}

/// The originally measured solo-Vahn framing must fall out of the
/// formula, not be hardcoded: `yaw 2288 / TR (-512, 1152, 2457)`.
#[test]
fn default_actor_reproduces_the_measured_vahn_framing() {
    let p = BattleCamActor::default().submenu_pose();
    assert_eq!(p.pitch, 32.0);
    assert_eq!(p.yaw, 2288.0, "0x8F0 - facing 0");
    assert_eq!(p.tr, [-512.0, 1152.0, 2457.0]);
}

/// `2288` is `0x8F0` minus the actor's facing - a fixed
/// over-the-shoulder offset, so any seat's framing follows its facing.
#[test]
fn submenu_yaw_tracks_actor_facing() {
    let at = |facing| {
        BattleCamActor {
            facing,
            ..Default::default()
        }
        .submenu_pose()
        .yaw
    };
    assert_eq!(at(0), 2288.0);
    assert_eq!(at(1024), 1264.0, "quarter turn right");
    assert_eq!(at(2288), 0.0, "actor facing the base angle");
    // Wraps into [0, 4096) rather than going negative.
    assert_eq!(at(3000), (0x8F0 - 3000 + 4096) as f32);
    assert_eq!(at(4096), 2288.0, "full turn is identity");
}

/// TR.x / TR.z / pitch are seat- and character-invariant constants: only
/// TR.y (the disc table) and the yaw/focus vary.
#[test]
fn submenu_constants_do_not_vary_by_actor() {
    let heights = [1152.0f32, 960.0, 1408.0, 512.0];
    for facing in [0, 700, 2048, 4095] {
        for height in heights {
            let p = BattleCamActor {
                facing,
                height: Some(height),
                world: [1.0, 2.0, 3.0],
            }
            .submenu_pose();
            assert_eq!(p.pitch, 32.0);
            assert_eq!(p.tr[0], -512.0);
            assert_eq!(p.tr[2], 2457.0);
            assert_eq!(p.tr[1], height, "TR.y is the per-character table");
        }
    }
}

/// The prescale truncates - `0x600` lands on 2457, not 2458.
#[test]
fn tr_z_prescale_truncates() {
    assert_eq!(prescale_tr_z(0x600), 2457.0);
    // The other traced framings fall out of the same divide.
    assert_eq!(prescale_tr_z(0x400), 1638.0);
    assert_eq!(prescale_tr_z(0x800), 3276.0);
}

/// The per-seat half of the framing: the focus is the acting actor's own
/// position, so the camera orbits about whoever is acting.
#[test]
fn focus_is_the_acting_actor_position() {
    let a = BattleCamActor {
        facing: 0,
        height: None,
        world: [640.0, -128.0, -800.0],
    };
    assert_eq!(a.submenu_pose().focus, [640.0, -128.0, -800.0]);
    // Two seats at different positions frame differently even though
    // their rotation + translation trios agree - which is exactly why a
    // solo trace could not tell the focus from a constant.
    let b = BattleCamActor {
        world: [-640.0, -128.0, -800.0],
        ..a
    };
    assert_eq!(a.submenu_pose().tr, b.submenu_pose().tr);
    assert_eq!(a.submenu_pose().yaw, b.submenu_pose().yaw);
    assert_ne!(a.submenu_pose().focus, b.submenu_pose().focus);
}

/// Retargeting the camera at a different seat moves the glide target.
#[test]
fn set_actor_retargets_the_submenu_glide() {
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    cam.set_actor(BattleCamActor {
        facing: 1024,
        height: Some(960.0),
        world: [640.0, 0.0, -800.0],
    });
    cam.set_phase(BattleCamPhase::Submenu);
    steps(&mut cam, 2 * SUBMENU_ENTER_STEPS as u64);
    assert_eq!(cam.pose().yaw.rem_euclid(4096.0), 1264.0);
    // Height comes from the disc table, and the focus followed the seat.
    assert_eq!(cam.pose().tr[1], 960.0);
    assert_eq!(cam.pose().focus, [640.0, 0.0, -800.0]);
}

/// The traced far framing falls out of the case-9 formation law rather
/// than being a constant: a 1600-unit span reproduces `TR.z = 7680`
/// (`prescale(0x12C0)`), and the focus lands on the formation centre.
#[test]
fn menu_framing_reproduces_the_traced_depth_from_the_formation() {
    let p = menu_framing(Some(traced_formation()), 0.0);
    assert_eq!(p.pitch, MENU_PITCH);
    assert_eq!(p.tr, [0.0, 1280.0, 7680.0]);
    assert_eq!(p.focus, [0.0; 3], "symmetric formation centres on origin");
}

/// A wider formation pushes the camera back; an off-centre one drags the
/// focus with it. Both are invisible to a solo trace.
#[test]
fn menu_framing_tracks_the_formation() {
    // Twice the span -> twice the raw depth.
    let wide = menu_framing(
        Some(FormationBox {
            min: [-1600.0, -1600.0],
            max: [1600.0, 1600.0],
        }),
        0.0,
    );
    assert_eq!(wide.tr[2], prescale_tr_z(3200 * 3));
    // The LARGER of the two extents wins, so a wide-but-shallow line
    // frames on its width.
    let shallow = menu_framing(
        Some(FormationBox {
            min: [-800.0, -10.0],
            max: [800.0, 10.0],
        }),
        0.0,
    );
    assert_eq!(shallow.tr[2], traced_menu_tr()[2]);
    // Off-centre formation -> off-centre focus.
    let off = menu_framing(
        Some(FormationBox {
            min: [200.0, -800.0],
            max: [1800.0, 800.0],
        }),
        0.0,
    );
    assert_eq!(off.focus, [1000.0, 0.0, 0.0]);
    assert_eq!(off.tr[2], traced_menu_tr()[2], "same span, same depth");
}

/// Below the 0x800 floor the depth clamps - a solo actor (a degenerate
/// box) does not collapse the camera onto its own head.
#[test]
fn menu_depth_clamps_at_the_retail_floor() {
    let solo = menu_framing(
        Some(FormationBox {
            min: [640.0, -800.0],
            max: [640.0, -800.0],
        }),
        0.0,
    );
    assert_eq!(solo.tr[2], prescale_tr_z(0x800));
    assert_eq!(solo.focus, [640.0, 0.0, -800.0], "still centres on it");
    // No actors at all degenerates the same way, on the origin.
    assert_eq!(menu_framing(None, 0.0).tr[2], prescale_tr_z(0x800));
    assert_eq!(menu_framing(None, 0.0).focus, [0.0; 3]);
}

/// The idle orbit owns yaw across the menu framing, so the case-9 pose
/// passes the live yaw straight through (retail's `_DAT_8007B792`).
#[test]
fn menu_framing_passes_yaw_through() {
    for yaw in [0.0, 1234.0, 4064.0] {
        assert_eq!(menu_framing(Some(traced_formation()), yaw).yaw, yaw);
    }
}

/// The focus trio glides on the same clock as the rotation and
/// translation trios (`FUN_801D829C` tweens all nine together), so a
/// submenu open pans onto the acting seat instead of cutting.
#[test]
fn focus_tweens_with_the_rest_of_the_pose() {
    let mut cam = traced_cam(BattleCamPhase::Menu);
    cam.set_actor(BattleCamActor {
        facing: 0,
        height: Some(1408.0),
        world: [1200.0, 0.0, -800.0],
    });
    cam.set_phase(BattleCamPhase::Submenu);
    // Mid-glide the focus is partway between the formation centre and
    // the seat - not snapped to either end.
    steps(&mut cam, 3);
    let mid = cam.pose().focus;
    assert!(mid[0] > 0.0 && mid[0] < 1200.0, "focus mid-pan: {mid:?}");
    // And it arrives with everything else on step 6.
    steps(&mut cam, 3);
    assert_eq!(cam.pose().focus, [1200.0, 0.0, -800.0]);
    assert_eq!(cam.pose().tr[1], 1408.0);
}

/// Two different seats produce genuinely different framings - the check
/// a solo-Vahn trace structurally cannot make. Same TR trio, different
/// yaw and different focus.
#[test]
fn non_vahn_seats_frame_differently() {
    let seat = |facing, height, world| {
        let mut cam = traced_cam(BattleCamPhase::Menu);
        cam.set_actor(BattleCamActor {
            facing,
            height: Some(height),
            world,
        });
        cam.set_phase(BattleCamPhase::Submenu);
        steps(&mut cam, SUBMENU_ENTER_STEPS as u64);
        cam.pose()
    };
    // Vahn centre-seat, Noa left-seat, Gala right-seat: retail heights
    // 0x480 / 0x3C0 / 0x580 and three different facings.
    let vahn = seat(0, 1152.0, [0.0, 0.0, -800.0]);
    let noa = seat(512, 960.0, [-700.0, 0.0, -900.0]);
    let gala = seat(3584, 1408.0, [700.0, 0.0, -900.0]);
    for (a, b) in [(&vahn, &noa), (&noa, &gala), (&vahn, &gala)] {
        assert_ne!(a.yaw, b.yaw, "facing-relative yaw must differ");
        assert_ne!(a.focus, b.focus, "focus must follow the seat");
        assert_ne!(a.tr[1], b.tr[1], "per-character height must differ");
        assert_eq!(a.tr[0], b.tr[0], "TR.x is seat-invariant");
        assert_eq!(a.tr[2], b.tr[2], "TR.z is seat-invariant");
    }
    // Each yaw is its own `0x8F0 - facing`.
    assert_eq!(vahn.yaw, 2288.0);
    assert_eq!(noa.yaw, (0x8F0 - 512) as f32);
    assert_eq!(gala.yaw.rem_euclid(4096.0), (0x8F0 - 3584 + 4096) as f32);
}

/// The submenu-exit swing stays on the acting actor (retail case 1) and
/// only the return segment pulls the focus back to the formation centre.
#[test]
fn exit_swing_holds_the_seat_then_releases_it() {
    let mut cam = traced_cam(BattleCamPhase::Menu);
    cam.set_actor(BattleCamActor {
        facing: 0,
        height: Some(960.0),
        world: [-700.0, 0.0, -900.0],
    });
    cam.set_phase(BattleCamPhase::Submenu);
    steps(&mut cam, SUBMENU_ENTER_STEPS as u64);
    cam.set_phase(BattleCamPhase::Menu);
    steps(&mut cam, SUBMENU_SWING_STEPS as u64);
    assert_eq!(cam.pose().focus, [-700.0, 0.0, -900.0], "swing holds it");
    steps(&mut cam, SWING_RETURN_STEPS as u64);
    assert_eq!(cam.pose().focus, [0.0; 3], "return re-centres");
    assert_eq!(cam.pose().tr, traced_menu_tr());
}

fn steps(cam: &mut BattleCamera, n: u64) {
    for _ in 0..n {
        cam.advance_to(cam.last_frames + 2);
    }
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

/// `phase_for` is the shared boolean mapping: dialogue outranks the
/// submenu (retail's tutorial text draws over an open menu), and an
/// executing action outranks only the idle far framing.
#[test]
fn phase_for_maps_the_battle_state() {
    assert_eq!(phase_for(true, false, false), BattleCamPhase::Dialogue);
    assert_eq!(phase_for(true, true, true), BattleCamPhase::Dialogue);
    assert_eq!(phase_for(false, true, false), BattleCamPhase::Submenu);
    assert_eq!(phase_for(false, true, true), BattleCamPhase::Submenu);
    assert_eq!(phase_for(false, false, true), BattleCamPhase::Action);
    assert_eq!(phase_for(false, false, false), BattleCamPhase::Menu);
}

/// Case 6's arm fork: the battle-end signal `DAT_8007BD71 == 0xFE`
/// **and** a party slot take the battle-over arm; a running fight
/// (`0xFF`) takes the in-fight arm for party and monster alike.
#[test]
fn action_arm_fork_needs_both_conditions() {
    let f = ActionFraming::default();
    assert!(
        !f.takes_party_arm(),
        "an ordinary party attack in a running fight is the in-fight arm"
    );
    let over = ActionFraming {
        battle_over: true,
        ..f
    };
    assert!(over.takes_party_arm(), "battle over, party seat");
    assert!(
        !ActionFraming {
            party_slot: false,
            ..over
        }
        .takes_party_arm(),
        "monster slot"
    );
}

/// **The in-fight arm, pinned against retail RAM.** Three PCSX-Redux
/// captures parked in `ctx[7] == 0x19` with Gaza (seat 3) acting read the
/// rotation / translation / focus trios directly; each is reproduced here
/// from the context bytes the same captures carry (`ctx[+0x6DA]`,
/// `ctx[+0x6D0]`, `ctx[+0xD]`, `actor[+0x46]`, `actor[+0x34/+0x38]`).
/// The live yaw trails the counter by the tween's lag, so the yaw is
/// checked against the counter the arm computes from, which is what the
/// walker converges on.
#[test]
fn in_fight_arm_reproduces_the_gaza_captures() {
    // gaza2_park_0x19: rot (0, 1657), TR (0, 1280, 5324), focus (-433, 0,
    // -291) stored negated; 6DA 4352, 6D0 3328, D 0, facing 2687.
    let a = action_framing(
        BattleCamActor {
            facing: 2687,
            world: [433.0, -410.0, 291.0],
            height: None,
        },
        ActionFraming {
            party_slot: false,
            yaw_base: 4352,
            depth_raw: 3328,
            ..Default::default()
        },
    );
    assert_eq!(a.pitch, 0.0);
    assert_eq!(a.yaw, ((4352 - 2687) & 0xFFF) as f32);
    assert_eq!(a.tr, [0.0, 1280.0, 5324.0]);
    assert_eq!(a.focus, [433.0, 0.0, 291.0], "focus height is the floor");
    // gaza2_park_0x19_summon_melee: rot (0, 1412), same TR, focus (785,
    // 0, -39); 6DA 2476, facing 1056.
    let b = action_framing(
        BattleCamActor {
            facing: 1056,
            world: [-785.0, 0.0, 39.0],
            height: None,
        },
        ActionFraming {
            party_slot: false,
            yaw_base: 2476,
            depth_raw: 3328,
            ..Default::default()
        },
    );
    assert_eq!(b.yaw, 1420.0);
    assert_eq!(b.focus, [-785.0, 0.0, 39.0]);
    // gaza2_park_0x19_target_vahn: rot (128, 574), TR (0, 1024, 5324),
    // focus (0, 0, -1490); 6DA 6726, D 2, facing 2048.
    let c = action_framing(
        BattleCamActor {
            facing: 2048,
            world: [0.0, 0.0, 1490.0],
            height: None,
        },
        ActionFraming {
            party_slot: false,
            yaw_base: 6726,
            depth_raw: 3328,
            style: 2,
            ..Default::default()
        },
    );
    assert_eq!(c.pitch, 128.0);
    assert_eq!(c.yaw, ((6726 - 2048) & 0xFFF) as f32);
    assert_eq!(c.tr, [0.0, 1024.0, 5324.0]);
    assert_eq!(c.focus, [0.0, 0.0, 1490.0]);
}

/// The reading this replaces put a party attacker through the
/// battle-over arm: eye `prescale(0x500)` = 2048 projection units behind
/// the actor, which parked the camera inside whichever combatant stood
/// there. A party seat in a running fight frames at the framed
/// monster's depth like everyone else, and the old value must not come
/// back.
#[test]
fn a_party_attack_in_a_running_fight_is_not_the_battle_over_close_up() {
    let vahn = BattleCamActor {
        facing: 0,
        world: [0.0, 0.0, -800.0],
        height: None,
    };
    let live = action_framing(
        vahn,
        ActionFraming {
            yaw_base: 0x280,
            depth_raw: 0xC00,
            ..Default::default()
        },
    );
    assert_eq!(
        live.tr[2],
        prescale_tr_z(0xC00),
        "the framed monster's depth"
    );
    assert_ne!(live.tr[2], prescale_tr_z(ACTION_PARTY_TR_Z_RAW));
    assert_eq!(live.tr[1], ACTION_TR_Y);
    assert_eq!(
        live.yaw, 0x280 as f32,
        "counter minus facing, not 0x800 - facing"
    );
    // The same seat once the battle-end signal is up is the close-up.
    let over = action_framing(
        vahn,
        ActionFraming {
            battle_over: true,
            ..Default::default()
        },
    );
    assert_eq!(over.tr[2], prescale_tr_z(ACTION_PARTY_TR_Z_RAW));
    assert_eq!(over.yaw, 0x800 as f32);
}

/// **The screen-space consequence, pinned through the retail projection.**
/// A party melee at the range retail's own mid-art capture measures
/// (`battle_melee_hit_spark`: attacker `(-21, 136)`, target `(-6, 405)`,
/// 270 apart) is filmed by the in-fight arm at the framed monster's
/// depth (`ctx[+0x6D0] = 0xC00` in that fight) from the party seed
/// `0x280`; both combatants must land inside the 320x240 frame, feet
/// and head. The arm the port shipped before - the battle-over arm with
/// its `prescale(0x500)` eye behind the attacker - puts the eye 968
/// projection units short of the target, whose body then spans more
/// than the whole frame ("223 of 240 scanlines covered, ndc.y = -1.63"
/// was the earlier measurement). The two arms are projected through the
/// same [`battle_vp`] so the assertion is on the picture, not the pose.
#[test]
fn a_party_melee_keeps_both_combatants_in_frame() {
    // Retail party model height stand-in: ~200 stage units (the display
    // trio reads `-215` for a seated party member's origin), Y-down.
    const HEAD: f32 = -200.0;
    let attacker = BattleCamActor {
        facing: 0,
        world: [0.0, 0.0, 530.0],
        height: None,
    };
    let target = [0.0, 0.0, 800.0];
    let live = action_framing(
        attacker,
        ActionFraming {
            yaw_base: 0x280,
            depth_raw: 0xC00,
            ..Default::default()
        },
    );
    // Actor draw class: the hosts compose `scale(4) * FLIP` under the
    // camera (see `battle_vp_matches_the_handrolled_retail_projection`).
    let scale4: [f32; 16] = [
        4.0, 0.0, 0.0, 0.0, //
        0.0, 4.0, 0.0, 0.0, //
        0.0, 0.0, 4.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    let model = mat_mul(&scale4, &FLIP);
    let vp = battle_vp(&live, 4.0, 4.0 / 3.0);
    let inside = |v: [f32; 3]| -> (f32, f32) {
        let (x, y) = project(&vp, &model, v).expect("in front of the eye");
        assert!(
            (0.0..=320.0).contains(&x) && (0.0..=240.0).contains(&y),
            "{v:?} projects off-frame at ({x:.1}, {y:.1})"
        );
        (x, y)
    };
    let (_, a_feet) = inside(attacker.world);
    let (_, a_head) = inside([attacker.world[0], HEAD, attacker.world[2]]);
    let (_, t_feet) = inside(target);
    let (_, t_head) = inside([target[0], HEAD, target[2]]);
    assert!(a_head < a_feet && t_head < t_feet, "upright");
    // Neither body dominates the frame: retail's mid-art frame shows both
    // fighters at well under half the frame height.
    assert!(a_feet - a_head < 120.0, "attacker {}", a_feet - a_head);
    assert!(t_feet - t_head < 120.0, "target {}", t_feet - t_head);

    // The arm the port used to take for the same swing.
    let over = action_framing(
        attacker,
        ActionFraming {
            battle_over: true,
            ..Default::default()
        },
    );
    let vp = battle_vp(&over, 4.0, 4.0 / 3.0);
    let (_, t_feet) = project(&vp, &model, target).expect("target in front of the eye");
    let t_head = project(&vp, &model, [target[0], HEAD, target[2]]).map(|p| p.1);
    // Feet below the frame or head above it: the target's body spans
    // more than the whole frame - the belly close-up.
    let spans_frame = t_feet > 240.0 || t_head.is_none_or(|y| y < 0.0);
    assert!(
        spans_frame,
        "the battle-over arm must reproduce the old close-up (feet {t_feet:.1}, head {t_head:?})"
    );
}

/// The yaw counter's per-action ladder, driven through the shared entry
/// on the action-state edges: round begin `0`, seed `0x800`, Attack entry
/// `0x200`, and a party attacker's strike loop `0x280` / `0xA80`. A
/// monster attacker keeps the `0x200` base through its own strike loop.
#[test]
fn the_yaw_counter_is_reseeded_on_the_action_state_edges() {
    let mut slot: Option<BattleCamera> = None;
    let mut frames = 0u64;
    let mut feed = |slot: &mut Option<BattleCamera>, state: u8, party: bool| {
        frames += 2;
        drive(
            slot,
            true,
            BattleCamInputs {
                phase: phase_for_state(false, false, state, DoneBandInputs::default()),
                acting: Some(BattleCamActor::default()),
                action: ActionFraming {
                    party_slot: party,
                    ..Default::default()
                },
                action_state: state,
                ..Default::default()
            },
            frames,
            None,
        );
    };
    // Retail's counter advances one unit per display frame, so each fed
    // frame pair adds 2 on top of the seed (the creating frame elapses
    // nothing).
    feed(&mut slot, 0x00, true);
    assert_eq!(slot.as_ref().unwrap().action_yaw_base(), 0, "round begin");
    feed(&mut slot, 0x0C, true);
    assert_eq!(slot.as_ref().unwrap().action_yaw_base(), 0x802);
    feed(&mut slot, 0x14, true);
    assert_eq!(slot.as_ref().unwrap().action_yaw_base(), 0x202);
    feed(&mut slot, 0x16, true);
    feed(&mut slot, 0x16, true);
    assert_eq!(
        slot.as_ref().unwrap().action_yaw_base(),
        0x206,
        "no edge, drift only"
    );
    feed(&mut slot, 0x1E, true);
    let seeded = slot.as_ref().unwrap().action_yaw_base() - 2;
    assert!(
        seeded == 0x280 || seeded == 0xA80,
        "party strike loop seeds 0x280 or 0xA80, got {seeded:#x}"
    );
    // A fresh camera, monster attacker: the strike loop leaves 0x200.
    let mut monster: Option<BattleCamera> = None;
    feed(&mut monster, 0x0C, false);
    assert_eq!(monster.as_ref().unwrap().action_yaw_base(), 0x800);
    feed(&mut monster, 0x14, false);
    feed(&mut monster, 0x1E, false);
    assert_eq!(monster.as_ref().unwrap().action_yaw_base(), 0x204);
    // Round begin zeroes it again.
    feed(&mut monster, 0x00, false);
    assert_eq!(monster.as_ref().unwrap().action_yaw_base(), 2);
}

/// The seed pass and the band entry can land inside one host tick, so
/// the observer keys on **band entry**: a monster attack seen as
/// `0x00 -> 0x15` and a party attack seen as `0x5A -> 0x0A -> 0x14` both
/// land on the `0x200` base (the `0x801E2F20` store is in the `0x0C`
/// pass), a spell band entered the same way keeps `0x800`, and an
/// attack-band entry from another action band - no seed pass between -
/// stores nothing.
#[test]
fn the_yaw_counter_seeds_on_band_entry_when_states_are_skipped() {
    let mut frames = 0u64;
    let mut feed = |slot: &mut Option<BattleCamera>, state: u8, party: bool| {
        frames += 2;
        drive(
            slot,
            true,
            BattleCamInputs {
                phase: phase_for_state(false, false, state, DoneBandInputs::default()),
                acting: Some(BattleCamActor::default()),
                action: ActionFraming {
                    party_slot: party,
                    ..Default::default()
                },
                action_state: state,
                ..Default::default()
            },
            frames,
            None,
        );
    };
    // Monster: the engine's SM runs `0x0A -> 0x0C -> 0x14` inside the
    // tick, and the camera first sees the walk state.
    let mut monster: Option<BattleCamera> = None;
    feed(&mut monster, 0x00, false);
    feed(&mut monster, 0x15, false);
    assert_eq!(monster.as_ref().unwrap().action_yaw_base(), 0x202);
    feed(&mut monster, 0x1E, false);
    assert_eq!(
        monster.as_ref().unwrap().action_yaw_base(),
        0x204,
        "monster: no coin"
    );
    // Party, overworld shape: `0x5A -> 0x0A -> 0x14`.
    let mut party: Option<BattleCamera> = None;
    feed(&mut party, 0x5A, true);
    feed(&mut party, 0x0A, true);
    assert_eq!(
        party.as_ref().unwrap().action_yaw_base(),
        // (the creating feed elapses nothing; the second adds 2)
        2,
        "setup band: untouched"
    );
    feed(&mut party, 0x14, true);
    assert_eq!(party.as_ref().unwrap().action_yaw_base(), 0x202);
    // A spell band entered from the setup band keeps the seed's 0x800.
    let mut caster: Option<BattleCamera> = None;
    feed(&mut caster, 0x0A, true);
    feed(&mut caster, 0x28, true);
    assert_eq!(caster.as_ref().unwrap().action_yaw_base(), 0x802);
    // ... and crossing into the attack band from there is not a seed.
    feed(&mut caster, 0x14, true);
    assert_eq!(caster.as_ref().unwrap().action_yaw_base(), 0x804);
}

/// The battle-over arm frames from behind the actor at a constant depth,
/// and the height floor tilts the pitch instead of sinking the camera.
#[test]
fn party_action_arm_floors_the_height_and_tilts_the_pitch() {
    let ground = BattleCamActor {
        facing: 0x200,
        world: [100.0, 0.0, -800.0],
        height: None,
    };
    let over = ActionFraming {
        battle_over: true,
        ..Default::default()
    };
    let p = action_framing(ground, over);
    assert_eq!(p.yaw, (0x800 - 0x200) as f32, "0x800 - facing");
    assert_eq!(p.tr[0], 0.0);
    assert_eq!(p.tr[2], prescale_tr_z(ACTION_PARTY_TR_Z_RAW));
    assert_eq!(p.focus, [100.0, 0.0, -800.0], "orbits the acting actor");
    // TR.y would be -5 * 0 = 0, under the floor: raised to 0x280 with a
    // quarter of the shortfall added to the pitch.
    assert_eq!(p.tr[1], ACTION_HEIGHT_FLOOR);
    assert_eq!(p.pitch, (0x280 / 4) as f32);
    // An actor lifted off the ground (retail Y is down-positive, so a
    // negative Y is airborne) frames higher and stops tilting once the
    // scaled height clears the floor.
    let airborne = BattleCamActor {
        world: [0.0, -200.0, 0.0],
        ..ground
    };
    let q = action_framing(airborne, over);
    assert_eq!(q.tr[1], 1000.0, "-5 * -200");
    assert_eq!(q.pitch, 0.0, "clear of the floor, no compensation");
}

/// The in-fight arm is the one that reads `ctx[+0x6D0]` - the depth
/// `camera_height_for_frame` derives from the framed monster's size.
#[test]
fn fallback_action_arm_reads_the_computed_depth() {
    let monster = ActionFraming {
        party_slot: false,
        depth_raw: 0x1400,
        ..Default::default()
    };
    let actor = BattleCamActor {
        facing: 0x100,
        world: [0.0, 0.0, 800.0],
        height: None,
    };
    let p = action_framing(actor, monster);
    assert_eq!(p.pitch, 0.0);
    assert_eq!(p.yaw, (4096 - 0x100) as f32, "yaw_base 0 minus the facing");
    assert_eq!(p.tr, [0.0, ACTION_TR_Y, prescale_tr_z(0x1400)]);
    assert_eq!(monster.raw_z(), 0x1400);
    // A bulkier monster pulls the camera back; the size class is the only
    // thing that moves between these two.
    let small = ActionFraming {
        depth_raw: crate::battle_formulas::CAMERA_HEIGHT_MIN as i32,
        ..monster
    };
    assert!(action_framing(actor, small).tr[2] < p.tr[2]);
}

/// `ctx[+0xD]` styles 1/3 add the half turn; 2/3 share the height +
/// pitch body (retail reaches it by falling out of the `== 3` arm).
#[test]
fn fallback_style_byte_selects_the_three_tweaks() {
    let base = ActionFraming {
        party_slot: false,
        yaw_base: 0x400,
        ..Default::default()
    };
    let actor = BattleCamActor::default();
    let at = |style| action_framing(actor, ActionFraming { style, ..base });
    assert_eq!(at(0).yaw, 0x400 as f32);
    assert_eq!(at(0).tr[1], ACTION_TR_Y);
    assert_eq!(at(1).yaw, 0xC00 as f32);
    assert_eq!(at(1).tr[1], ACTION_TR_Y, "style 1 leaves the height");
    assert_eq!(at(2).yaw, 0x400 as f32, "style 2 leaves the yaw");
    assert_eq!(at(2).tr[1], ACTION_STYLE_TR_Y);
    assert_eq!(at(2).pitch, ACTION_STYLE_PITCH);
    // Style 3 is both.
    assert_eq!(at(3).yaw, 0xC00 as f32);
    assert_eq!(at(3).tr[1], ACTION_STYLE_TR_Y);
    assert_eq!(at(3).pitch, ACTION_STYLE_PITCH);
    assert_eq!(at(4).yaw, at(0).yaw, "no arm for any other style");
    assert_eq!(at(4).tr, at(0).tr);
}

/// Character `4` in a party seat replaces the fallback translation
/// wholesale, depth included.
#[test]
fn fallback_character_four_override_replaces_the_translation() {
    let f = ActionFraming {
        char_id: ACTION_OVERRIDE_CHAR_ID, // party slot, fight running
        depth_raw: 0x1400,
        style: 2,
        ..Default::default()
    };
    let p = action_framing(BattleCamActor::default(), f);
    assert_eq!(p.pitch, 0x80 as f32, "overrides the style tilt too");
    assert_eq!(p.tr[1], 0x300 as f32);
    assert_eq!(p.tr[2], prescale_tr_z(0xC00));
    assert_eq!(f.raw_z(), 0xC00);
    // A monster slot with the same id keeps the computed depth - retail
    // gates the override on `ctx[+0x13] < 3`.
    let monster = ActionFraming {
        party_slot: false,
        ..f
    };
    assert_eq!(monster.raw_z(), 0x1400);
    assert_eq!(
        action_framing(BattleCamActor::default(), monster).tr[1],
        ACTION_STYLE_TR_Y
    );
}

/// Entering the action phase glides to the case-6 framing over retail's
/// own `a3 = 0xC` (6 camera steps); leaving it returns to the far
/// framing over case 9's `a3 = 0xE` (7 steps) with the orbit running.
#[test]
fn action_phase_glides_in_over_six_steps_and_out_over_seven() {
    let mut cam = traced_cam(BattleCamPhase::Menu);
    cam.set_actor(BattleCamActor {
        facing: 0,
        height: None,
        world: [0.0, 0.0, -800.0],
    });
    let want = action_framing(
        BattleCamActor {
            facing: 0,
            height: None,
            world: [0.0, 0.0, -800.0],
        },
        ActionFraming::default(),
    );
    cam.set_phase(BattleCamPhase::Action);
    steps(&mut cam, ACTION_STEPS as u64 - 1);
    assert_ne!(cam.framing_pose().tr, want.tr, "still mid-glide");
    steps(&mut cam, 1);
    let p = cam.framing_pose();
    assert_eq!(p.pitch, want.pitch);
    assert_eq!(p.tr, want.tr);
    assert_eq!(p.focus, want.focus);
    // The in-fight arm's yaw is `ctx[+0x6DA] - facing`, and the counter
    // advances one unit per display frame while the framing is re-armed
    // every pass - so the yaw the glide lands on is the counter's value
    // at the landing step (two frames per step), not the one it started
    // from.
    assert_eq!(cam.action_yaw_base(), 2 * ACTION_STEPS as i32);
    assert_eq!(p.yaw.rem_euclid(4096.0), cam.action_yaw_base() as f32);
    // Held while the action runs - no idle orbit in the Action phase.
    // Pitch, translation and focus stand still; the yaw keeps chasing
    // the drifting counter (the three retail `0x19` parks read it eight
    // units behind the counter, so the drift is retail's, not a
    // settling residue).
    steps(&mut cam, 20);
    let held = cam.framing_pose();
    assert_eq!((held.pitch, held.tr, held.focus), (p.pitch, p.tr, p.focus));
    // 20 steps = 40 counter units; the re-armed 6-step tween trails the
    // moving target by a few units between its exact landings.
    let drift = held.yaw.rem_euclid(4096.0) - p.yaw;
    assert!(
        (28.0..=40.0).contains(&drift),
        "yaw chased the counter: {drift}"
    );
    // End of action: back to the far framing over 7 steps.
    cam.set_phase(BattleCamPhase::Menu);
    steps(&mut cam, SWING_RETURN_STEPS as u64);
    assert_eq!(cam.framing_pose().tr, traced_menu_tr());
    assert_eq!(cam.framing_pose().pitch, MENU_PITCH);
}

/// The action framing pulls in on the actor - the whole point of the
/// phase - and the in-fight arm's depth is the one thing that can push
/// it back out again.
#[test]
fn action_framing_pulls_in_except_for_the_bulkiest_monsters() {
    let actor = BattleCamActor::default();
    let depth = |d| {
        action_framing(
            actor,
            ActionFraming {
                party_slot: false,
                depth_raw: d,
                ..Default::default()
            },
        )
        .tr[2]
    };
    let far = menu_framing(Some(traced_formation()), 0.0).tr[2];
    let party = action_framing(actor, ActionFraming::default());
    assert!(party.tr[2] < far, "party close-up: {party:?} vs {far}");
    assert_eq!(party.focus, actor.world, "framed on the acting actor");
    // The size-class floor still frames closer than the traced far
    // framing; the ceiling does not - a maximum-bulk monster is framed
    // from further out than the solo tutorial formation was.
    assert!(depth(crate::battle_formulas::CAMERA_HEIGHT_MIN as i32) < far);
    assert!(depth(crate::battle_formulas::CAMERA_HEIGHT_MAX as i32) > far);
}

/// The idle bands - stated over the real [`ActionState`] enum rather
/// than over raw bytes, so a state added to the SM lands on one side of
/// the line deliberately.
///
/// The **Done band** is on the framed side: its arms re-arm case 6 / 8
/// every pass under the `ctx[+0x6D8]` tail timer, and the end-of-action
/// gate `0x5A` is where the far framing takes over.
#[test]
fn the_idle_states_leave_the_action_framing() {
    use crate::battle_action::ActionState;
    for s in [
        ActionState::Begin,
        ActionState::PreActionWait,
        ActionState::QueuedFromMenu,
        ActionState::EndOfAction,
        ActionState::RunBegin,
    ] {
        assert!(
            !action_state_frames_the_action(s.as_byte()),
            "{s:?} is idle - it must leave the far framing and its orbit"
        );
    }
    for s in [
        ActionState::ActionSeed,
        ActionState::AttackFace,
        ActionState::AttackAdvance,
        ActionState::AttackStrike,
        ActionState::MagicHitLoop,
        ActionState::SummonSustain,
        ActionState::DoneCleanup,
        ActionState::DoneFadeDown,
        ActionState::DoneMultiCast,
    ] {
        assert!(
            action_state_frames_the_action(s.as_byte()),
            "{s:?} frames the action"
        );
    }
    // Retail's own pair is a subset, kept visible rather than folded in.
    for s in RETAIL_ORBIT_STATES {
        assert!(!action_state_frames_the_action(s));
    }
}

/// The in-fight arm's yaw drifts with the action SM's own counter, so
/// successive enemy actions do not all frame from the same angle.
#[test]
fn action_yaw_counter_drifts_one_unit_per_display_frame() {
    let inputs = BattleCamInputs {
        target: None,
        entry_yaw: 0.0,
        phase: BattleCamPhase::Menu,
        action: ActionFraming {
            party_slot: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut slot: Option<BattleCamera> = None;
    drive(&mut slot, true, inputs, 0, None);
    // 200 display frames of idling, then the action opens.
    drive(&mut slot, true, inputs, 200, None);
    let action = BattleCamInputs {
        target: None,
        entry_yaw: 0.0,
        phase: BattleCamPhase::Action,
        ..inputs
    };
    for f in 0..=ACTION_STEPS as u64 {
        drive(&mut slot, true, action, 200 + f * 2, None);
    }
    let first = slot.as_ref().unwrap().framing_pose().yaw;
    // Case 6 is re-armed on every action-SM pass, so the framing chases
    // the counter rather than freezing on its value at the phase change:
    // 200 at entry plus the 12 display frames the glide spans.
    assert_eq!(
        first.rem_euclid(4096.0),
        200.0 + (ACTION_STEPS as f32) * 2.0,
        "yaw_base chases the live counter"
    );
    // A later action frames from a different angle.
    drive(&mut slot, true, inputs, 400, None);
    for f in 0..=ACTION_STEPS as u64 {
        drive(&mut slot, true, action, 400 + f * 2, None);
    }
    assert_ne!(slot.as_ref().unwrap().framing_pose().yaw, first);
}

/// The shake rides on top of the framing pose: `pose()` carries it,
/// `framing_pose()` does not, and amplitude 0 keeps them equal.
#[test]
fn shake_offsets_the_pose_only_while_the_amplitude_is_live() {
    let mut cam = traced_cam(BattleCamPhase::Menu);
    steps(&mut cam, 4);
    assert_eq!(cam.pose(), cam.framing_pose(), "no amplitude, no jitter");
    assert_eq!(cam.shake_offset(), [0, 0]);
    cam.set_shake_amplitude(8);
    let mut moved = false;
    for _ in 0..8 {
        steps(&mut cam, 1);
        let (shaken, framing) = (cam.pose(), cam.framing_pose());
        assert_eq!(shaken.pitch, framing.pitch, "shake is translation-only");
        assert_eq!(shaken.tr[2], framing.tr[2]);
        assert_eq!(
            [shaken.tr[0] - framing.tr[0], shaken.tr[1] - framing.tr[1]],
            [cam.shake_offset()[0] as f32, cam.shake_offset()[1] as f32]
        );
        moved |= shaken.tr[0] != framing.tr[0] || shaken.tr[1] != framing.tr[1];
    }
    assert!(moved, "amplitude 8 must actually jitter the translation");
    // Dropping the amplitude backs the last offset out on the next step.
    cam.set_shake_amplitude(0);
    steps(&mut cam, 1);
    assert_eq!(cam.shake_offset(), [0, 0]);
    assert_eq!(cam.pose(), cam.framing_pose());
}

/// A live shake must not stall a glide: the framing still lands exactly
/// on its endpoint (which is why the offset is held beside the pose).
#[test]
fn shake_does_not_prevent_a_glide_from_completing() {
    let mut cam = traced_cam(BattleCamPhase::Dialogue);
    cam.set_shake_amplitude(0x10);
    cam.set_phase(BattleCamPhase::Menu);
    steps(&mut cam, 20);
    assert_eq!(cam.framing_pose().tr, traced_menu_tr());
}

/// `drive` owns the whole per-frame ordering: entry snap, retarget,
/// phase change, step - and drops the state when the battle ends.
#[test]
fn drive_creates_steps_and_drops() {
    let at = |phase, formation| BattleCamInputs {
        target: None,
        entry_yaw: 0.0,
        phase,
        formation,
        ..Default::default()
    };
    let mut slot: Option<BattleCamera> = None;
    // Inactive: stays empty.
    drive(&mut slot, false, at(BattleCamPhase::Menu, None), 0, None);
    assert!(slot.is_none());
    // First active frame in the Menu phase: entry snap to the far
    // framing (BOOT depth - no formation installed yet on frame 0).
    drive(&mut slot, true, at(BattleCamPhase::Menu, None), 0, None);
    let p0 = slot.as_ref().unwrap().pose();
    assert_eq!((p0.pitch, p0.tr), (BOOT_POSE.pitch, BOOT_POSE.tr));
    // Formation + 2 frames: the framing resizes and the orbit runs.
    let formation = Some(FormationBox {
        min: [-800.0, -800.0],
        max: [800.0, 800.0],
    });
    drive(
        &mut slot,
        true,
        at(BattleCamPhase::Menu, formation),
        2,
        None,
    );
    let p1 = slot.as_ref().unwrap().pose();
    assert_eq!(p1.yaw, 4092.0, "one orbit step");
    // Submenu opens on the traced default seat: 6 steps arrive on the
    // close-up.
    for f in 2..8 {
        drive(
            &mut slot,
            true,
            at(BattleCamPhase::Submenu, formation),
            f * 2,
            None,
        );
    }
    let p2 = slot.as_ref().unwrap().pose();
    assert_eq!(p2.tr, BattleCamActor::default().submenu_pose().tr);
    // Battle ends: the state drops so the next battle re-snaps.
    drive(&mut slot, false, at(BattleCamPhase::Menu, None), 16, None);
    assert!(slot.is_none());
}

/// `drive` carries the action framing and the shake amplitude through to
/// the camera - the two channels a host could otherwise wire on one side
/// only.
#[test]
fn drive_carries_the_action_and_shake_channels() {
    let inputs = BattleCamInputs {
        target: None,
        entry_yaw: 0.0,
        phase: BattleCamPhase::Action,
        acting: Some(BattleCamActor::default()),
        action: ActionFraming {
            party_slot: false,
            depth_raw: 0x1400,
            ..Default::default()
        },
        shake_amplitude: 6,
        ..Default::default()
    };
    let mut slot: Option<BattleCamera> = None;
    for f in 0..=ACTION_STEPS as u64 {
        drive(&mut slot, true, inputs, f * 2, None);
    }
    let cam = slot.as_ref().unwrap();
    let want = action_framing(BattleCamActor::default(), inputs.action);
    assert_eq!(cam.framing_pose().tr, want.tr, "case-6 fallback depth");
    assert_ne!(cam.shake_offset(), [0, 0], "amplitude reached the kernel");
}

/// A Menu-phase entry with the formation already live snaps straight to
/// the case-9 formation-sized framing - no degenerate minimum-depth
/// frame while waiting for the first phase transition.
#[test]
fn drive_entry_sizes_to_the_live_formation() {
    let formation = Some(FormationBox {
        min: [-800.0, -800.0],
        max: [800.0, 800.0],
    });
    let mut slot: Option<BattleCamera> = None;
    drive(
        &mut slot,
        true,
        BattleCamInputs {
            phase: BattleCamPhase::Menu,
            formation,
            ..Default::default()
        },
        0,
        None,
    );
    let p = slot.as_ref().unwrap().pose();
    assert_eq!(p.tr, [0.0, 1280.0, 7680.0], "the traced far framing");
    assert_eq!(p.focus, [0.0; 3]);
}

/// A battle that opens on dialogue snaps to the held close-up; any other
/// entry snaps to the far framing.
#[test]
fn drive_entry_snap_follows_the_opening_phase() {
    let mut slot: Option<BattleCamera> = None;
    drive(
        &mut slot,
        true,
        BattleCamInputs {
            phase: BattleCamPhase::Dialogue,
            ..Default::default()
        },
        0,
        None,
    );
    assert_eq!(slot.as_ref().unwrap().pose(), dialogue_pose(None));
    let mut slot: Option<BattleCamera> = None;
    // Opening straight into a submenu still enters at the menu framing
    // and glides in (retail's loading pose resolves at the far framing).
    drive(
        &mut slot,
        true,
        BattleCamInputs {
            phase: BattleCamPhase::Submenu,
            ..Default::default()
        },
        0,
        None,
    );
    let p = slot.as_ref().unwrap().pose();
    assert_ne!(p.tr, BattleCamActor::default().submenu_pose().tr);
}

/// Project a point through [`battle_vp`] composed with the same
/// per-draw model factors the hosts use, to PSX 320x240 screen pixels.
fn project(vp: &[f32; 16], model: &[f32; 16], v: [f32; 3]) -> Option<(f32, f32)> {
    let m = mat_mul(vp, model);
    let x = m[0] * v[0] + m[4] * v[1] + m[8] * v[2] + m[12];
    let y = m[1] * v[0] + m[5] * v[1] + m[9] * v[2] + m[13];
    let w = m[3] * v[0] + m[7] * v[1] + m[11] * v[2] + m[15];
    if w <= 1.0 {
        return None;
    }
    Some((160.0 + x / w * 160.0, 120.0 - y / w * 120.0))
}

/// The Y-flip model factor every host draw carries.
const FLIP: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, -1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

/// The GTE origin projects to the control file's screen centre, not to
/// the geometric centre of the frame.
///
/// This is the assertion the hand-rolled comparison below cannot make on
/// its own: that test only says the matrix and the reference agree, so it
/// stayed green while both carried `120`. Here the expected value comes
/// from [`GTE_OFY`] and the projected value comes from the matrix, so a
/// matrix built on `240 / 2` fails.
#[test]
fn the_projected_origin_lands_on_the_retail_screen_centre() {
    // Eye straight down -Z at the origin: `focus` and the rotation are
    // zero, so `v = origin` lands at eye `(0, 0, TR.z)` and the retail
    // transform reduces to `screen = (OFX, OFY)`.
    let pose = BattleCamPose {
        pitch: 0.0,
        yaw: 0.0,
        tr: [0.0, 0.0, 7680.0],
        focus: [0.0, 0.0, 0.0],
    };
    let vp = battle_vp(&pose, 4.0, 4.0 / 3.0);
    let (sx, sy) = project(&vp, &FLIP, [0.0, 0.0, 0.0]).unwrap();
    assert!((sx - GTE_OFX).abs() < 0.01, "OFX: {sx} vs {GTE_OFX}");
    assert!((sy - GTE_OFY).abs() < 0.01, "OFY: {sy} vs {GTE_OFY}");
    // And the centre is genuinely off the naive one, so this is not a
    // tautology about the viewport map.
    assert!((sy - 120.0).abs() > 5.0);
}

/// [`battle_vp`] reproduces the exact retail projection
/// `screen = H*(Rx(p)*Ry(y)*(v*S - focus*S) + TR)/Ez` against a
/// hand-rolled reference, dome (raw units) and actor (4x world scale)
/// draw classes both.
#[test]
fn battle_vp_matches_the_handrolled_retail_projection() {
    let pose = BattleCamPose {
        pitch: 32.0,
        yaw: 224.0,
        tr: [0.0, 1280.0, 7680.0],
        focus: [100.0, 0.0, -50.0],
    };
    let handrolled = |v: [f32; 3], s: f32| -> Option<(f32, f32)> {
        let to_rad = |u: f32| u / 4096.0 * std::f32::consts::TAU;
        let (sy, cy) = to_rad(pose.yaw).sin_cos();
        let (sp, cp) = to_rad(pose.pitch).sin_cos();
        // World-scale the vertex and subtract the scaled focus.
        let p = [
            v[0] * s - pose.focus[0] * 4.0,
            v[1] * s - pose.focus[1] * 4.0,
            v[2] * s - pose.focus[2] * 4.0,
        ];
        let ry = [cy * p[0] + sy * p[2], p[1], -sy * p[0] + cy * p[2]];
        let e = [ry[0], cp * ry[1] - sp * ry[2], sp * ry[1] + cp * ry[2]];
        let ez = e[2] + pose.tr[2];
        if ez <= 1.0 {
            return None;
        }
        // The screen centre is the GTE control file's `(OFX, OFY)`, not
        // the geometric `(160, 120)`: writing the naive centre on both
        // sides of this comparison is what let the six-pixel error live.
        Some((
            256.0 * (e[0] + pose.tr[0]) / ez + GTE_OFX,
            256.0 * (e[1] + pose.tr[1]) / ez + GTE_OFY,
        ))
    };
    let vp = battle_vp(&pose, 4.0, 4.0 / 3.0);
    // Dome class: raw PSX vertices, model = FLIP (the vp's trailing flip
    // cancels it, so the retail chain sees the raw Y-down vertex).
    for v in [[1000.0f32, -500.0, 3000.0], [-2000.0, 0.0, 6000.0]] {
        let got = project(&vp, &FLIP, v).unwrap();
        // The dome draws unscaled but the camera focus is scaled - the
        // handrolled reference scales the vertex by 1 and focus by 4.
        let want = handrolled(v, 1.0).unwrap();
        let d = ((got.0 - want.0).powi(2) + (got.1 - want.1).powi(2)).sqrt();
        assert!(d < 0.05, "dome {v:?}: {d}px ({got:?} vs {want:?})");
    }
    // Actor class: model composes scale(4)*FLIP under the camera.
    let scale4: [f32; 16] = [
        4.0, 0.0, 0.0, 0.0, //
        0.0, 4.0, 0.0, 0.0, //
        0.0, 0.0, 4.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    let model = mat_mul(&scale4, &FLIP);
    for v in [[100.0f32, -130.0, -800.0], [0.0, 0.0, 800.0]] {
        let got = project(&vp, &model, v).unwrap();
        let want = handrolled(v, 4.0).unwrap();
        let d = ((got.0 - want.0).powi(2) + (got.1 - want.1).powi(2)).sqrt();
        assert!(d < 0.05, "actor {v:?}: {d}px ({got:?} vs {want:?})");
    }
}

/// The far menu framing keeps the whole formation inside the 320x240
/// frame - the property the browser's old orbit-projection approximation
/// broke (party out of shot at some angles).
#[test]
fn menu_framing_keeps_the_formation_on_screen() {
    let formation = FormationBox {
        min: [-800.0, -900.0],
        max: [800.0, 800.0],
    };
    let scale4: [f32; 16] = [
        4.0, 0.0, 0.0, 0.0, //
        0.0, 4.0, 0.0, 0.0, //
        0.0, 0.0, 4.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    let model = mat_mul(&scale4, &FLIP);
    for yaw in [0.0f32, 512.0, 1024.0, 2048.0, 3000.0, 3900.0] {
        let pose = menu_framing(Some(formation), yaw);
        let vp = battle_vp(&pose, 4.0, 4.0 / 3.0);
        for (x, z) in [
            (formation.min[0], formation.min[1]),
            (formation.min[0], formation.max[1]),
            (formation.max[0], formation.min[1]),
            (formation.max[0], formation.max[1]),
        ] {
            // Seat position + a standing character's head (~130 raw
            // units up = negative retail Y; the model/vp flips cancel,
            // so the raw Y-down coordinate goes straight in).
            for y in [0.0f32, -130.0] {
                let (sx, sy) = project(&vp, &model, [x, y, z])
                    .expect("formation corner in front of the camera");
                assert!(
                    (-40.0..360.0).contains(&sx) && (-40.0..280.0).contains(&sy),
                    "yaw {yaw}: corner ({x},{z}) h {y} off-frame at ({sx},{sy})"
                );
            }
        }
    }
}

/// **The top-level command chooser keeps the far framing.**
///
/// The retail battle menu driver `FUN_801D388C` arms case `0` *and* case
/// `9`, so "a menu is open" does not select the close-up on its own; two
/// retail framebuffers separate them (a Begin/Run save reads case 9's
/// `TR (0, 1280, 7680)` over `+-800` seats, an arts-input save reads case
/// 0's `TR (-512, 1152, 2457)`). The port used to fold every battle menu
/// into `Submenu`, which put the camera behind the acting character with
/// the enemy **behind the eye** for the whole command phase.
#[test]
fn only_the_input_pickers_take_the_close_up() {
    assert_eq!(
        phase_for_state(false, false, 0x5A, DoneBandInputs::default()),
        BattleCamPhase::Menu,
        "the command chooser is retail's case 9, not the case-0 close-up"
    );
    assert_eq!(
        phase_for_state(false, true, 0x5A, DoneBandInputs::default()),
        BattleCamPhase::Submenu
    );
    assert_eq!(
        phase_for_state(true, true, 0x5A, DoneBandInputs::default()),
        BattleCamPhase::Dialogue
    );
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

/// The two post-strike states hand the camera to case 7, the multi-cast
/// continuation / idle hold to case 8, and the Done-cleanup pair to
/// whichever case its category fork picks - `FUN_801E295C`'s own arms.
#[test]
fn the_post_strike_states_arm_the_two_shot() {
    let none = DoneBandInputs::default();
    for s in RECOVER_STATES {
        assert_eq!(
            phase_for_state(false, false, s, none),
            BattleCamPhase::Recover,
            "state 0x{s:02X} arms FUN_801D5854 case 7"
        );
    }
    for s in ACTION_END_STATES {
        assert_eq!(
            phase_for_state(false, false, s, none),
            BattleCamPhase::ActionEnd,
            "state 0x{s:02X} arms FUN_801D5854 case 8"
        );
    }
    // The Done-cleanup pair: `0x801E5EA0..0x801E5EF4`'s ladder.
    let attack = DoneBandInputs {
        category: DONE_CATEGORY_ATTACK,
        party_slot: true,
        target_dead: false,
    };
    let run = DoneBandInputs {
        category: DONE_CATEGORY_RUN,
        ..attack
    };
    let monster_spell = DoneBandInputs {
        category: 2,
        party_slot: false,
        target_dead: false,
    };
    let party_over_corpse = DoneBandInputs {
        category: 1,
        party_slot: true,
        target_dead: true,
    };
    let monster_over_corpse = DoneBandInputs {
        party_slot: false,
        ..party_over_corpse
    };
    for s in DONE_STATES {
        assert_eq!(
            phase_for_state(false, false, s, attack),
            BattleCamPhase::ActionEnd
        );
        assert_eq!(phase_for_state(false, false, s, run), BattleCamPhase::Menu);
        assert_eq!(
            phase_for_state(false, false, s, monster_spell),
            BattleCamPhase::Action
        );
        assert_eq!(
            phase_for_state(false, false, s, party_over_corpse),
            BattleCamPhase::ActionEnd
        );
        assert_eq!(
            phase_for_state(false, false, s, monster_over_corpse),
            BattleCamPhase::Action,
            "the dead-target arm is gated on ctx[+0x13] < 3"
        );
    }
    // The far framing still owns the end-of-action gate.
    assert_eq!(
        phase_for_state(false, false, 0x5A, attack),
        BattleCamPhase::Menu
    );
}

/// **Capture pin for the Done band.** `zora_glare_petrify_post` is a
/// retail save parked in `ctx[7] == 0x51` after a monster's spell (slot
/// 3, `ctx[+0xD] = 0`, `ctx[+0x6D0] = 0xC00`, `ctx[+0x6DA] = 1966`,
/// Zora at `+0x34/+0x38 = (649, -47)` facing `3297`). Its camera trio
/// reads pitch `0`, yaw `2735`, `TR (0, 1275, 4820)`, focus the negated
/// `(624, 0, -40)` - a tween one step short of case 6's in-fight pose,
/// nowhere near the far framing (pitch `32`, `TR.z` sized to the
/// formation, focus at the bbox centre; the two share `TR.y = 0x500`).
/// The Done band with a non-attack category is therefore
/// case 6 over the caster, and this is the pose the port targets.
#[test]
fn a_monster_spell_done_tail_reads_the_zora_capture() {
    let done = DoneBandInputs {
        category: 2,
        party_slot: false,
        target_dead: false,
    };
    assert_eq!(
        phase_for_state(false, false, 0x51, done),
        BattleCamPhase::Action
    );
    let zora = BattleCamActor {
        facing: 3297,
        world: [649.0, 0.0, -47.0],
        height: None,
    };
    let f = ActionFraming {
        party_slot: false,
        depth_raw: 0xC00,
        yaw_base: 1966,
        style: 0,
        char_id: 0,
        ..Default::default()
    };
    let pose = action_framing(zora, f);
    assert_eq!(pose.pitch, 0.0);
    assert_eq!(
        pose.yaw,
        ((1966 - 3297) & 0xFFF) as f32,
        "2765, the live 2735 chasing it"
    );
    assert_eq!(pose.tr, [0.0, 0x500 as f32, prescale_tr_z(0xC00)]);
    assert!(
        (pose.tr[2] - 4915.0).abs() < 1.0,
        "4820 in the capture, one step short"
    );
    assert_eq!(pose.focus, [649.0, 0.0, -47.0]);
    // The far framing this band used to take is a different pose on
    // every axis the capture reads.
    let far = menu_framing(
        Some(FormationBox {
            min: [-829.0, -319.0],
            max: [830.0, 319.0],
        }),
        2735.0,
    );
    assert_ne!(far.pitch, pose.pitch, "32 vs 0");
    assert_ne!(far.tr[2], pose.tr[2], "formation-sized vs ctx[+0x6D0]");
    assert_ne!(far.focus, pose.focus, "bbox centre vs the caster");
}

/// The case-8 **death re-frame** is a different pose on every component,
/// and the `ctx[+0x270]` ramp is what separates its two ends.
///
/// Retail literals: `TR.y = 0x300`, `pitch = 0x140`, `TR.z = ctx[+0x6D0]`
/// on the floor arm (`0x801D6B00..0x801D6B14`); `TR.y = 0x300 - r`,
/// `pitch = 0x180 - 3r/2`, `TR.z = ctx[+0x6D0] - 4r` on the ramped one
/// (`0x801D6B50..0x801D6B98`).
#[test]
fn the_death_reframe_ramp_drops_levels_and_pushes_in() {
    let base = BattleCamPose {
        pitch: 0.0,
        yaw: 100.0,
        tr: [0.0, POST_TR_Y, prescale_tr_z(0xC00)],
        focus: [1.0, 2.0, 3.0],
    };

    // Body on the stage floor: the flat pose, and the caller is told to
    // clear the ramp.
    let mut floor = base;
    assert!(apply_death_reframe(&mut floor, 0xC00, 0xC8, 0.0));
    assert_eq!(floor.pitch, DEATH_PITCH_FLAT as f32);
    assert_eq!(floor.tr[1], DEATH_TR_Y as f32);
    assert_eq!(floor.tr[2], prescale_tr_z(0xC00));
    // Yaw and focus are the base pose's - the tail rewrites neither.
    assert_eq!((floor.yaw, floor.focus), (base.yaw, base.focus));

    // Body still falling, ramp at zero: the pitch seed is the *other*
    // constant. `0x180`, not `0x140` - the one place the two arms of the
    // fork disagree at r = 0.
    let mut fresh = base;
    assert!(!apply_death_reframe(&mut fresh, 0xC00, 0, 1.0));
    assert_eq!(fresh.pitch, DEATH_PITCH_BASE as f32);
    assert_eq!(fresh.tr[1], DEATH_TR_Y as f32);
    assert_eq!(fresh.tr[2], prescale_tr_z(0xC00));

    // Saturated ramp: all three tighten together.
    let mut deep = base;
    assert!(!apply_death_reframe(&mut deep, 0xC00, 0xC8, 1.0));
    assert_eq!(deep.pitch, 0x54 as f32, "0x180 - (3 * 0xC8 >> 1)");
    assert_eq!(deep.tr[1], 0x238 as f32, "0x300 - 0xC8");
    assert_eq!(deep.tr[2], prescale_tr_z(0xC00 - 4 * 0xC8));
    assert!(deep.tr[2] < fresh.tr[2], "the camera pushes in");
    assert!(deep.tr[1] < fresh.tr[1], "and drops");
    assert!(deep.pitch < fresh.pitch, "and levels off");
}

/// The ramp reaches the camera only through a **dead** target, and taking
/// it zeroes the yaw ladder - so a death shot does not inherit the
/// swing's accumulated orbit (`sh zero,0x4(t0)`, `t0 = ctx + 0x6D6`).
#[test]
fn only_a_dead_target_reaches_the_death_reframe() {
    let mut cam = BattleCamera::new(BattleCamPhase::ActionEnd, 0);
    cam.set_actor(BattleCamActor::default());
    cam.action_yaw = 0x321;
    cam.attack.ctx.death_ramp = 0xC8;

    // A live target keeps the base framing and the yaw ladder.
    cam.target = Some(PostActionTarget {
        world: [200.0, 400.0, 600.0],
        live: true,
    });
    let (live_pose, live_z) = cam.action_end_pose();
    assert_eq!(cam.action_yaw, 0x321, "the ladder survives a live target");
    assert_eq!(live_z, cam.live_action_framing().depth_raw);
    assert_ne!(live_pose.tr[1], (DEATH_TR_Y - 0xC8) as f32);

    // The same target dead, still above the floor: the ramped pose, and
    // the glide's raw depth follows the `4 * r` the pose took off.
    cam.target = Some(PostActionTarget {
        world: [200.0, 400.0, 600.0],
        live: false,
    });
    let raw = cam.live_action_framing().depth_raw;
    let (dead_pose, dead_z) = cam.action_end_pose();
    assert_eq!(cam.action_yaw, 0, "the death re-frame zeroes the ladder");
    assert_eq!(dead_pose.tr[1], (DEATH_TR_Y - 0xC8) as f32);
    assert_eq!(dead_z, raw - 4 * 0xC8, "glide target follows the pose");
    assert_eq!(cam.attack.ctx.death_ramp, 0xC8, "still falling, not reset");

    // Dropped to the floor: the flat pose, and the ramp is re-zeroed.
    cam.target = Some(PostActionTarget {
        world: [200.0, 0.0, 600.0],
        live: false,
    });
    let (floor_pose, floor_z) = cam.action_end_pose();
    assert_eq!(floor_pose.pitch, DEATH_PITCH_FLAT as f32);
    assert_eq!(floor_z, raw);
    assert_eq!(cam.attack.ctx.death_ramp, 0);
}

/// **Case 7 orbits the midpoint, and that is what keeps both combatants
/// on screen.** The property no other framing in the set has: case 6
/// focuses one actor, case 8 the target, case 9 the formation centre.
#[test]
fn the_recover_framing_orbits_the_pair_not_one_actor() {
    let actor = BattleCamActor {
        facing: 0,
        world: [0.0, 0.0, -800.0],
        height: None,
    };
    let target = PostActionTarget {
        world: [200.0, 0.0, 600.0],
        live: true,
    };
    let f = ActionFraming::default();
    let pose = recover_framing(actor, Some(target), f, 0.0, false);
    assert_eq!(pose.focus, [100.0, 0.0, -100.0], "midpoint of the pair");
    // Case 8 frames the target alone, on the stage floor.
    let end = action_end_framing(actor, Some(target), f, 0.0);
    assert_eq!(end.focus, [200.0, 0.0, 600.0]);
    // A dead / out-of-range target falls back to the acting actor
    // (retail's `0x801D6870` arm).
    let dead = action_end_framing(
        actor,
        Some(PostActionTarget {
            live: false,
            ..target
        }),
        f,
        0.0,
    );
    assert_eq!(dead.focus, [0.0, 0.0, -800.0]);
    // No target at all degenerates case 7 to the acting actor.
    assert_eq!(
        recover_framing(actor, None, f, 0.0, false).focus,
        actor.world
    );
}

/// Case 7's style fork and pull-in tweak, against the arms at
/// `0x801D6698` and `0x801D6780`.
#[test]
fn the_recover_framing_style_and_pull_in_match_the_arms() {
    let actor = BattleCamActor::default();
    let base = ActionFraming {
        depth_raw: 0xC00,
        ..Default::default()
    };
    let plain = recover_framing(actor, None, base, 0.0, false);
    assert_eq!(plain.pitch, 0.0);
    assert_eq!(plain.tr[1], 0x500 as f32);
    assert_eq!(plain.tr[2], prescale_tr_z(0xC00));
    // Style 2: `TR.y = 0x400`, `pitch += 0x80` (`0x801D66E8`).
    let s2 = recover_framing(actor, None, ActionFraming { style: 2, ..base }, 0.0, false);
    assert_eq!(s2.pitch, 0x80 as f32);
    assert_eq!(s2.tr[1], 0x400 as f32);
    // Style 1 adds the half turn to the yaw and leaves the rest.
    let s1 = recover_framing(actor, None, ActionFraming { style: 1, ..base }, 0.0, false);
    assert_eq!(s1.tr[1], 0x500 as f32);
    assert_eq!(
        s1.yaw,
        (plain.yaw as i32 + 0x800).rem_euclid(4096) as f32,
        "style 1 is a half turn"
    );
    // Pull-in (`0x801D6780`): pitch levelled, `TR.y += 0x40`,
    // `TR.z = 3z/5`.
    let pull = recover_framing(actor, None, ActionFraming { style: 2, ..base }, 0.0, true);
    assert_eq!(pull.pitch, 0.0);
    assert_eq!(pull.tr[1], 0x400 as f32 + 0x40 as f32);
    assert_eq!(pull.tr[2], prescale_tr_z(0xC00 * 3 / 5));
}

/// The one-way yaw unwrap both post-action cases run (`0x801D6700` /
/// `0x801D6930`): wrap into 12 bits, then add a full turn when the result
/// would make the tween rotate backwards past the live camera yaw.
#[test]
fn the_post_action_yaw_unwraps_forward_only() {
    // `0 - 0x700` wraps to `0x900`; above a camera yaw of 0, so it stands.
    assert_eq!(unwrap_forward(-0x700, 0.0), 0x900 as f32);
    // Below the live yaw -> a full turn is added rather than rotating back.
    assert_eq!(unwrap_forward(-0x700, 4000.0), (0x900 + 0x1000) as f32);
}

/// The action framing owns the camera **per band**, and the Done band is
/// one of them.
///
/// The band boundaries are `FUN_801E295C`'s own (see
/// [`action_state_frames_the_action`]). `DoneFadeDown` (`0x51`) is
/// retail's bounded action tail (`ctx[+0x6D8]`, seeded `0x3C` at `0x50`),
/// re-armed on case 6 / 8 every pass; the far framing takes over at the
/// end-of-action gate `0x5A`, not before.
#[test]
fn the_done_band_owns_the_action_framing_until_end_of_action() {
    for idle in [0x00u8, 0x0A, 0x0B, 0x5A, 0xFD, 0xFE, 0xFF] {
        assert!(
            !action_state_frames_the_action(idle),
            "state 0x{idle:02X} is idle - the far framing owns it"
        );
    }
    for run in 0x64u8..=0x67 {
        assert!(
            !action_state_frames_the_action(run),
            "Run band 0x{run:02X}: retail arms case 9 + the orbit itself"
        );
    }
    // The action bands DO own it - the seed arm and every attack state
    // re-arm case 6 (`0x801E6464`, `0x801E32E4`).
    for act in [
        0x0Cu8, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1E, 0x1F, 0x28, 0x3C, 0x46, 0x50, 0x51, 0x52,
    ] {
        assert!(
            action_state_frames_the_action(act),
            "state 0x{act:02X} is an action in flight"
        );
    }
}

/// **The residency law the phase script exists to satisfy.** A turn's
/// Done tail is filmed by the per-action framing (case 8 for an attack)
/// and hands back to the far framing at the end-of-action gate, where the
/// formation is on screen and the idle orbit lives.
///
/// The sequence below is a measured `--no-player-battle` turn from the
/// native play-window (`LEGAIA_DIAG_BATCAM`): seed, the attack band, the
/// `DoneFadeDown` tail, then `0x5A` and the next `Begin`. An earlier
/// reading kept the tail on the far framing so that it owned most of the
/// fight's frames; retail's `0x51` arm re-arms case 6 / 8 every pass
/// (`0x801E5FC0..0x801E6018`), and with the in-fight arm right that is a
/// two-shot, not a close-up. What must hold: no tail frame is on the far
/// framing, and every frame from `0x5A` on is.
#[test]
fn a_real_turn_films_its_done_tail_and_hands_back_at_end_of_action() {
    // (state byte, display frames spent there) - one measured turn.
    let turn: [(u8, u32); 10] = [
        (0x00, 2),  // Begin
        (0x0C, 1),  // ActionSeed
        (0x14, 1),  // AttackFace
        (0x16, 24), // AttackAdvance
        (0x17, 1),  // AttackCloseRange
        (0x18, 1),  // AttackStrike
        (0x1E, 2),  // AttackChain
        (0x50, 1),  // DoneCleanup
        (0x51, 48), // DoneFadeDown - the bounded settle
        (0x5A, 2),  // EndOfAction
    ];
    let done = DoneBandInputs {
        category: DONE_CATEGORY_ATTACK,
        party_slot: true,
        target_dead: false,
    };
    let mut slot: Option<BattleCamera> = None;
    let mut frames = 0u64;
    let (mut tail_far, mut tail, mut gate_far, mut gate) = (0u32, 0u32, 0u32, 0u32);
    for _round in 0..2 {
        for (state, dwell) in turn {
            for _ in 0..dwell {
                frames += 1;
                let inputs = BattleCamInputs {
                    target: None,
                    entry_yaw: 0.0,
                    phase: phase_for_state(false, false, state, done),
                    acting: Some(BattleCamActor::default()),
                    formation: Some(traced_formation()),
                    action: ActionFraming::default(),
                    shake_amplitude: 0,
                    attack: None,
                    action_state: state,
                };
                drive(&mut slot, true, inputs, frames, None);
                let far = slot.as_ref().map(|c| c.phase()) == Some(BattleCamPhase::Menu);
                if DONE_STATES.contains(&state) {
                    tail += 1;
                    tail_far += u32::from(far);
                } else if state == 0x5A || state == 0x00 {
                    gate += 1;
                    gate_far += u32::from(far);
                }
            }
        }
    }
    assert_eq!(
        tail_far, 0,
        "the Done tail is a per-action framing ({tail} frames)"
    );
    assert_eq!(
        gate_far, gate,
        "end of action and Begin are the far framing"
    );
    assert_eq!(
        slot.as_ref().unwrap().phase(),
        BattleCamPhase::Menu,
        "the round closes on the far framing"
    );
}

/// **The actor-anchored close-ups must not park the eye inside the
/// actor they frame.** The submenu and action framings are built around
/// the acting actor (`focus = actor.world`), so that actor's own
/// footprint has to stay in front of the eye and its standing height
/// has to fit the frame.
///
/// Scoped to those two deliberately: the dialogue close-up and the far
/// menu framing are anchored on the *formation*, not on the acting
/// actor - the traced dialogue pose (yaw `0`, TR.z `1638`, focus at the
/// origin) genuinely leaves the near party row behind the eye, and the
/// formation framing has its own on-screen sweep above.
///
/// This is the invariant the pose-equality tests structurally cannot
/// see. They compare two hosts' poses to each other; a pose that puts
/// the camera *inside* the geometry is equal on both hosts and passes
/// them both. Here the pose is projected against the world it frames.
#[test]
fn no_resting_framing_puts_the_eye_inside_the_acting_actor() {
    // The actor draw class: BATTLE_WORLD_SCALE + the per-model Y-flip.
    const S: f32 = 4.0;
    let scale_s: [f32; 16] = [
        S, 0.0, 0.0, 0.0, //
        0.0, S, 0.0, 0.0, //
        0.0, 0.0, S, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    let model = mat_mul(&scale_s, &FLIP);
    let actor = BattleCamActor::default(); // seated at the traced (0,0,-800)
    let poses = [
        ("submenu", actor.submenu_pose()),
        ("action", action_framing(actor, ActionFraming::default())),
    ];
    for (name, pose) in poses {
        let vp = battle_vp(&pose, S, 4.0 / 3.0);
        // The ground the acting actor stands on, drawn in the same
        // scaled stage space as the actor itself.
        let foot = project(&vp, &model, actor.world).unwrap_or_else(|| {
            panic!("{name}: the acting actor's own footprint is behind the eye")
        });
        // ... and its head, so a camera parked inside the body (the
        // footprint in front but the whole mesh wrapped around the
        // lens) is caught too.
        let head = project(&vp, &model, [actor.world[0], -370.0, actor.world[2]])
            .unwrap_or_else(|| panic!("{name}: the acting actor's head is behind the eye"));
        // A standing character spans a bounded share of the 240-line
        // frame; more than a full frame means the eye is inside it.
        let span = (foot.1 - head.1).abs();
        assert!(
            span < 240.0,
            "{name}: the acting actor spans {span} of 240 scanlines - the eye is inside it"
        );
    }
}

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
    let (pose, raw_z) = summon_cast_framing(actor, 649, 0xC8);
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
    let (want, _) = summon_cast_framing(cam.actor, c.accum, c.ramp);
    let got = cam.framing_pose();
    assert_eq!(got.pitch, want.pitch);
    assert!(
        (got.tr[1] - want.tr[1]).abs() <= 32.0,
        "{got:?} vs {want:?}"
    );
}
