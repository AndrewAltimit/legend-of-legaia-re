use super::*;

fn seats() -> ModuleCamSeats {
    ModuleCamSeats {
        caster: ModuleSeat {
            x: 82,
            y: 0,
            z: -542,
            facing: 0,
        },
        victim: ModuleSeat {
            x: 0,
            y: 0,
            z: 800,
            facing: 0x800,
        },
        band_timer: 0,
        first_monster: 0,
        action: 0,
        depth_raw: 0,
    }
}

/// Walk a director through its arms, running the phase chain's advance
/// on every pass the way the engine seam does. Returns the tick each arm
/// was first passed on, and every shot with the tick it was armed.
fn walk(last: u8) -> (Vec<u32>, Vec<(u32, u8, ModuleShot)>) {
    let mut st = ModuleCamState::default();
    let mut phase = 0u8;
    let mut passed = Vec::new();
    let mut shots = Vec::new();
    for tick in 0..4000u32 {
        let d = gimard_direct(&mut st, phase, seats());
        if let Some(s) = d.shot {
            shots.push((tick, phase, s));
        }
        if !d.hold {
            passed.push(tick);
            phase += 1;
            if phase > last {
                break;
            }
        }
    }
    (passed, shots)
}

#[test]
fn gimard_arm_timing_follows_the_countdown() {
    let (passed, shots) = walk(10);
    // Arms 0 and 1 pass on consecutive ticks; arm 2 drains `scalar << 6`
    // at `scalar` a tick.
    assert_eq!(passed[0], 0);
    assert_eq!(passed[1], 1);
    assert_eq!(passed[2] - passed[1], 64);
    // Arm 4 holds until the word falls to `scalar << 5`, arm 5 until it
    // is spent: half each.
    assert_eq!(passed[4] - passed[3], 32);
    assert_eq!(passed[5] - passed[4], 32);
    // Arm 6 waits out the caption's `scalar * 180`.
    assert_eq!(passed[6] - passed[5], 180);
    // Five shots: arms 0, 1, 4, 6, 7.
    let arms: Vec<u8> = shots.iter().map(|s| s.1).collect();
    assert_eq!(arms, vec![0, 1, 4, 6, 7]);
    // Arm 7's pan is the long one.
    assert_eq!(shots[4].2.frames, 0xC0);
}

#[test]
fn gimard_walk_arm_follows_the_creature_until_it_arrives() {
    let mut st = ModuleCamState::default();
    let s = seats();
    st.creature_live = Some(s.caster);
    let d = gimard_direct(&mut st, 10, s);
    assert!(!d.hold);
    assert_eq!(st.yaw_base, GIMARD_WALK_YAW_BASE);
    let d = gimard_direct(&mut st, 11, s);
    assert!(d.hold, "holds on the range poll");
    let f = d.follow.expect("case 6 on the creature");
    assert_eq!(f.depth_raw, GIMARD_WALK_DEPTH);
    assert_eq!(f.yaw_base, GIMARD_WALK_YAW_BASE + 6 * MODULE_DRAIN_PER_TICK);
    st.creature_arrived = true;
    let d = gimard_direct(&mut st, 11, s);
    assert!(!d.hold);
    assert!(d.follow.is_some());
}

#[test]
fn gimard_creature_shots_frame_the_placed_creature() {
    let mut st = ModuleCamState::default();
    let s = seats();
    let _ = gimard_direct(&mut st, 3, s);
    let c = st.creature.expect("arm 3 places the creature");
    let h = (heading(s.victim, s.caster) + 0x800) & 0xFFF;
    assert_eq!(c.facing, h);
    // Toward the caster from the victim, half a unit out.
    assert!(c.z < s.victim.z);
    st.countdown.0 = 0;
    let d = gimard_direct(&mut st, 6, s);
    let shot = d.shot.expect("arm 6 snaps");
    assert_eq!(shot.focus, [c.x.wrapping_neg(), 0, c.z.wrapping_neg()]);
    assert_eq!(shot.angles[1], ((0x880 - i32::from(h)) & 0xFFF) as i16);
    let (pose, raw_z) = shot.pose();
    assert_eq!(raw_z, 0x400);
    assert_eq!(pose.focus, [f32::from(c.x), 0.0, f32::from(c.z)]);
}

#[test]
fn camera_only_directors_park_past_their_arms() {
    for entry in [914, 915, 917, 920, 923, 928, 930, 931] {
        let p = module_profile(entry).expect("directed");
        assert!(!p.paces_band(), "PROT {entry} is camera-only");
        let mut st = ModuleCamState::default();
        let d = (p.direct)(&mut st, 0, seats());
        assert!(d.shot.is_some(), "PROT {entry} cuts at arm 0");
        // Walk until it parks; every covered arm lets the phase through
        // eventually.
        let mut phase = 0u8;
        for _ in 0..4096 {
            let d = (p.direct)(&mut st, phase, seats());
            if d.park {
                break;
            }
            if !d.hold {
                phase += 1;
            }
        }
        assert!((1..8).contains(&phase), "PROT {entry} parked at {phase}");
    }
}

#[test]
fn barra_climbs_until_tr_y_reaches_0x800() {
    let mut st = ModuleCamState::default();
    let _ = barra_direct(&mut st, 0, seats());
    let mut holds = 0;
    while barra_direct(&mut st, 1, seats()).hold {
        holds += 1;
    }
    assert_eq!(holds, (0x800 - 0x400) / MODULE_DRAIN_PER_TICK);
}

/// Mystic Shield's arm 0 frames the caster from behind and seeds the
/// countdown; arm 1 holds on it and re-arms `scalar << 8` as it passes. The
/// `cort_mystic_shield_mid_cast` capture's word (`496` left in arm 1) is
/// `34` vsync drains past arm 0's `scalar * 0x60` seed.
#[test]
fn mystic_shield_arms_its_shot_and_gates_on_its_countdown() {
    let mut st = ModuleCamState::default();
    let seats = ModuleCamSeats {
        caster: ModuleSeat {
            x: 0,
            y: 0,
            z: 813,
            facing: 0x800,
        },
        ..Default::default()
    };
    let arm0 = mystic_shield_camera(&mut st, 0, seats);
    let shot = arm0.shot.expect("arm 0 shoots");
    assert_eq!(shot.angles, [0x200, 0, 0]);
    assert_eq!(shot.tr, [0, -0x100, 0x400]);
    assert_eq!(shot.focus, [0, 0, -813]);
    assert_eq!(shot.frames, 0x40);
    assert!(!arm0.hold);
    for _ in 0..34 {
        assert!(mystic_shield_camera(&mut st, 1, seats).hold);
    }
    assert_eq!(st.countdown.0, 496);
    let drift = mystic_shield_camera(&mut st, 4, seats).drift.unwrap();
    assert_eq!((drift.yaw, drift.tr_z), (-16, -6));
    assert_eq!(capture_countdown_va(0xAC), Some(MYSTIC_SHIELD_COUNTDOWN));
}

/// Guilty Cross's arm 0 frames from behind the caster; arm 2's exit cuts to
/// the victim; arm 3 drifts eight times as fast as arms 1 / 2 / 5.
#[test]
fn guilty_cross_cuts_from_caster_to_victim() {
    let mut st = ModuleCamState::default();
    let seats = ModuleCamSeats {
        caster: ModuleSeat {
            x: 0,
            y: 0,
            z: 813,
            facing: 2285,
        },
        victim: ModuleSeat {
            x: -600,
            y: 0,
            z: -762,
            facing: 0,
        },
        ..Default::default()
    };
    let a0 = guilty_cross_camera(&mut st, 0, seats).shot.unwrap();
    assert_eq!(a0.angles[1], ((0x800 - 2285) & 0xFFF) as i16);
    assert_eq!((a0.tr, a0.frames), ([0, 0x600, 0x800], 0x20));
    st.countdown.0 = 1;
    let a2 = guilty_cross_camera(&mut st, 2, seats);
    assert!(!a2.hold);
    let cut = a2.shot.unwrap();
    assert_eq!(
        (cut.tr, cut.focus, cut.frames),
        ([0, 0x400, 0xA00], [600, 0, 762], 1)
    );
    assert_eq!(
        guilty_cross_camera(&mut st, 3, seats).drift.unwrap().tr_z,
        32
    );
}

/// Big Wave runs on past its pan: arm 5 hands to arm 6's cut (pitch
/// `0x180`, yaw `0xF00 - facing`, `TR.z = 3z/2`), arms 7 and 8 spin the yaw
/// a quarter scalar a vsync through their gates, and arm 8 finishes.
#[test]
fn big_wave_arms_6_to_8_cut_spin_and_finish() {
    let mut st = ModuleCamState::default();
    let seats = ModuleCamSeats {
        caster: ModuleSeat {
            x: 0,
            y: 0,
            z: 813,
            facing: 0x800,
        },
        action: 0x56,
        depth_raw: 0xA80,
        ..Default::default()
    };
    st.countdown.0 = 1;
    let a5 = wave_camera(&mut st, 5, seats);
    assert_eq!((a5.hold, a5.next), (false, Some(6)));
    assert_eq!(
        st.countdown.0,
        1 - MODULE_DRAIN_PER_TICK + SPEED_SCALAR * 0xC0
    );
    st.countdown.0 = 1;
    let a6 = wave_camera(&mut st, 6, seats);
    assert_eq!(a6.next, Some(7));
    let shot = a6.shot.expect("arm 6 cuts");
    assert_eq!(shot.angles, [0x180, 0x700, 0]);
    assert_eq!(shot.tr, [0, 0x600, 0xA80 * 3 / 2]);
    let a7 = wave_camera(&mut st, 7, seats);
    assert!(a7.hold);
    assert_eq!(a7.drift.unwrap().yaw, (SPEED_SCALAR / 4) as i16);
    st.countdown.0 = 1;
    assert_eq!(wave_camera(&mut st, 7, seats).next, Some(8));
    st.countdown.0 = 1;
    let a8 = wave_camera(&mut st, 8, seats);
    assert_eq!((a8.hold, a8.next), (false, None));
    assert!(a8.drift.is_some());
}

/// PROT 0903 reports its own calls on the arms that make them: the creature
/// spawns in arm 3, the spell name in 5, the tunnel and the attack name in
/// 6, the breath and its CLUT move in 8 - and nothing else anywhere.
#[test]
fn gimard_reports_its_spawns_and_captions_on_their_arms() {
    let mut st = ModuleCamState::default();
    let mut phase = 0u8;
    let mut seen = Vec::new();
    for _ in 0..4000u32 {
        let d = gimard_direct(&mut st, phase, seats());
        if !d.spawns.is_empty() || d.caption.is_some() || d.vram_move.is_some() {
            seen.push((phase, d.spawns.len(), d.caption, d.vram_move.is_some()));
        }
        if !d.hold {
            phase += 1;
            if phase > 10 {
                break;
            }
        }
    }
    assert_eq!(
        seen,
        vec![
            (3, 3, None, false),
            (5, 0, Some(ModuleCaption::SpellName), false),
            (6, 3, Some(ModuleCaption::AttackName), false),
            (8, 1, None, true),
        ]
    );
    assert!(module_profile(903).is_some_and(|p| p.stages_spawns));
    assert!(!module_profile(905).is_some_and(|p| p.stages_spawns));
}
