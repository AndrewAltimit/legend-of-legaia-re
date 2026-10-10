//! The command-menu and submenu framings, the per-seat close-up and the cursor shots.

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

/// The ring passes from one member to the next with no far framing in
/// between (a commit lands on the next member's `0x28`), so the close-up has
/// to follow the member now commanding: `FUN_801D388C` re-arms case 0 with
/// `ctx[+0x13]`. Before the re-arm the phase stayed `Submenu` across the
/// hand-off and the camera stayed on Vahn while Noa and Gala chose.
#[test]
fn the_close_up_follows_the_ring_to_the_next_member() {
    let vahn = BattleCamActor {
        facing: 0,
        height: Some(1152.0),
        world: [0.0, 0.0, -800.0],
    };
    let noa = BattleCamActor {
        facing: 512,
        height: Some(960.0),
        world: [-700.0, 0.0, -900.0],
    };
    let mut cam = traced_cam(BattleCamPhase::Menu);
    cam.set_actor(vahn);
    cam.set_phase(BattleCamPhase::Submenu);
    steps(&mut cam, SUBMENU_ENTER_STEPS as u64);
    assert_eq!(cam.pose().focus, vahn.world);
    // Vahn commits; Noa's ring opens on the same phase.
    cam.set_actor(noa);
    cam.set_phase(BattleCamPhase::Submenu);
    steps(&mut cam, 3);
    let mid = cam.pose().focus;
    assert!(mid[0] < 0.0 && mid[0] > -700.0, "pans, not cuts: {mid:?}");
    steps(&mut cam, SUBMENU_ENTER_STEPS as u64 - 3);
    let want = noa.submenu_pose();
    assert_eq!(cam.pose().focus, want.focus);
    assert_eq!(cam.pose().tr, want.tr);
    assert_eq!(cam.pose().yaw.rem_euclid(4096.0), want.yaw);
    // Re-supplying the same member every frame does not restart the glide.
    for _ in 0..4 {
        cam.set_actor(noa);
        steps(&mut cam, 1);
    }
    assert_eq!(cam.pose().focus, want.focus);
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

/// **The round prompt keeps the far framing.**
///
/// The retail battle menu driver `FUN_801D388C` arms case `0` *and* case
/// `9`, so "a menu is open" does not select the close-up on its own; the
/// retail captures separate them (a Begin/Run save reads case 9's
/// `TR (0, 1280, 7680)` over `+-800` seats, a ring or arts-input save reads
/// case 0's `TR (-512, 1152, 2457)`). Folding the round prompt into
/// `Submenu` put the enemy **behind the eye** while the party chose.
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

/// Case 1 with the target dead ahead on the seat axis is the swing pose the
/// solo-Tetsu camera trace measures: pitch `256`, yaw `0`,
/// `TR (0, 1536, prescale(0x800))`, focused on the member.
#[test]
fn the_enemy_cursor_shot_on_the_seat_axis_is_the_traced_swing() {
    let member = BattleCamActor {
        facing: 0,
        world: [0.0, 0.0, -800.0],
        height: None,
    };
    let p = target_enemy_pose(member, [0.0, 800.0], member.world);
    assert_eq!(p.pitch, 256.0);
    assert_eq!(p.yaw, 0.0);
    assert_eq!(p.tr, [0.0, 1536.0, prescale_tr_z(0x800)]);
    assert_eq!(p.focus, member.world);
}

/// Case 3 is case 0 turned onto the targeted member: `TR.x = 0` and yaw base
/// `0x900` (`0x801D5BD4..0x801D5C54`).
#[test]
fn the_ally_cursor_shot_is_the_close_up_on_the_target() {
    let ally = BattleCamActor {
        facing: 0x100,
        world: [700.0, 0.0, -800.0],
        height: Some(1000.0),
    };
    let p = target_ally_pose(ally);
    assert_eq!(p.pitch, 32.0);
    assert_eq!(p.yaw, (0x900 - 0x100) as f32);
    assert_eq!(p.tr, [0.0, 1000.0, prescale_tr_z(0x600)]);
    assert_eq!(p.focus, ally.world);
}

/// The cursor framings glide in over 6 camera steps and hand back to the
/// far framing over case 9's 7 when the cursor commits.
#[test]
fn the_cursor_shot_glides_in_and_hands_back_to_the_far_framing() {
    let member = BattleCamActor::default();
    let cursor = Some(CursorFraming::Enemy {
        target: [0.0, 800.0],
    });
    let mut slot = None;
    let mut frames = 0u64;
    let mut run = |slot: &mut Option<BattleCamera>, phase, cursor, n: u32| {
        for _ in 0..n {
            frames += 2;
            drive(
                slot,
                true,
                BattleCamInputs {
                    phase,
                    acting: Some(member),
                    cursor,
                    ..Default::default()
                },
                frames,
                None,
            );
        }
    };
    run(&mut slot, BattleCamPhase::Submenu, None, 10);
    run(&mut slot, BattleCamPhase::TargetEnemy, cursor, 6);
    let cam = slot.as_ref().unwrap();
    assert_eq!(cam.phase(), BattleCamPhase::TargetEnemy);
    let want = target_enemy_pose(member, [0.0, 800.0], member.world);
    let p = cam.pose();
    assert_eq!((p.pitch, p.tr, p.focus), (want.pitch, want.tr, want.focus));
    assert_eq!(p.yaw.rem_euclid(4096.0), want.yaw);
    run(&mut slot, BattleCamPhase::Menu, None, 7);
    let p = slot.as_ref().unwrap().pose();
    assert_eq!(p.pitch, 32.0, "back on the far framing's pitch");
}
