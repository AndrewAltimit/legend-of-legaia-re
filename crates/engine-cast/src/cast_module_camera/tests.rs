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
        victim_radius: 0,
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
fn slippery_director_walks_the_whole_chain_to_0xff() {
    let p = module_profile(920).expect("directed");
    assert!(p.paces_band() && p.director_body && p.owns_phase);
    assert_eq!(p.hit_arm, Some(SLIPPERY_DAMAGE_ARM));
    let mut st = ModuleCamState::default();
    let mut phase = 0u8;
    let mut dwell = std::collections::BTreeMap::<u8, u32>::new();
    for _ in 0..10_000 {
        if phase == crate::seru_magic_ticks::SERU_B_DONE_PHASE {
            break;
        }
        let d = slippery_direct(&mut st, phase, seats());
        *dwell.entry(phase).or_default() += 1;
        if !d.hold {
            phase = d.jump.unwrap_or(phase + 1);
        }
    }
    assert_eq!(phase, crate::seru_magic_ticks::SERU_B_DONE_PHASE);
    let got: Vec<u32> = (3..=10).map(|a| dwell[&a]).collect();
    assert_eq!(got, [64, 128, 128, 256, 256, 32, 128, 128]);
    // Arm 0xFF holds: the director never walks past the done phase.
    assert!(slippery_direct(&mut st, 0xFF, seats()).hold);
}

/// PROT 0920's arm 7 pans TR x by the scalar a pass and its exit cuts back
/// onto the creature.
#[test]
fn slippery_arm_seven_pans_then_cuts_on_the_creature() {
    let mut st = ModuleCamState::default();
    let creature = ModuleSeat {
        x: 63,
        y: 0,
        z: -591,
        facing: 13,
    };
    st.creature_live = Some(creature);
    st.countdown.0 = 2 * MODULE_DRAIN_PER_TICK;
    let d = slippery_direct(&mut st, 7, seats());
    assert!(d.hold);
    assert_eq!(d.nudge.unwrap().tr_x, -(MODULE_DRAIN_PER_TICK as i16));
    let d = slippery_direct(&mut st, 7, seats());
    let shot = d.shot.expect("arm 7 cuts");
    assert_eq!(shot.angles, [0, 0x800 - 13, 0]);
    assert_eq!(shot.tr, [0, 0x400, 0x600]);
    assert_eq!(shot.focus, [-63, 0, 591]);
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
    // ...but the fourth, the dim star, is rolled an eighth of a turn so its
    // rays fall between the third's.
    let (pos4, rot4) =
        spawn_anchor_point(VERA_OPEN_SPAWNS[3].anchor, None, Some(s.victim), shot).unwrap();
    assert_eq!(pos4, pos);
    assert_eq!(rot4, [0, 0, 0x200]);
}

/// Every summon-creature module's director carries its whole chain: no arm
/// parks, it cuts at arm 0, it reaches the done phase with the row settled
/// and the band timer spent, and it passes its hit arm on the way.
#[test]
fn every_summon_creature_director_walks_its_chain_to_0xff() {
    for entry in [
        914, 915, 916, 917, 921, 923, 928, 929, 930, 931, 932, 933, 934,
    ] {
        let p = module_profile(entry).expect("directed");
        assert!(
            p.director_body && p.paces_band() && p.owns_phase,
            "PROT {entry} carries its chain"
        );
        let hit = p.hit_arm.expect("a hit arm");
        let st = ModuleCamState {
            row_settled: true,
            victim_settled: true,
            ..Default::default()
        };
        let mut first = st;
        assert!(
            (p.direct)(&mut first, 0, seats()).shot.is_some(),
            "PROT {entry} cuts at arm 0"
        );
        let (dwell, _, _, _) = walk_body(p.direct, seats(), st);
        assert!(
            dwell.contains_key(&hit),
            "PROT {entry} runs its hit arm {hit:#x}"
        );
        let total: u32 = dwell.values().sum();
        assert!(total > 200, "PROT {entry} lasts {total} ticks");
    }
}

#[test]
fn terra_counts_its_rise_out_and_cuts_on_arm_4() {
    let (arms, shots, _, _) = walk_body(
        terra_direct,
        seats(),
        ModuleCamState {
            row_settled: true,
            victim_settled: true,
            ..Default::default()
        },
    );
    assert_eq!(arms[&3], 0x100);
    assert_eq!(arms[&4], 0x20);
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
        let n = d.drift.expect("every pass swings");
        assert_eq!((n.pitch, n.tr_y, n.tr_z), (-4, 12, -12));
        if !d.hold {
            break;
        }
        holds += 1;
    }
    assert_eq!(holds, 0xC0 - 0x41);
    let (_, shots, _, _) = walk_body(
        meta_direct,
        seats(),
        ModuleCamState {
            row_settled: true,
            victim_settled: true,
            ..Default::default()
        },
    );
    let arm4 = shots.iter().find(|(a, _)| *a == 4).unwrap().1;
    assert_eq!(arm4.tr, [-0x5C8, 0x600, -0x1000]);
}

/// Walk a director that carries its whole chain to `0xFF`, honouring its
/// phase jumps. Returns the passes each arm dwelt, every shot with its arm,
/// every direction with its arm, and the director state it ended on.
type BodyWalk = (
    std::collections::BTreeMap<u8, u32>,
    Vec<(u8, ModuleShot)>,
    Vec<(u8, ArmDirection)>,
    ModuleCamState,
);

fn walk_body(direct: ModuleDirector, seats: ModuleCamSeats, mut st: ModuleCamState) -> BodyWalk {
    let mut phase = 0u8;
    let mut dwell = std::collections::BTreeMap::<u8, u32>::new();
    let mut shots = Vec::new();
    let mut passes = Vec::new();
    for _ in 0..20_000 {
        if phase == crate::seru_magic_ticks::SERU_B_DONE_PHASE {
            return (dwell, shots, passes, st);
        }
        let d = direct(&mut st, phase, seats);
        *dwell.entry(phase).or_default() += 1;
        if let Some(s) = d.shot {
            shots.push((phase, s));
        }
        passes.push((phase, d));
        if !d.hold {
            phase = d.jump.unwrap_or(phase + 1);
        }
    }
    panic!("director never reached 0xFF");
}

/// Aluru frames a point half a unit off the victim along the reversed
/// victim-to-caster heading, then seats its creature there and frames it.
#[test]
fn aluru_frames_the_victim_then_its_creature() {
    let s = seats();
    let mut st = ModuleCamState::default();
    let h = heading(s.victim, s.caster).wrapping_add(0x800) & 0xFFF;
    let (sin, cos) = trig12(h);
    let arm0 = aluru_direct(&mut st, 0, s).shot.unwrap();
    assert_eq!(arm0.angles, [-0x80, yaw_from(0x800, h), 0]);
    assert_eq!(arm0.focus[0], (half(sin) - i32::from(s.victim.x)) as i16);
    assert_eq!(arm0.focus[2], (half(cos) - i32::from(s.victim.z)) as i16);
    // A spent band timer skips arm 2 (`addiu v0,a0,0x2` at `0x801F6F00`).
    assert_eq!(aluru_direct(&mut st, 1, s).jump, Some(3));
    let settled = ModuleCamState {
        victim_settled: true,
        ..Default::default()
    };
    let (dwell, shots, _, _) = walk_body(aluru_direct, s, settled);
    // The band word `scalar * 20` drains `scalar` a tick under `bgez`.
    assert_eq!(dwell[&4], 21);
    let arm4 = shots.iter().find(|(a, _)| *a == 4).unwrap().1;
    assert_eq!(arm4.frames, 0x80);
    assert_eq!(arm4.focus[0], (half(sin) - i32::from(s.victim.x)) as i16);
}

/// Aluru's chain reaches `0xFF` on both paths: the damage path steps
/// through arms `0x12`, `0x13` and `0x14`, the kill path leaves `0x13` for
/// `0xFF`. The band word grows `scalar` a tick, so arm `0x12` lasts until it
/// passes `scalar * 32` and arm `0x13` until it passes `scalar * 192`.
#[test]
fn aluru_walks_both_paths_to_0xff_with_the_band_word_dwell() {
    let profile = module_profile(ALURU_ENTRY).expect("directed");
    assert!(profile.director_body && profile.paces_band() && profile.stages_spawns);
    assert_eq!(profile.hit_arm, Some(ALURU_HIT_ARM));
    let damage = ModuleCamState {
        kill: Some(false),
        victim_settled: true,
        ..Default::default()
    };
    let (dwell, shots, passes, _) = walk_body(aluru_direct, seats(), damage);
    assert_eq!(dwell[&ALURU_HIT_ARM], 33);
    assert_eq!(dwell[&ALURU_SPREAD_ARM], 160);
    assert_eq!(dwell[&ALURU_SETTLE_ARM], 1);
    // Arm 5 drains the creature's colour word `0x800` to `0x400`, arm 6 on
    // to `0`, `scalar` a tick.
    assert_eq!(dwell[&5], 0x400 / MODULE_DRAIN_PER_TICK as u32);
    assert_eq!(dwell[&6], 0x400 / MODULE_DRAIN_PER_TICK as u32);
    // The spell name, then the attack name, then the clear on the hit.
    let captions: Vec<(u8, ModuleCaption)> = passes
        .iter()
        .filter_map(|(a, d)| d.caption.map(|c| (*a, c)))
        .collect();
    assert_eq!(
        captions,
        vec![
            (6, ModuleCaption::SpellName),
            (7, ModuleCaption::AttackName),
            (ALURU_HIT_ARM, ModuleCaption::Cleared),
        ]
    );
    let settle = shots
        .iter()
        .find(|(a, _)| *a == ALURU_SETTLE_ARM)
        .unwrap()
        .1;
    assert_eq!(settle.angles[0], 0x40);
    assert_eq!(settle.frames, 0x10);
    // The kill path never reaches arm 0x14, and breaks the victim up on
    // every pass of arm 0x13.
    let kill = ModuleCamState {
        kill: Some(true),
        ..Default::default()
    };
    let (dwell, _, passes, _) = walk_body(aluru_direct, seats(), kill);
    assert!(!dwell.contains_key(&ALURU_SETTLE_ARM));
    assert_eq!(dwell[&ALURU_SPREAD_ARM], 160);
    assert!(
        passes
            .iter()
            .filter(|(a, _)| *a == ALURU_SPREAD_ARM)
            .all(|(_, d)| d.jump.is_none_or(|j| j == 0xFF))
    );
}

/// The bands draw once at rest on arm `0x11`'s passing tick and then on
/// every pass of `0x12` / `0x13`, spread by the band word over 24.
#[test]
fn aluru_band_spread_follows_the_band_word() {
    let mut st = ModuleCamState {
        kill: Some(false),
        ..Default::default()
    };
    let d = aluru_direct(&mut st, ALURU_BLAST_ARM, seats());
    assert!(!d.hold);
    assert_eq!(st.band_spread, Some(0));
    for k in 1..=10 {
        let _ = aluru_direct(&mut st, ALURU_HIT_ARM, seats());
        assert_eq!(st.countdown.0, k * MODULE_DRAIN_PER_TICK);
        assert_eq!(
            st.band_spread,
            Some((k * MODULE_DRAIN_PER_TICK / 24) as i16)
        );
    }
    let _ = aluru_direct(&mut st, 8, seats());
    assert_eq!(st.band_spread, None, "a non-band arm draws none");
    assert_eq!(aluru_band_spread(0xFFE8), -1);
}

/// Iota pulls TR z in by the drain while its word runs and only passes arm
/// 2 / 3 once it is spent.
#[test]
fn iota_pulls_in_while_its_countdown_runs() {
    let mut st = ModuleCamState::default();
    let _ = iota_direct(&mut st, 0, seats());
    let a1 = iota_direct(&mut st, 1, seats());
    assert!(!a1.hold);
    assert_eq!(a1.drift.unwrap().tr_z, -MODULE_DRAIN_PER_TICK as i16);
    let (arms, shots, _, _) = walk_body(
        iota_direct,
        seats(),
        ModuleCamState {
            row_settled: true,
            victim_settled: true,
            ..Default::default()
        },
    );
    // Arm 2: `scalar << 6` less the arm-1 drain, a drain a tick, then the
    // passing tick.
    assert_eq!(arms[&2], (64 - 1) + 1);
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

/// PROT 0907's arm 10 holds on the creature's loop counter `+0x21B`
/// (`0x801F7AC0`): while clip 2's count `0xFF` is unspent, and while the
/// seat still plays clip 1 (whose count, `0`, is stale until the commit).
#[test]
fn nighto_arm_ten_holds_on_the_loop_counter_sentinel() {
    let mut st = ModuleCamState {
        creature_clip: Some(1),
        creature_loops: Some(0),
        ..Default::default()
    };
    assert!(
        nighto_direct(&mut st, 10, seats()).hold,
        "clip 1 still playing"
    );
    st.creature_clip = Some(NIGHTO_SUMMON_CLIP);
    st.creature_loops = Some(NIGHTO_LOOP_SENTINEL);
    for _ in 0..4 {
        assert!(nighto_direct(&mut st, 10, seats()).hold, "count unspent");
    }
    st.creature_loops = Some(0xFE);
    let d = nighto_direct(&mut st, 10, seats());
    assert!(!d.hold, "the park took the counter off 0xFF");
    assert_eq!(st.countdown.0, SPEED_SCALAR << 7);
}

/// PROT 0903 arm 3's three calls read the creature seat before the arm
/// stores `+0x36 = 0x200` (`0x801F6E48`): the rings open on the floor under
/// the creature's plane pair, facing its way, while the seat itself drops
/// under the floor.
#[test]
fn gimard_rings_open_on_the_floor_under_the_sunk_creature() {
    let mut st = ModuleCamState::default();
    let d = gimard_direct(&mut st, 3, seats());
    let c = st.creature.expect("arm 3 seats the creature");
    assert_eq!(c.y, 0x200);
    assert_eq!(d.spawns.len(), 3);
    for s in d.spawns {
        let (pos, rot) = spawn_anchor_point(s.anchor, Some(c), Some(seats().victim), None)
            .expect("creature anchor");
        assert_eq!(pos, [c.x, GIMARD_SEAT_SPAWN_Y, c.z]);
        assert_eq!(rot, [0, c.facing as i16, 0]);
    }
}

/// Every player-Seru module ends its caption with `FUN_800319A8(0)`: Gimard
/// as the hit lands (arm 11), Vera on the last arm's pass (arm 10), after
/// putting up the spell name in arm 5 and the attack name in arm 8.
#[test]
fn seru_modules_take_their_caption_down() {
    let mut st = ModuleCamState {
        creature_arrived: true,
        creature_live: Some(ModuleSeat::default()),
        ..Default::default()
    };
    let d = gimard_direct(&mut st, 11, seats());
    assert!(!d.hold);
    assert_eq!(d.caption, Some(ModuleCaption::Cleared));

    let mut st = ModuleCamState::default();
    let mut phase = 0u8;
    let mut seen = Vec::new();
    for _ in 0..8000u32 {
        let d = vera_direct(&mut st, phase, seats());
        if let Some(c) = d.caption {
            seen.push((phase, c));
        }
        if !d.hold {
            phase += if phase == 5 { 3 } else { 1 };
            if phase > 10 {
                break;
            }
        }
    }
    assert_eq!(
        seen,
        vec![
            (5, ModuleCaption::SpellName),
            (8, ModuleCaption::AttackName),
            (10, ModuleCaption::Cleared),
        ]
    );
}

/// A body-paced director's pass lands on the next arm's first tick, and it
/// carries the pass's spawn calls with it: Orb's opening flash (arm 2), the
/// light column (arm 5, which jumps to 9) and the heal's group parts and
/// rings (arm 9), each on its own anchor.
#[test]
fn orb_reports_its_pass_spawns_on_the_next_arm() {
    let mut st = ModuleCamState {
        target_aim: Some(legaia_engine_battle_vm::battle_target_group::GroupAim {
            centroid_x: -100,
            centroid_z: 200,
            extent: 0x666,
        }),
        ..Default::default()
    };
    let mut seen = Vec::new();
    for phase in [0u8, 1, 2, 3, 4, 5, 9, 10] {
        for _ in 0..2 {
            let d = orb_direct(&mut st, phase, seats());
            if !d.spawns.is_empty() {
                seen.push((phase, d.spawns.len(), d.caption));
            }
        }
    }
    assert_eq!(
        seen,
        vec![
            (3, ORB_FLASH_SPAWNS.len(), None),
            (9, ORB_RAY_SPAWNS.len(), Some(ModuleCaption::AttackName)),
            (10, ORB_HEAL_SPAWNS.len(), Some(ModuleCaption::Cleared)),
        ]
    );
    assert!(module_profile(911).is_some_and(|p| p.stages_spawns));
}

/// The anchors Orb's calls name: `0x3E8` over the caster, `0x140` over the
/// creature, the framed group's centre turned to the creature's facing
/// plus `0x800` at the group extent's scale, and a live party seat.
#[test]
fn orb_anchors_resolve_like_the_arms_build_them() {
    let caster = ModuleSeat {
        x: 63,
        y: 0,
        z: -591,
        facing: 34,
    };
    let creature = ModuleSeat {
        x: 89,
        y: -1024,
        z: -80,
        facing: 0x10,
    };
    let seats = AnchorSeats {
        caster: Some(caster),
        caster_body: None,
        party: [Some(caster), None, None],
        depth: 0xC00,
    };
    let at = |a| spawn_anchor_point_with(a, Some(creature), None, None, seats);
    assert_eq!(
        at(SpawnAnchor::Caster { y: -0x3E8 }),
        Some(([63, -1000, -591], [0, 34, 0]))
    );
    // The capture holds arm 5's parts at `(89, -1344, -80)`.
    assert_eq!(
        at(SpawnAnchor::CreatureRaised { dy: -0x140 }).map(|p| p.0),
        Some([89, -1344, -80])
    );
    assert_eq!(
        at(SpawnAnchor::PartySeat(0)).map(|p| p.0),
        Some([63, 0, -591])
    );
    assert_eq!(at(SpawnAnchor::PartySeat(1)), None);
    let shot = ModuleShot {
        angles: [0x80, 0, 0],
        tr: [0, 0x300, 0x666],
        focus: [-63, 0, 591],
        frames: 1,
    };
    let anchor = SpawnAnchor::ShotPointFacing { y: 0, yaw: 0x800 };
    assert_eq!(
        spawn_anchor_point_with(anchor, Some(creature), None, Some(shot), seats),
        Some(([63, 0, -591], [0, 0x810, 0]))
    );
    assert_eq!(spawn_anchor_scale(anchor, Some(shot), seats), 0x666);
    assert_eq!(
        spawn_anchor_scale(SpawnAnchor::Caster { y: 0 }, Some(shot), seats),
        0x1000
    );
}

/// Gola Gola's arms 4..6 hold until the creature's clip cursor reaches
/// `0x100` / `0x200` / `0x300` (`slti` on node `+0x68`), pass at once with
/// no cursor fed, and never hold past the bound.
#[test]
fn gola_gola_frame_waits_follow_the_creature_cursor() {
    let s = seats();
    let mut st = ModuleCamState {
        creature_frame: Some(0xF0),
        ..Default::default()
    };
    assert!(gola_gola_direct(&mut st, 4, s).hold);
    st.creature_frame = Some(0x100);
    assert!(!gola_gola_direct(&mut st, 4, s).hold, "the frame reached");
    st.creature_frame = Some(0x100);
    assert!(gola_gola_direct(&mut st, 5, s).hold);
    st.creature_frame = None;
    assert!(!gola_gola_direct(&mut st, 6, s).hold, "no cursor: pass");
    // A cursor that never moves is let go after the bound.
    let mut st = ModuleCamState {
        creature_frame: Some(0),
        ..Default::default()
    };
    let mut holds = 0;
    while gola_gola_direct(&mut st, 8, s).hold {
        holds += 1;
        assert!(holds <= super::body_kit::HOST_WAIT_CAP);
    }
    assert_eq!(holds, super::body_kit::HOST_WAIT_CAP);
}

/// Gola Gola's arm `0xB` waits for the attack clip's park (`+0x21B` off
/// `0xFF`) with the yaw drift on every pass, and arm `0xC` re-arms the park
/// and ramps the creature's rate `word >> 4 + 2` to `0x20`.
#[test]
fn gola_gola_holds_the_park_and_ramps_the_rate() {
    let s = seats();
    let mut st = ModuleCamState {
        creature_loops: Some(0xFF),
        ..Default::default()
    };
    let d = gola_gola_direct(&mut st, 0xB, s);
    assert!(d.hold && d.drift.is_some());
    st.creature_loops = Some(0xFE);
    assert!(!gola_gola_direct(&mut st, 0xB, s).hold);
    let mut rates = Vec::new();
    loop {
        let d = gola_gola_direct(&mut st, 0xC, s);
        assert!(
            std::mem::take(&mut st.creature_loop_arm),
            "the park re-armed"
        );
        rates.push(st.creature_rate.take().unwrap());
        if !d.hold {
            break;
        }
    }
    assert_eq!(rates.first(), Some(&2));
    assert_eq!(rates.last(), Some(&0x20));
    assert!(rates.windows(2).all(|w| w[0] <= w[1]));
}

/// Palma's arms 8 and 9 hold on the park, then put the cursor back on
/// keyframe 15 and re-arm the park; arm `0xA` only waits.
#[test]
fn palma_replays_its_swing_from_keyframe_fifteen() {
    let s = seats();
    let mut st = ModuleCamState {
        creature_loops: Some(0xFF),
        ..Default::default()
    };
    for arm in [8u8, 9, 0xA] {
        st.creature_loops = Some(0xFF);
        assert!(palma_direct(&mut st, arm, s).hold, "arm {arm} waits");
        st.creature_loops = Some(3);
        assert!(!palma_direct(&mut st, arm, s).hold);
        let replays = arm != 0xA;
        assert_eq!(
            st.creature_seek.take(),
            replays.then_some(0xF0),
            "arm {arm}"
        );
        assert_eq!(
            std::mem::take(&mut st.creature_loop_arm),
            replays,
            "arm {arm}"
        );
    }
}

/// The summon pre-hit cannot kill and stages the reaction.
#[test]
fn a_summon_pre_hit_leaves_the_target_at_one_hp() {
    let mut v = crate::cast_module_ticks::CastActorState {
        hp: 50,
        reaction_alt: 3,
        knockdown_anim: 4,
        ..Default::default()
    };
    let applied = crate::seru_magic_ticks::summon_pre_hit(&mut v, 999);
    assert_eq!((applied, v.hp, v.hp_bar_delta), (49, 1, 49));
    assert_eq!(v.staged_anim, 3);
    assert_eq!(v.restage & 0x05, 0x05);
}

/// Mule's arm 1 waits on the side-band applier stage, not on the band timer.
#[test]
fn mule_arm_1_waits_on_the_sideband_stage() {
    let mut s = seats();
    s.band_timer = 5;
    let mut st = ModuleCamState {
        sideband_busy: true,
        ..Default::default()
    };
    assert!(mule_direct(&mut st, 1, s).hold);
    st.sideband_busy = false;
    assert!(
        !mule_direct(&mut st, 1, s).hold,
        "the band timer alone does not hold arm 1"
    );
    assert_eq!(sideband_wait_arm(929), Some(1));
}

/// Jedo's arm 2 holds while the side-band applier stage is up
/// (`0x801F6D18..0x801F6D20`), and passes once it drops.
#[test]
fn jedo_arm_2_waits_on_the_sideband_stage() {
    let mut st = ModuleCamState {
        sideband_busy: true,
        ..Default::default()
    };
    assert!(jedo_direct(&mut st, 2, seats()).hold);
    st.sideband_busy = false;
    assert!(!jedo_direct(&mut st, 2, seats()).hold);
    assert_eq!(sideband_wait_arm(931), Some(2));
}

/// Mule's arm-16 exit frames a lone target on its own `+0x34` / `+0x38`
/// and ten times its body radius, capped at `0x1800`.
#[test]
fn mule_arm_16_frames_a_lone_target_on_its_radius() {
    let mut s = seats();
    s.victim_radius = 0x100;
    let mut st = ModuleCamState::default();
    let shot = mule_direct(&mut st, 16, s)
        .shot
        .expect("arm 16 lands a cut on its pass");
    assert_eq!(shot.tr[2], 0xA00);
    assert_eq!(shot.focus, [0, 0, 800]);
    s.victim_radius = 0x400;
    let mut st = ModuleCamState::default();
    let shot = mule_direct(&mut st, 16, s).shot.unwrap();
    assert_eq!(shot.tr[2], 0x1800, "capped");
}

/// Mule's arm kills follow the module's parked handles.
#[test]
fn mule_arm_kills_follow_the_parked_handles() {
    assert_eq!(mule_arm_kills(12), &[0x801F_917C]);
    assert_eq!(mule_arm_kills(14).len(), 3);
    assert!(mule_arm_kills(16).contains(&0x801F_9398));
    assert_eq!(mule_arm_loop_ends(16), &[0x801F_9468, 0x801F_95A8]);
    assert!(mule_arm_kills(15).is_empty());
}

/// Palma's arm 3 drops the stage once its word passes `scalar << 7`.
#[test]
fn palma_arm_3_drops_the_stage_halfway() {
    assert_eq!(palma_stage_byte(3, (SPEED_SCALAR << 7) - 1), None);
    assert_eq!(palma_stage_byte(3, SPEED_SCALAR << 7), Some(3));
    assert_eq!(palma_stage_byte(4, SPEED_SCALAR << 8), None);
}
