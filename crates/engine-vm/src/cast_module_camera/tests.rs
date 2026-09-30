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
