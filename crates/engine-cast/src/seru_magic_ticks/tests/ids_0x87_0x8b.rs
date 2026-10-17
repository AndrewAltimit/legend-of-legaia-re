use super::*;

fn seats(n: usize) -> Vec<CastActorState> {
    (0..n)
        .map(|_| CastActorState {
            hp: 400,
            anim_rate: ANIM_RATE_NORMAL,
            knockdown_anim: 0x20,
            reaction_alt: 0x21,
            reaction_alt2: 0x22,
            ..Default::default()
        })
        .collect()
}

fn ctx_at(phase: u8) -> CastModuleCtx {
    CastModuleCtx {
        party_count: 8,
        monster_count: 4,
        caster_seat: 0,
        phase,
        ..Default::default()
    }
}

/// Ticks a body spends in its post-hit countdown before it writes the
/// terminal phase, with every seat idle.
fn settle_ticks(mut tick: impl FnMut(&mut CastModuleCtx) -> CastTickStep, from: u8) -> u32 {
    let mut ctx = ctx_at(from);
    let mut n = 0;
    while ctx.phase != SERU_B_DONE_PHASE {
        tick(&mut ctx);
        n += 1;
        assert!(n < 10_000, "never settled");
    }
    n
}

#[test]
fn the_post_hit_countdowns_pace_the_settle_arms() {
    let per = |shift: i32| (SETTLE_SCALAR << shift) / SETTLE_DRAIN_PER_TICK;
    // PROT 0909: the damage arm stores `scalar << 7`, the settle arm
    // drains it.
    let mut cd = 0;
    let mut s = seats(8);
    let n = settle_ticks(
        |c| viguro_tick(c, &mut s, 0, 7, &mut cd, |_| 0).0,
        VIGURO_DAMAGE_PHASE,
    );
    assert_eq!(n as i32, 1 + per(7));
    // PROT 0911: arm 9 adds `scalar << 8`, the settle arm drains it.
    // Arm 9 enters on its last drain pass.
    let mut cd = SETTLE_DRAIN_PER_TICK;
    let mut s = seats(8);
    let n = settle_ticks(
        |c| {
            orb_tick(
                c,
                &mut s,
                7,
                0,
                None,
                &mut cd,
                &mut OrbSeatPos::default(),
                |_| 400,
            )
            .0
        },
        9,
    );
    assert_eq!(n as i32, 1 + per(8));
    // PROT 0912: `scalar << 7` through the hold arm, `scalar << 5` more
    // through the settle arm. The damage arm drains the same word first, so
    // it enters on its last drain pass.
    let mut cd = SETTLE_DRAIN_PER_TICK;
    let mut s = seats(8);
    let n = settle_ticks(
        |c| freed_tick(c, &mut s, 7, &mut cd, None, |_| 0).0,
        FREED_DAMAGE_PHASE,
    );
    assert_eq!(n as i32, 1 + per(7) + per(5));
    // PROT 0913: `scalar << 7`, and a dead victim holds until faded. The
    // damage arm drains first, so it too enters on its last pass.
    let mut cd = SETTLE_DRAIN_PER_TICK;
    let mut s = seats(8);
    let n = settle_ticks(
        |c| nova_tick(c, &mut s, 7, 3, 0, &mut cd).0,
        NOVA_DAMAGE_PHASE,
    );
    assert_eq!(n as i32, 1 + per(7));
    let mut ctx = ctx_at(NOVA_SETTLE_PHASE);
    let mut s = seats(8);
    s[3].hp = 0;
    s[3].present_04 = 0x2008_0200;
    nova_tick(&mut ctx, &mut s, 7, 3, 0, &mut 0);
    assert_eq!(ctx.phase, NOVA_SETTLE_PHASE, "the fade has not run");
    s[3].present_04 = 0;
    nova_tick(&mut ctx, &mut s, 7, 3, 0, &mut 0);
    assert_eq!(ctx.phase, SERU_B_DONE_PHASE);
}

#[test]
fn viguro_arm_zero_writes_both_context_bytes() {
    let mut ctx = ctx_at(0);
    let mut s = seats(8);
    let (step, sweep) = viguro_tick(&mut ctx, &mut s, 0, 7, &mut 0, |_| 0);
    assert_eq!(step, CastTickStep::Busy);
    assert_eq!(ctx.ctx_278, VIGURO_ARM0_CTX_278);
    assert_eq!(ctx.ctx_27a, VIGURO_ARM0_CTX_27A);
    assert_eq!(ctx.phase, 1, "arm 0 advances");
    assert!(sweep.hits.is_empty());
}

/// The rendezvous: phases 3 and 8 report busy and do **not** advance, so
/// the choreography waits for the stager's own `ctx[+0x279] += 1`.
#[test]
fn viguro_parks_on_the_two_rendezvous_phases() {
    for p in VIGURO_RENDEZVOUS_PHASES {
        let mut ctx = ctx_at(p);
        let mut s = seats(8);
        let (step, _) = viguro_tick(&mut ctx, &mut s, 0, 7, &mut 0, |_| 0);
        assert_eq!(step, CastTickStep::Busy, "phase {p} holds");
        assert_eq!(ctx.phase, p, "phase {p} does not advance");
    }
}

/// The sweep's bound is the fixed `3..7` seat range, not `ctx[+1]`, and
/// its clamp can kill.
#[test]
fn viguro_sweep_hits_the_enemy_row_only_and_can_kill() {
    let mut ctx = ctx_at(VIGURO_DAMAGE_PHASE);
    let mut s = seats(8);
    s[4].flags |= FLAG_NON_TARGETABLE;
    s[5].hp = 0;
    let (_, sweep) = viguro_tick(&mut ctx, &mut s, 0, 7, &mut 0, |_| 10_000);
    let seats_hit: Vec<u8> = sweep.hits.iter().map(|h| h.seat).collect();
    assert_eq!(seats_hit, vec![3, 6], "dead and untargetable seats skipped");
    assert_eq!(s[3].hp, 0, "shape A clamps to HP, so the hit kills");
    assert_eq!(s[0].hp, 400, "the party row is untouched");
    assert_eq!(s[3].hp_bar_delta, 400);
    assert_eq!(
        s[3].staged_anim, 0x20,
        "a killed seat takes the knockdown clip"
    );
    assert_eq!(s[3].restage, 1, "PROT 0909 assigns the restage counter");
    assert_eq!(s[3].root_speed, VIGURO_HIT_ROOT_SPEED);
}

/// The one-live-target path publishes the roll retail writes to
/// `0x8007BD14`.
#[test]
fn viguro_reports_the_single_target_roll() {
    let mut ctx = ctx_at(VIGURO_DAMAGE_PHASE);
    let mut s = seats(8);
    for a in s[4..7].iter_mut() {
        a.hp = 0;
    }
    let (_, sweep) = viguro_tick(&mut ctx, &mut s, 0, 7, &mut 0, |_| 25);
    assert_eq!(sweep.single_target_roll, Some(25));
    assert_eq!(sweep.hits.len(), 1);
}

/// Two live targets: the arm does not publish.
#[test]
fn viguro_does_not_publish_with_two_targets() {
    let mut ctx = ctx_at(VIGURO_DAMAGE_PHASE);
    let mut s = seats(8);
    s[5].hp = 0;
    s[6].hp = 0;
    let (_, sweep) = viguro_tick(&mut ctx, &mut s, 0, 7, &mut 0, |_| 25);
    assert_eq!(sweep.single_target_roll, None);
    assert_eq!(sweep.hits.len(), 2);
}

/// A live victim with `+0x1F2 == 0` takes the alternate clip and the
/// assigning restage `5`.
#[test]
fn viguro_alternate_reaction_sets_restage_five() {
    let mut ctx = ctx_at(VIGURO_DAMAGE_PHASE);
    let mut s = seats(8);
    for a in s[4..7].iter_mut() {
        a.hp = 0;
    }
    viguro_tick(&mut ctx, &mut s, 0, 7, &mut 0, |_| 1);
    assert_eq!(s[3].staged_anim, 0x21);
    assert_eq!(s[3].restage, 5);
}

#[test]
fn viguro_settles_only_when_the_row_is_idle() {
    let mut ctx = ctx_at(VIGURO_SETTLE_PHASE);
    let mut s = seats(8);
    s[3].playing_anim = 4;
    let (step, _) = viguro_tick(&mut ctx, &mut s, 0, 7, &mut 0, |_| 0);
    assert_eq!(step, CastTickStep::Busy);
    assert_eq!(ctx.phase, VIGURO_SETTLE_PHASE);

    s[3].playing_anim = 0;
    let (step, _) = viguro_tick(&mut ctx, &mut s, 0, 7, &mut 0, |_| 0);
    assert_eq!(step, CastTickStep::Busy);
    assert_eq!(ctx.phase, SERU_B_DONE_PHASE);

    let (step, _) = viguro_tick(&mut ctx, &mut s, 0, 7, &mut 0, |_| 0);
    assert_eq!(step, CastTickStep::Done);
    assert_eq!(s[7].render_flag, VIGURO_DONE_RENDER_FLAG);
    assert_eq!(ctx.ctx_27a, 0);
}

const RETAIL_CLOCK: SwordieClock = SwordieClock { rate: 4, speed: 1 };

/// PROT 0910's tick writes the summon seat and never HP.
#[test]
fn swordie_tick_stages_and_never_touches_hp() {
    let mut ctx = ctx_at(2);
    let mut s = seats(8);
    let mut st = SwordieSlashes::default();
    let (step, hits) = swordie_tick(&mut ctx, &mut s, 7, 3, &mut st, RETAIL_CLOCK, |_| 0);
    assert_eq!(step, CastTickStep::Busy);
    assert!(hits.is_empty());
    assert_eq!(s[7].staged_anim, SWORDIE_ARM2_CLIP);
    assert_eq!(s[7].restage, SWORDIE_ARM2_CLIP);
    assert!(s.iter().all(|a| a.hp == 400));
}

/// Arm 7 holds on the `ctx[+0x6D8]` countdown arm 6 loads, then sets the
/// summon's animation rate to the game-speed scalar.
#[test]
fn swordie_arm_seven_holds_then_writes_the_speed_as_anim_rate() {
    let mut ctx = ctx_at(6);
    let mut s = seats(8);
    let mut st = SwordieSlashes::default();
    let clock = SwordieClock { rate: 4, speed: 3 };
    swordie_tick(&mut ctx, &mut s, 7, 3, &mut st, clock, |_| 0);
    assert_eq!(ctx.phase, 7);
    assert_eq!(st.timer, 3 << 8);
    let mut ticks = 0;
    while ctx.phase == 7 {
        swordie_tick(&mut ctx, &mut s, 7, 3, &mut st, clock, |_| 0);
        ticks += 1;
    }
    // `speed << 8` drained by `rate * speed` a tick until it is negative.
    assert_eq!(ticks, 65);
    assert_eq!(s[7].anim_rate, 3);
}

/// The measured dwell (`docs/subsystems/cast-module.md`, PROT 0910: arm
/// `7` 65 ticks, arm `9` 25) falls out of the timers at retail's battle
/// cadence of four vsyncs a tick, and every slash lands exactly once.
#[test]
fn swordie_timers_reproduce_the_measured_dwell_and_land_four_hits() {
    let mut ctx = ctx_at(6);
    let mut s = seats(8);
    let mut st = SwordieSlashes::default();
    let mut dwell = std::collections::BTreeMap::<u8, u32>::new();
    let mut landed = Vec::new();
    for _ in 0..1000 {
        let phase = ctx.phase;
        let (step, hits) = swordie_tick(&mut ctx, &mut s, 7, 3, &mut st, RETAIL_CLOCK, |_| 40_000);
        *dwell.entry(phase).or_default() += 1;
        landed.extend(hits);
        if step == CastTickStep::Done {
            break;
        }
    }
    assert_eq!(dwell[&7], 65);
    assert_eq!(dwell[&9], 25);
    assert_eq!(landed.len(), 4, "one landing per slash");
    assert!(landed.iter().all(|h| h.seat == 3));
    // Three non-lethal quarter-hits then the lethal one.
    assert_eq!(landed[0].applied, 399);
    assert_eq!(s[3].hp, 0, "the fourth landing is the kill-capable one");
    assert_eq!(st.hits, 4);
}

/// Arms 3..5 ramp the split counter by `(rate * speed) / 4` a pass; at the
/// retail cadence (step `32`) they take the measured 8, 17 and 46 passes.
/// The panels draw on every pass of arms 0..4 and the streak on every pass
/// of arm 3; arm 4 slides the panels off their rest tables.
#[test]
fn swordie_split_arms_reproduce_the_measured_dwell_and_draw_their_packets() {
    let clock = SwordieClock { rate: 4, speed: 8 };
    let mut ctx = ctx_at(0);
    let mut s = seats(8);
    let mut st = SwordieSlashes::default();
    let mut dwell = std::collections::BTreeMap::<u8, u32>::new();
    while ctx.phase < 6 {
        let phase = ctx.phase;
        swordie_tick(&mut ctx, &mut s, 7, 3, &mut st, clock, |_| 0);
        *dwell.entry(phase).or_default() += 1;
        assert_eq!(
            st.draw.split.is_some(),
            phase <= SWORDIE_SPLIT_ARM,
            "arm {phase}"
        );
        assert_eq!(
            st.draw.streak.is_some(),
            phase == SWORDIE_STREAK_ARM,
            "arm {phase}"
        );
        if phase == SWORDIE_STREAK_ARM {
            assert!(st.draw.streak.unwrap() < SWORDIE_STREAK_END);
            assert_eq!(st.draw.split, Some(SWORDIE_PANELS_AT_REST));
        }
        assert!(dwell[&phase] < 1000);
    }
    assert_eq!(dwell[&3], 8);
    assert_eq!(dwell[&4], 17);
    assert_eq!(dwell[&5], 46);
    // Arm 4's last pass drew the panels at counter 17 * 8.
    assert_eq!(st.panels, swordie_split_panels(17 * 8));
    assert_eq!(st.panels[0][0], -4 * 17 * 8);
}

/// A finished slash is inert: the settle arm keeps calling it every frame.
#[test]
fn swordie_slash_step_lands_once() {
    let mut st = SwordieSlashes::default();
    let mut v = seats(1)[0];
    let mut calls = 0;
    let mut lands = 0;
    for _ in 0..40 {
        if swordie_slash_step(&mut st, 0, &mut v, RETAIL_CLOCK, || {
            calls += 1;
            8
        })
        .is_some()
        {
            lands += 1;
        }
    }
    assert_eq!(lands, 1);
    assert_eq!(calls, 1, "the wrapper is drawn on the landing call only");
    assert_eq!(st.progress[0], 64);
}

/// A victim already on 0 HP has a cap of `0xFFFFFFFF` (`lhu` + `addiu -1`),
/// so nothing clamps and HP wraps.
#[test]
fn unsigned_floor_one_wraps_on_a_dead_victim() {
    let mut v = seats(1)[0];
    v.hp = 0;
    assert_eq!(apply_hit_unsigned_floor_one(&mut v, 5), 5);
    assert_eq!(v.hp, 0xFFFB);
}

/// The finding: the first three slashes cannot kill, the fourth can.
#[test]
fn swordie_slashes_switch_clamp_shape_on_the_last_hit() {
    for counter in 1..SWORDIE_LETHAL_HIT {
        let mut v = seats(1)[0];
        let applied = swordie_slash(&mut v, 0, counter, 40_000);
        assert_eq!(applied, 399, "hit {counter} clamps to HP - 1");
        assert_eq!(v.hp, 1, "hit {counter} cannot kill");
    }
    let mut v = seats(1)[0];
    let applied = swordie_slash(&mut v, SWORDIE_KNOCKDOWN_SLASH, SWORDIE_LETHAL_HIT, 40_000);
    assert_eq!(applied, 400);
    assert_eq!(v.hp, 0, "the fourth hit clamps to HP and kills");
    assert_eq!(v.staged_anim, 0x20, "and stages the knockdown clip");
    assert_eq!(v.restage & 0x01, 0x01);
    assert_eq!(v.restage & 0x04, 0, "the knockdown leg does not OR in 4");
}

/// A non-final slash takes the alternate clip even when the gate is set.
#[test]
fn swordie_non_final_slash_ignores_the_knockdown_gate() {
    let mut v = seats(1)[0];
    v.reaction_gate = 1;
    swordie_slash(&mut v, 1, 2, 10);
    assert_eq!(v.staged_anim, 0x21);
    assert_eq!(v.restage, 0x05);
}

/// A negative roll is a huge unsigned value, so shape C clamps it to the
/// cap rather than healing.
#[test]
fn unsigned_floor_one_never_heals() {
    let mut v = seats(1)[0];
    let applied = apply_hit_unsigned_floor_one(&mut v, -50);
    assert_eq!(applied, 399);
    assert_eq!(v.hp, 1);
}

#[test]
fn orb_heal_amount_is_level_times_sixty_four_plus_four_hundred_forty_eight() {
    assert_eq!(orb_heal_amount(0), 0x1C0);
    assert_eq!(orb_heal_amount(1), 0x200);
    assert_eq!(orb_heal_amount(5), 0x1C0 + 5 * 64);
    // Twice the band-wide formula `docs/formats/spell-table.md` records.
    assert_eq!(orb_heal_amount(5), 2 * ((5u32 << 5) + 0xE0));
}

#[test]
fn orb_cleanse_masks_match_the_four_selector_arms() {
    assert_eq!(orb_cleanse_mask(0), None);
    assert_eq!(orb_cleanse_mask(1), Some(0xFFFC));
    assert_eq!(orb_cleanse_mask(2), Some(0xFF84));
    assert_eq!(orb_cleanse_mask(3), Some(0xFB84));
    assert_eq!(orb_cleanse_mask(4), Some(0xFB84));
    assert_eq!(orb_cleanse_mask(5), None);
    assert_eq!(
        orb_cleanse_mask(4).unwrap() & crate::cast_module_ticks::FLAG_KISS_OF_DEATH_MARK,
        0,
        "the top selector clears the Kiss of Death mark"
    );
}

/// Arm 5 jumps to phase 9 rather than advancing, which is what makes 6,
/// 7 and 8 unreachable.
#[test]
fn orb_arm_five_jumps_to_phase_nine() {
    let mut ctx = ctx_at(5);
    let mut s = seats(8);
    let (step, healed) = orb_tick(
        &mut ctx,
        &mut s,
        7,
        0,
        None,
        &mut 0,
        &mut OrbSeatPos::default(),
        |_| 400,
    );
    assert_eq!(step, CastTickStep::Busy);
    assert_eq!(ctx.phase, ORB_ARM5_TARGET_PHASE);
    assert_eq!(ctx.ctx_278, ORB_ARM5_CTX_278);
    assert!(healed.is_empty());
}

/// The heal clamps to the missing HP, skips a dead seat, and writes the
/// bar delta as the negative of the amount restored.
#[test]
fn orb_heals_the_whole_row_and_clamps_to_missing_hp() {
    let mut ctx = ctx_at(9);
    let mut s = seats(8);
    s[0].hp = 100;
    s[1].hp = 390;
    s[2].hp = 0;
    s[3].flags |= FLAG_NON_TARGETABLE;
    // Seat 0 has room for the whole amount, seat 1 only for ten points.
    let (_, healed) = orb_tick(
        &mut ctx,
        &mut s,
        7,
        orb_heal_amount(0),
        None,
        &mut 0,
        &mut OrbSeatPos::default(),
        |seat| {
            if seat == 0 { 2000 } else { 400 }
        },
    );
    assert_eq!(s[0].hp, 100 + 0x1C0, "a full 448 fits under the cap");
    assert_eq!(s[0].hp_bar_delta, -0x1C0);
    assert_eq!(s[1].hp, 400, "clamped to maxHP - HP");
    assert_eq!(s[1].hp_bar_delta, -10);
    assert_eq!(s[2].hp, 0, "a dead seat is skipped");
    assert_eq!(s[3].hp, 400, "and so is an untargetable one");
    assert_eq!(s[4].hp, 400, "a seat already at maxHP gains nothing");
    assert_eq!(s[4].hp_bar_delta, 0, "and its bar delta is overwritten");
    let seats_healed: Vec<u8> = healed.iter().map(|h| h.seat).collect();
    assert_eq!(seats_healed, vec![0, 1, 4, 5, 6, 7]);
}

/// Arm 9's pass hides the creature by flag and colour word together, and
/// lights the party it brings back.
#[test]
fn orb_heal_pass_hides_the_creature_and_lights_the_party() {
    let mut ctx = ctx_at(9);
    ctx.party_count = 2;
    let mut s = seats(8);
    s[7].present_04 = 0x1234;
    s[0].render_flag = 0xFF;
    orb_tick(
        &mut ctx,
        &mut s,
        7,
        0,
        None,
        &mut 0,
        &mut OrbSeatPos::default(),
        |_| 400,
    );
    assert_eq!((s[7].render_flag, s[7].present_04), (0xFF, 0));
    assert_eq!((s[0].render_flag, s[0].present_04), (0, ORB_PARTY_LIT_WORD));
    assert_eq!(s[1].present_04, ORB_PARTY_LIT_WORD);
    assert_ne!(
        s[2].present_04, ORB_PARTY_LIT_WORD,
        "an empty seat stays dark"
    );
}

#[test]
fn orb_cleanse_clears_the_status_bits_only_at_level_three() {
    let mut ctx = ctx_at(9);
    let mut s = seats(8);
    for a in s.iter_mut() {
        a.flags = 0x0403;
    }
    orb_tick(
        &mut ctx,
        &mut s,
        7,
        0,
        None,
        &mut 0,
        &mut OrbSeatPos::default(),
        |_| 400,
    );
    assert_eq!(s[0].flags, 0x0403, "no selector: nothing is cleared");

    let mut ctx = ctx_at(9);
    let mut s = seats(8);
    for a in s.iter_mut() {
        a.flags = 0x0403;
    }
    orb_tick(
        &mut ctx,
        &mut s,
        7,
        0,
        Some(4),
        &mut 0,
        &mut OrbSeatPos::default(),
        |_| 400,
    );
    assert_eq!(s[0].flags, 0, "selector 4 clears Venom, Toxic and the mark");
}

/// Selector `4` (a level-9 Orb) doubles every living seat's AP gauge,
/// capped at 100 - including a seat whose `+0x16E & 4` skipped the HP
/// store, since retail's ladder sits past that skip. A dead seat is
/// skipped whole.
#[test]
fn orb_selector_four_doubles_the_ap_gauge() {
    let mut ctx = ctx_at(9);
    let mut s = seats(8);
    s[0].spirit_gauge = 17;
    s[1].spirit_gauge = 60;
    s[2].spirit_gauge = 20;
    s[2].hp = 0;
    s[3].spirit_gauge = 9;
    s[3].flags |= FLAG_NON_TARGETABLE;
    let (_, healed) = orb_tick(
        &mut ctx,
        &mut s,
        7,
        0,
        Some(4),
        &mut 0,
        &mut OrbSeatPos::default(),
        |_| 400,
    );
    assert_eq!(s[0].spirit_gauge, 34);
    assert_eq!(s[1].spirit_gauge, 100, "capped at 100");
    assert_eq!(s[2].spirit_gauge, 20, "a dead seat is untouched");
    assert_eq!(s[3].spirit_gauge, 18, "the ladder runs past the HP skip");
    assert!(healed.iter().all(|h| h.doubled_ap));

    for sel in [None, Some(1), Some(2), Some(3)] {
        let mut ctx = ctx_at(9);
        let mut s = seats(8);
        s[0].spirit_gauge = 17;
        orb_tick(
            &mut ctx,
            &mut s,
            7,
            0,
            sel,
            &mut 0,
            &mut OrbSeatPos::default(),
            |_| 400,
        );
        assert_eq!(s[0].spirit_gauge, 17, "selector {sel:?} leaves AP alone");
    }
}

#[test]
fn spoon_heal_amount_is_twice_orbs() {
    assert_eq!(spoon_heal_amount(1), 0x400);
    assert_eq!(spoon_heal_amount(9), 0x800);
    for lv in 1..=9 {
        assert_eq!(spoon_heal_amount(lv), 2 * orb_heal_amount(lv));
    }
}

/// Spoon's ladder walks seats 0..3 only, skips a dead seat, still runs
/// past `+0x16E & 4`, and doubles AP on selector 4 alone.
#[test]
fn spoon_cure_sweep_doubles_ap_on_selector_four() {
    let mut s = seats(8);
    for a in s.iter_mut() {
        a.flags = 0x0403;
        a.spirit_gauge = 17;
    }
    s[1].hp = 0;
    s[2].flags |= FLAG_NON_TARGETABLE;
    let doubled = spoon_cure_sweep(&mut s, Some(4));
    assert_eq!(doubled, vec![0, 2]);
    assert_eq!(s[0].spirit_gauge, 34);
    assert_eq!(s[0].flags, 0);
    assert_eq!(s[1].spirit_gauge, 17, "dead seat skipped");
    assert_eq!(s[1].flags, 0x0403);
    assert_eq!(s[2].spirit_gauge, 34, "the ladder runs past the HP skip");
    assert_eq!(s[3].spirit_gauge, 17, "only seats 0..3");

    for sel in [None, Some(1), Some(2), Some(3), Some(10)] {
        let mut s = seats(8);
        s[0].spirit_gauge = 17;
        assert!(spoon_cure_sweep(&mut s, sel).is_empty());
        assert_eq!(s[0].spirit_gauge, 17, "selector {sel:?}");
    }
}

#[test]
fn freed_sweep_uses_the_or_restage_form() {
    let mut ctx = ctx_at(FREED_DAMAGE_PHASE);
    let mut s = seats(8);
    let (_, hits) = freed_tick(&mut ctx, &mut s, 7, &mut 0, None, |_| 30);
    assert_eq!(hits.len(), 4, "seats 3..7");
    assert_eq!(s[3].hp, 370);
    assert_eq!(s[3].staged_anim, 0x21, "alive, gate clear: alternate clip");
    assert_eq!(s[3].restage, 0x05, "OR of 4 and 1, not an assignment");
    assert_eq!(s[3].anim_rate, FREED_HIT_ANIM_RATE);
    assert_eq!(s[0].hp, 400, "the party row is untouched");
}

/// Ticks a body spends on each phase from arm 0 to its terminal, every seat
/// idle.
fn arm_dwell(mut tick: impl FnMut(&mut CastModuleCtx) -> CastTickStep) -> Vec<(u8, u32)> {
    let mut ctx = ctx_at(0);
    let mut dwell: Vec<(u8, u32)> = Vec::new();
    for _ in 0..10_000 {
        let phase = ctx.phase;
        match dwell.last_mut() {
            Some((p, n)) if *p == phase => *n += 1,
            _ => dwell.push((phase, 1)),
        }
        if tick(&mut ctx) == CastTickStep::Done {
            return dwell;
        }
    }
    panic!("never finished: {dwell:?}");
}

#[test]
fn freed_walks_twenty_arms_then_settles() {
    let mut s = seats(8);
    let mut peak = 0;
    let mut cd = 0;
    let dwell = arm_dwell(|c| {
        let step = freed_tick(c, &mut s, 7, &mut cd, None, |_| 0).0;
        peak = peak.max(s[7].shatter);
        step
    });
    // Retail's pass counts at one pass a vsync: every gated arm drains the
    // module word by the scalar and holds while it is positive; arm 9 holds
    // on the shatter word arm 8 has already half drained.
    let expected: Vec<(u8, u32)> = vec![
        (0, 1),
        (1, 1),
        (2, 1),
        (3, 128),
        (4, 64),
        (5, 64),
        (6, 64),
        (7, 32),
        (8, 32),
        (9, 32),
        (0x0A, 148),
        (0x0B, 1),
        (0x0C, 16),
        (0x0D, 128),
        (0x0E, 64),
        (0x0F, 128),
        (0x10, 64),
        (0x11, 64),
        (0x12, 128),
        (0x13, 32),
        (SERU_B_DONE_PHASE, 1),
    ];
    assert_eq!(dwell, expected);
    assert_eq!(peak, FREED_SHATTER_START, "arm 7 breaks the creature up");
    assert_eq!(s[7].shatter, 0, "arm 9 leaves it whole");
}

/// PROT 0912's colour-word stores: arm 2 hides the creature (`+0x04 = 0`),
/// arm 7 lights its shards, arm `0x0E` hides it again, arm `0x0F` restores
/// the living row's neutral word and arm `0x10` freezes it blue-white.
#[test]
fn freed_writes_the_creature_and_row_colour_words() {
    let mut s = seats(8);
    s[7].present_04 = 0x0080_2008;
    // Each arm on a spent countdown, so a gated arm passes.
    let run = |arm: u8, s: &mut Vec<CastActorState>| {
        let mut ctx = ctx_at(arm);
        freed_tick(&mut ctx, s, 7, &mut 0, None, |_| 0);
        assert_eq!(ctx.phase, arm + 1, "arm {arm:#x} passes");
    };
    run(2, &mut s);
    assert_eq!(s[7].present_04, 0);
    run(7, &mut s);
    assert_eq!(s[7].present_04, FREED_SHARD_COLOUR);
    run(0x0E, &mut s);
    assert_eq!(s[7].present_04, 0);
    s[5].hp = 0;
    s[5].present_04 = 7;
    run(0x0F, &mut s);
    assert_eq!(s[3].present_04, FREED_ROW_NEUTRAL_COLOUR);
    assert_eq!(s[5].present_04, 7, "a dead seat keeps its word");
    run(0x10, &mut s);
    assert_eq!(s[4].present_04, FREED_FROZEN_COLOUR);
    assert_eq!(s[4].render_flag, 0xFF);
    assert_eq!(s[0].present_04, 0, "the party row is untouched");
}

/// PROT 0912's arm `0x0B` waits on the creature's strike clip reaching
/// cursor `0x49`.
#[test]
fn freed_strike_arm_waits_on_the_clip_cursor() {
    let mut ctx = ctx_at(FREED_STRIKE_WAIT_PHASE);
    let mut s = seats(8);
    let mut cd = 0;
    freed_tick(&mut ctx, &mut s, 7, &mut cd, Some(0x48), |_| 0);
    assert_eq!(ctx.phase, FREED_STRIKE_WAIT_PHASE);
    freed_tick(&mut ctx, &mut s, 7, &mut cd, Some(0x49), |_| 0);
    assert_eq!(ctx.phase, FREED_STRIKE_WAIT_PHASE + 1);
    assert_eq!(cd, SETTLE_SCALAR << 4, "a store, not a re-arm");
}

#[test]
fn orb_holds_each_arm_on_its_countdown_and_drops_the_creature() {
    let mut s = seats(8);
    let mut pos = OrbSeatPos::default();
    let mut cd = 0;
    let mut lowest = 0i16;
    let dwell = arm_dwell(|c| {
        let step = orb_tick(c, &mut s, 7, 0, None, &mut cd, &mut pos, |_| 400).0;
        lowest = lowest.min(pos.y);
        step
    });
    let expected: Vec<(u8, u32)> = vec![
        (0, 1),
        (1, 1),
        (2, 1),
        (3, 128),
        (4, 100),
        (5, 180),
        (9, 256),
        (0x0A, 256),
        (SERU_B_DONE_PHASE, 1),
    ];
    assert_eq!(dwell, expected);
    assert_eq!(lowest, ORB_LIFT_HEIGHT, "arm 5 lifts it highest");
    assert_eq!(pos.z, 0x1000 >> 3, "arm 5 steps it along its facing");
}

#[test]
fn nova_holds_each_arm_on_its_countdown() {
    let mut s = seats(8);
    let mut cd = 0;
    let dwell = arm_dwell(|c| nova_tick(c, &mut s, 7, 3, 0, &mut cd).0);
    let expected: Vec<(u8, u32)> = vec![
        (0, 1),
        (1, 1),
        (2, 1),
        (3, 64),
        (4, 160),
        (5, 64),
        (6, 32),
        (7, 240),
        (8, 1),
        (9, 128),
        (0x0A, 128),
        (0x0B, 64),
        (0x0C, 64),
        (0x0D, 48),
        (0x0E, 1),
        (0x0F, 64),
        (0x10, 257),
        (0x11, 1),
        (0x12, 1),
        (0x13, 129),
        (0x14, 128),
        (SERU_B_DONE_PHASE, 1),
    ];
    assert_eq!(dwell, expected);
}

/// PROT 0913 rolls against a corpse: only `+0x16E & 4` guards the step.
#[test]
fn nova_has_no_dead_seat_guard() {
    let mut ctx = ctx_at(NOVA_DAMAGE_PHASE);
    let mut s = seats(8);
    s[3].hp = 0;
    let (_, hit) = nova_tick(&mut ctx, &mut s, 7, 3, 50, &mut 0);
    assert_eq!(hit.map(|h| h.applied), Some(0), "clamped to a zero HP bar");
    assert_eq!(s[3].staged_anim, 0x20, "and still staged the knockdown");

    let mut ctx = ctx_at(NOVA_DAMAGE_PHASE);
    let mut s = seats(8);
    s[3].flags |= FLAG_NON_TARGETABLE;
    let (_, hit) = nova_tick(&mut ctx, &mut s, 7, 3, 50, &mut 0);
    assert_eq!(hit, None, "the flag guard is the one that skips");
    assert_eq!(s[3].hp, 400);
}

#[test]
fn nova_arm_nine_writes_the_same_two_from_one_register() {
    let mut ctx = ctx_at(9);
    let mut s = seats(8);
    nova_tick(&mut ctx, &mut s, 7, 3, 0, &mut 0);
    assert_eq!(s[7].staged_anim, NOVA_ARM9_CLIP);
    assert_eq!(ctx.ctx_278, NOVA_ARM9_CLIP);
    assert_eq!(s[7].restage, 1);
}

#[test]
fn every_damage_shape_goes_through_the_summon_wrapper() {
    for entry in [909u32, 910, 912, 913] {
        let shape = seru_b_damage_shape(entry).expect("a shape per damage module");
        assert_eq!(shape.wrapper, CastWrapper::SharedSummon);
        assert!(!shape.never_kills);
        assert_eq!(shape.powers.len(), 1);
    }
    assert_eq!(seru_b_damage_shape(909).unwrap().powers[0], 0x12);
    assert_eq!(seru_b_damage_shape(912).unwrap().powers[0], 0x10);
    assert!(
        seru_b_damage_shape(911).is_none(),
        "PROT 0911 is a heal, not a damage module"
    );
}

//
// Retail fixtures from live PCSX-Redux captures of PROT 0910 and PROT 0911
// driven end to end in a random encounter. Per-arm dwell for both bodies is
// in `docs/subsystems/cast-module.md` § "Frame gating, measured".

/// PROT 0911's heal, measured: magic level `3`, one party seat on `42` of
/// `999` HP, retail restored exactly `640` = `(3 << 6) + 0x1C0`.
#[test]
fn retail_orb_heals_six_hundred_and_forty_at_level_three() {
    assert_eq!(orb_heal_amount(3), 640);
    let mut s = seats(8);
    s[0].hp = 42;
    let mut ctx = ctx_at(9);
    ctx.party_count = 1;
    let (_, healed) = orb_tick(
        &mut ctx,
        &mut s,
        7,
        orb_heal_amount(3),
        None,
        &mut 0,
        &mut OrbSeatPos::default(),
        |_| 999,
    );
    assert_eq!(healed.len(), 1);
    assert_eq!(healed[0].restored, 640);
    assert_eq!(s[0].hp, 42 + 640);
}

/// PROT 0911's arm `5` jumps to `9`, which is exactly what the capture's
/// phase walk shows: `0,1,2,3,4,5,9,0x0A,0xFF` with `6..8` never entered.
#[test]
fn retail_orb_arm_five_jumps_over_six_to_eight() {
    let mut s = seats(8);
    let mut ctx = ctx_at(5);
    orb_tick(
        &mut ctx,
        &mut s,
        7,
        0,
        None,
        &mut 0,
        &mut OrbSeatPos::default(),
        |_| 999,
    );
    assert_eq!(ctx.phase, ORB_ARM5_TARGET_PHASE);
    assert_eq!(ctx.ctx_278, ORB_ARM5_CTX_278);
}

/// PROT 0910's four slashes, measured in one cast: the wrapper returned
/// `427 / 427 / 380 / 384` from the single site `0x801F887C`, and retail
/// took `106 / 106 / 95 / 96` HP - each exactly `return >> 2`.
///
/// The shift is `srl s1, s1, 2` at `0x801F8898`, between the two operands
/// of the running-total update at `0x8007BD14`, and it rewrites the same
/// register the clamp (`0x801F88F0`) and both stores (`0x801F8900` /
/// `0x801F890C`) then use. [`swordie_slash`] takes the wrapper's raw
/// return and applies the shift itself, so this fixture asserts the retail
/// HP against the **kernel** rather than against a clamp helper - which is
/// what let an unscaled kernel pass while the capture said otherwise.
#[test]
fn retail_swordie_slash_applies_a_quarter_of_the_wrapper_return() {
    let returns: [i32; 4] = [427, 427, 380, 384];
    let applied: [u32; 4] = [106, 106, 95, 96];
    let mut v = seats(1).remove(0);
    v.hp = 9999;
    let mut hp: u32 = 9999;
    for (i, (&ret, &want)) in returns.iter().zip(applied.iter()).enumerate() {
        let counter = i as u32 + 1;
        let got = swordie_slash(&mut v, i as u8, counter, ret);
        assert_eq!(got, want, "slash {i} applied the wrapper return unscaled");
        hp -= want;
        assert_eq!(u32::from(v.hp), hp, "slash {i}");
    }
    assert_eq!(hp, 9999 - 403);
}

/// The shift is logical, so a negative wrapper return does not heal: it
/// reads as a huge unsigned value, survives the shift as a huge value and
/// clamps to the cap.
#[test]
fn a_negative_wrapper_return_still_clamps_rather_than_healing() {
    let mut v = seats(1)[0];
    assert_eq!(swordie_slash(&mut v, 0, 1, -8), 399);
    assert_eq!(v.hp, 1);
}

/// A slash's blade flies from the arm-8 origin through the victim's plane
/// pair: at the start of its flight it sits on the origin, and the two outer
/// slashes bow out to opposite sides of the line.
#[test]
fn swordie_blades_fly_from_the_origin_and_bow_apart() {
    let origin = (0, 0);
    let victim = (0, 0x800);
    let (p0, _) = swordie_blade_pose(0, 0, 8, origin, victim);
    assert_eq!(p0, [0, SWORDIE_BLADE_Y, 0]);
    let (a, ra) = swordie_blade_pose(0, 256, 8, origin, victim);
    let (b, rb) = swordie_blade_pose(1, 256, 8, origin, victim);
    // Half way: both have travelled the same distance along the line and sit
    // half a unit either side of it (`sin(0x400) >> 15`).
    assert_eq!(a[2], b[2]);
    assert_eq!(a[0], -b[0]);
    assert_eq!(a[0].abs(), 0x200);
    assert_eq!(ra[1], rb[1]);
    // The inner pair rides up as it goes.
    let (c, _) = swordie_blade_pose(2, 256, 8, origin, victim);
    assert_eq!(c[1], SWORDIE_BLADE_Y - 0x100);
}

/// The slash routine draws a blade on each call that leaves its slash
/// travelling, and none on the call that lands it.
#[test]
fn swordie_slash_draws_its_blade_until_it_lands() {
    let clock = SwordieClock { rate: 4, speed: 1 };
    let mut slashes = SwordieSlashes::default();
    let mut v = seats(1).remove(0);
    let mut drawn = 0;
    for _ in 0..20 {
        slashes.draw = SwordieDraw::default();
        swordie_slash_step(&mut slashes, 0, &mut v, clock, || 0);
        drawn += usize::from(slashes.draw.blades[0]);
    }
    assert_eq!(drawn, 15);
}
