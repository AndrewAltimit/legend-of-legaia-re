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
        caster_monster: 0,
        action: 0,
        depth_raw: 0,
        caster_latch: 0,
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

/// The settle arm frames the caster through case 8 on every pass, held or
/// not (`jal 0x801D5854` with `a1 = 8` at `0x801F761C`, ahead of the
/// countdown) - the call whose dead-target arm zeroes the yaw ladder.
#[test]
fn gimard_settle_arm_frames_case_eight_every_pass() {
    let mut st = ModuleCamState::default();
    let s = seats();
    st.countdown.add(192);
    let d = gimard_direct(&mut st, 12, s);
    assert!(d.hold, "holds on the countdown");
    assert!(d.end_frame);
    st.countdown.0 = 0;
    let d = gimard_direct(&mut st, 12, s);
    assert!(!d.hold);
    assert!(d.end_frame);
    // No other arm calls case 8.
    for arm in 0..12 {
        let mut st = ModuleCamState::default();
        assert!(!gimard_direct(&mut st, arm, s).end_frame, "arm {arm}");
    }
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
    assert!(module_profile(905).is_some_and(|p| p.stages_spawns));
}

/// PROT 0905 reports its spawn calls on the arms that make them: four glows
/// on the framed point in arm 0, three along the target's heading in arm 4,
/// two off the seated creature in arm 5, two on the creature in arm 8 and
/// five on the restored target in arm 9.
#[test]
fn vera_reports_its_spawns_on_their_arms() {
    let mut st = ModuleCamState::default();
    let mut phase = 0u8;
    let mut seen = Vec::new();
    for _ in 0..4000u32 {
        let d = vera_direct(&mut st, phase, seats());
        if !d.spawns.is_empty() {
            seen.push((phase, d.spawns.len()));
        }
        if !d.hold {
            phase += if phase == 5 { 3 } else { 1 };
            if phase > 10 {
                break;
            }
        }
    }
    assert_eq!(seen, vec![(0, 4), (4, 3), (5, 2), (8, 2), (9, 5)]);
    // Arm 0's glows sit on the framed point at `y = -0x280`, unturned.
    let s = seats();
    let shot = vera_direct(&mut ModuleCamState::default(), 0, s).shot;
    let (pos, rot) =
        spawn_anchor_point(VERA_OPEN_SPAWNS[0].anchor, None, Some(s.victim), shot).unwrap();
    let f = shot.unwrap().focus;
    assert_eq!(pos, [-f[0], -0x280, -f[2]]);
    assert_eq!(rot, [0; 3]);
}

/// Walk any director to its park, returning `(passes per arm, shots by arm)`.
fn walk_director(
    direct: ModuleDirector,
    seats: ModuleCamSeats,
) -> (Vec<u32>, Vec<(u8, ModuleShot)>) {
    let mut st = ModuleCamState::default();
    let mut phase = 0u8;
    let mut ticks_in_arm = vec![0u32];
    let mut shots = Vec::new();
    for _ in 0..4000 {
        let d = direct(&mut st, phase, seats);
        if d.park {
            return (ticks_in_arm, shots);
        }
        if let Some(s) = d.shot {
            shots.push((phase, s));
        }
        *ticks_in_arm.last_mut().unwrap() += 1;
        if !d.hold {
            phase += 1;
            ticks_in_arm.push(0);
        }
    }
    panic!("director never parked");
}

/// The six summon-creature directors read off PROT 0916 / 0921 / 0929 /
/// 0932 / 0933 / 0934 are registered as camera-only and each parks.
#[test]
fn summon_creature_directors_are_camera_only_and_park() {
    for (entry, park) in [(916, 5), (921, 5), (929, 3), (932, 6), (933, 5), (934, 5)] {
        let p = module_profile(entry).expect("registered");
        assert!(p.owns_phase && p.hit_arm.is_none(), "PROT {entry}");
        let (arms, _) = walk_director(p.direct, seats());
        assert_eq!(arms.len() - 1, park, "PROT {entry} parks at arm {park}");
    }
}

/// Terra's arm 3 / 4 gates drain the `0x100` its arm 2 arms by one frame
/// delta a tick and re-arm `0x20` / `0x60` on what is left.
#[test]
fn terra_counts_its_rise_out_and_cuts_on_arm_4() {
    let (arms, shots) = walk_director(terra_direct, seats());
    assert_eq!(arms[3], 0x100);
    assert_eq!(arms[4], 0x20);
    let arm2 = shots.iter().find(|(a, _)| *a == 2).unwrap().1;
    assert_eq!(
        (arm2.angles, arm2.tr, arm2.frames),
        ([0xA0, 0x800, 0], [0, 0x700, 0], 0x140)
    );
    assert!(shots.iter().any(|(a, s)| *a == 4 && s.tr == [0, 0x700, 0]));
}

/// Meta's arm 1 swings the camera every pass and lets go once its `0xC0`
/// word falls below `0x41`: `0xC0 - 0x40` passes.
#[test]
fn meta_swings_until_its_word_falls_below_0x41() {
    let mut st = ModuleCamState::default();
    let _ = meta_direct(&mut st, 0, seats());
    let mut holds = 0;
    loop {
        let d = meta_direct(&mut st, 1, seats());
        let n = d.nudge.expect("every pass swings");
        assert_eq!((n.pitch, n.tr_y, n.tr_z), (-4, 12, -12));
        if !d.hold {
            break;
        }
        holds += 1;
    }
    assert_eq!(holds, 0xC0 - 0x41);
    let (_, shots) = walk_director(meta_direct, seats());
    let arm4 = shots.iter().find(|(a, _)| *a == 4).unwrap().1;
    assert_eq!(arm4.tr, [-0x5C8, 0x600, -0x1000]);
}

/// Aluru frames a point half a unit off the victim along the reversed
/// victim-to-caster heading, then seats its creature there and frames it.
#[test]
fn aluru_frames_the_victim_then_its_creature() {
    let s = seats();
    let (arms, shots) = walk_director(aluru_direct, s);
    let h = heading(s.victim, s.caster).wrapping_add(0x800) & 0xFFF;
    let (sin, cos) = trig12(h);
    let arm0 = shots[0].1;
    assert_eq!(arm0.angles, [-0x80, yaw_from(0x800, h), 0]);
    assert_eq!(arm0.focus[0], (half(sin) - i32::from(s.victim.x)) as i16);
    assert_eq!(arm0.focus[2], (half(cos) - i32::from(s.victim.z)) as i16);
    // The band timer `scalar * 20` drains `scalar` a tick under `bgez`.
    assert_eq!(arms[4], 21);
    let arm4 = shots.iter().find(|(a, _)| *a == 4).unwrap().1;
    assert_eq!(arm4.frames, 0x80);
    assert_eq!(arm4.focus[0], (half(sin) - i32::from(s.victim.x)) as i16);
}

/// Iota pulls TR z in by the drain while its word runs and only passes arm
/// 2 / 3 once it is spent.
#[test]
fn iota_pulls_in_while_its_countdown_runs() {
    let mut st = ModuleCamState::default();
    let _ = iota_direct(&mut st, 0, seats());
    let a1 = iota_direct(&mut st, 1, seats());
    assert!(!a1.hold);
    assert_eq!(a1.nudge.unwrap().tr_z, -MODULE_DRAIN_PER_TICK as i16);
    let (arms, shots) = walk_director(iota_direct, seats());
    // Arm 2: `scalar << 6` less the arm-1 drain, a drain a tick, then the
    // passing tick.
    assert_eq!(arms[2], (64 - 1) + 1);
    assert!(
        shots
            .iter()
            .any(|(a, s)| *a == 3 && s.tr == [0, 0x400, 0x4E20])
    );
    assert!(shots.iter().any(|(a, s)| *a == 4 && s.frames == 0x118));
}

/// Mule and Ozma wait on the band timer at their CD poll.
#[test]
fn mule_and_ozma_wait_on_the_band_timer() {
    let mut s = seats();
    s.band_timer = 5;
    let mut st = ModuleCamState::default();
    assert!(mule_direct(&mut st, 2, s).hold);
    assert!(ozma_direct(&mut st, 3, s).hold);
    s.band_timer = 0;
    assert!(!mule_direct(&mut st, 2, s).hold);
    assert!(!ozma_direct(&mut st, 3, s).hold);
}

/// Glare frames the caster, holds `scalar << 8` with a pull-in, cuts to the
/// victim, then holds `scalar << 7` twice more and finishes.
#[test]
fn glare_frames_caster_then_victim_and_finishes_on_arm_3() {
    let mut st = ModuleCamState::default();
    let mut s = seats();
    s.caster_monster = 0xA9;
    let a0 = capture_camera_director(940, GLARE_BODY).unwrap()(&mut st, 0, s);
    assert_eq!(a0.shot.unwrap().tr, [0, 0x600, 0x800]);
    assert_eq!(a0.next, Some(1));
    let mut phase = 1u8;
    let mut held = [0i32; 4];
    let mut victim_cut = false;
    for _ in 0..10_000 {
        let a = glare_camera(&mut st, phase, s);
        if a.hold {
            held[phase as usize] += 1;
            assert!(a.drift.is_some(), "every pass drifts");
            continue;
        }
        if phase == 1 {
            victim_cut = a.shot.is_some_and(|sh| sh.focus == focus_on(s.victim));
        }
        match a.next {
            Some(n) => phase = n,
            None => break,
        }
    }
    assert!(victim_cut);
    assert_eq!(held[1], SPEED_SCALAR * 256 / MODULE_DRAIN_PER_TICK - 1);
    assert_eq!(held[2], SPEED_SCALAR * 128 / MODULE_DRAIN_PER_TICK - 1);
    assert_eq!(held[3], held[2]);
}

/// Walk a phase-owning capture director to its finish, returning the phase
/// sequence it passed through and every arm's output.
fn walk_capture(direct: CaptureCamDirector, s: ModuleCamSeats) -> (Vec<u8>, Vec<CaptureCamArm>) {
    let mut st = ModuleCamState::default();
    let mut phase = 0u8;
    let mut seq = vec![0u8];
    let mut arms = Vec::new();
    for _ in 0..20_000 {
        let a = direct(&mut st, phase, s);
        arms.push(a);
        if a.hold {
            continue;
        }
        match a.next {
            Some(n) => {
                phase = n;
                seq.push(n);
            }
            None => return (seq, arms),
        }
    }
    panic!("never finished");
}

/// Terio Punch forks on the caster's latch: clear charges (sets it, no
/// damage, done at arm 2); set punches (clears it, arms 4..8).
#[test]
fn terio_punch_charges_then_punches_on_its_latch() {
    let mut s = seats();
    s.depth_raw = 0x600;
    let (seq, arms) = walk_capture(terio_punch_camera, s);
    assert_eq!(seq, vec![0, 1, 2]);
    assert_eq!((arms[0].latch, arms[0].skips_fold), (Some(1), true));
    assert_eq!(arms[0].shot.unwrap().tr, [0, 0x600, 0x300]);
    assert_eq!(arms[1].shot.unwrap().tr, [0, 0, 0xC00]);

    s.caster_latch = 1;
    let (seq, arms) = walk_capture(terio_punch_camera, s);
    assert_eq!(seq, vec![0, 4, 5, 6, 7, 8]);
    assert_eq!((arms[0].latch, arms[0].skips_fold), (Some(0), false));
    let shots: Vec<_> = arms.iter().filter_map(|a| a.shot).collect();
    assert_eq!(shots[0].angles[0], 0x20);
    assert_eq!(shots[1].tr, [0x200, 0xA00, 0x200]);
    assert_eq!(shots.last().unwrap().tr, [0, 0x550, 0x2800]);
    assert!(capture_camera_director(953, SINGLE_BODY).is_some());
}

/// Final Crisis in formation `0xB5`: arm 0 cuts behind Cort at `TR.y 0x800`,
/// arm 1 drifts out and up a vsync at a time and holds on the countdown
/// (`scalar * 0xC0`, 192 vsyncs), then cuts low. Sixteen vsyncs of arm 1 is
/// the `cort_evolved_final_crisis_mid_cast` capture's pitch `16`, TR y
/// `0x800 - 32` and TR z `+128`.
#[test]
fn final_crisis_cuts_behind_cort_and_drifts_out() {
    let mut st = ModuleCamState::default();
    let seats = ModuleCamSeats {
        caster: ModuleSeat {
            x: 0,
            y: 0,
            z: 1024,
            facing: 0x800,
        },
        first_monster: 0xB5,
        depth_raw: 0x9F0,
        ..Default::default()
    };
    assert_eq!(
        capture_camera_director(961, DEAD_END_CRISIS_BODY).map(|f| f as usize),
        Some(dead_end_crisis_camera as CaptureCamDirector as usize)
    );
    assert_eq!(capture_countdown_va(0xB4), Some(DEAD_END_CRISIS_COUNTDOWN));
    let a0 = dead_end_crisis_camera(&mut st, 0, seats);
    let cut = a0.shot.expect("arm 0 cuts");
    assert_eq!(
        (cut.angles, cut.tr, cut.focus),
        ([0, 0, 0], [0, 0x800, 0x9F0], [0, 0, -1024])
    );
    assert!(!a0.hold);
    let (mut pitch, mut tr_y, mut tr_z) = (0i32, 0i32, 0i32);
    for _ in 0..16 {
        let a = dead_end_crisis_camera(&mut st, 1, seats);
        assert!(a.hold && a.shot.is_none());
        let d = a.drift.unwrap();
        pitch += i32::from(d.pitch);
        tr_y += i32::from(d.tr_y);
        tr_z += i32::from(d.tr_z);
    }
    assert_eq!((pitch, tr_y, tr_z), (16, -32, 128));
    let mut passed = None;
    for _ in 0..400 {
        let a = dead_end_crisis_camera(&mut st, 1, seats);
        if !a.hold {
            passed = a.shot;
            break;
        }
    }
    let low = passed.expect("arm 1 passes");
    assert_eq!((low.angles[0], low.tr), (-0x70, [0, 0xA20, 0x9F0 - 0x280]));
    dead_end_crisis_camera(&mut st, 0xFF, seats);
    assert_eq!(st.yaw_base, 0x780);
}

/// A capture body's finishing arm leaves the yaw counter at `0x780`: Zora's
/// Glare (PROT 0940 `0x801F69F8`, `0x801F7208`) does, Ultra Charge (PROT
/// 0962) does not, and the trampoline-less Evil Seru Magic is keyed on
/// [`SINGLE_BODY`].
#[test]
fn capture_bodies_that_finish_on_the_0x780_yaw_counter() {
    assert_eq!(capture_exit_yaw_base(940, 0x801F_69F8), Some(0x780));
    assert_eq!(capture_exit_yaw_base(964, 0x801F_88EC), Some(0x780));
    assert_eq!(capture_exit_yaw_base(966, SINGLE_BODY), Some(0x780));
    assert_eq!(capture_exit_yaw_base(962, ULTRA_CHARGE_BODY), None);
    assert_eq!(capture_exit_yaw_base(959, 0x801F_69F0), None);
}
