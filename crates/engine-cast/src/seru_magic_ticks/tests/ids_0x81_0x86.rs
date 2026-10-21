use super::*;
use legaia_render_kernels::cast_theeder_packet::TheederPacket;

fn seat(hp: u16) -> CastActorState {
    CastActorState {
        hp,
        anim_rate: ANIM_RATE_NORMAL,
        knockdown_anim: 0x40,
        reaction_alt: 0x41,
        reaction_alt2: 0x42,
        ..Default::default()
    }
}

fn row() -> Vec<CastActorState> {
    (0..8).map(|_| seat(200)).collect()
}

const WHO: SeruSeats = SeruSeats {
    caster: 0,
    victim: 3,
    summon: 7,
};

/// Walk a body to its terminal and return how many frames it took.
fn walk(mut step: impl FnMut(&mut CastModuleCtx) -> CastTickStep) -> (u32, u8) {
    let mut ctx = CastModuleCtx::default();
    for n in 0..512 {
        if step(&mut ctx) == CastTickStep::Done {
            return (n, ctx.phase);
        }
    }
    panic!("body never reported Done");
}

// -- the three clamps ---------------------------------------------------

#[test]
fn shape_c_cannot_kill_but_a_negative_roll_still_clamps() {
    let mut v = seat(50);
    // A roll far above HP stops one short of death.
    assert_eq!(apply_hit_cap_hp_minus_one(&mut v, 999), 49);
    assert_eq!(v.hp, 1);
    // A negative roll read as unsigned compares above any cap, so it
    // clamps rather than healing - which is what separates shape C from
    // the signed `HP - 1` shape.
    let mut v = seat(50);
    let applied = apply_hit_cap_hp_minus_one(&mut v, (-5i32) as u32);
    assert_eq!(applied, 49);
    assert_eq!(v.hp, 1);
}

#[test]
fn shape_a_site_can_kill_where_shape_c_site_cannot() {
    let opener = SERU_HIT_SITES[3];
    let finisher = SERU_HIT_SITES[4];
    assert_eq!(opener.clamp, SeruClamp::HpMinusOneUnsigned);
    assert_eq!(finisher.clamp, SeruClamp::HpUnsigned);

    // Same raw roll, same victim: the opener leaves 1 HP, the finisher
    // empties the bar.
    let mut a = seat(40);
    opener.apply(&mut a, 4000);
    assert_eq!(a.hp, 1);
    let mut b = seat(40);
    finisher.apply(&mut b, 4000);
    assert_eq!(b.hp, 0);
}

#[test]
fn zenoir_scales_each_site_the_way_its_bytes_do() {
    assert_eq!(SERU_HIT_SITES[3].scale(400), 100); // srl 2
    assert_eq!(SERU_HIT_SITES[4].scale(400), 300); // *3 then srl 2
    assert_eq!(SERU_HIT_SITES[5].scale(300), 200); // *2 / 3
    // No other site in the set scales at all.
    for s in &SERU_HIT_SITES[0..3] {
        assert_eq!(s.scale(1234), 1234);
    }
}

#[test]
fn baked_powers_are_the_call_site_immediates() {
    let powers: Vec<u16> = SERU_HIT_SITES.iter().map(|s| s.power()).collect();
    assert_eq!(powers, vec![0x12, 0x11, 0x12, 0x12, 0x10, 0x12]);
    // Both delay-slot constants survive.
    assert_eq!(seru_hit_site(0x801F_7D38).unwrap().power(), 0x11);
    assert_eq!(seru_hit_site(0x801F_7370).unwrap().power(), 0x12);
    // Every site goes through the shared kernel's summon branch.
    assert!(
        SERU_HIT_SITES
            .iter()
            .all(|s| s.wrapper == CastWrapper::SharedSummon)
    );
}

// -- the reaction stages ------------------------------------------------

#[test]
fn the_three_reaction_stages_differ_in_more_than_spelling() {
    // Bit-set form on a live victim: alt clip, bits 4 and 1 OR'd in over
    // whatever was there.
    let mut v = seat(10);
    v.restage = 0x80;
    stage_reaction_bits(&mut v);
    assert_eq!(v.staged_anim, 0x41);
    assert_eq!(v.restage, 0x85);

    // Assignment form on the same victim throws the old value away.
    let mut v = seat(10);
    v.restage = 0x80;
    stage_reaction_assign(&mut v);
    assert_eq!(v.restage, 5);

    // Dead victim takes the knockdown clip on both.
    let mut v = seat(0);
    stage_reaction_bits(&mut v);
    assert_eq!(v.staged_anim, 0x40);

    // Alt-only form has no knockdown branch at all, and increments.
    let mut v = seat(0);
    v.restage = 2;
    stage_reaction_alt_only(&mut v);
    assert_eq!(v.staged_anim, 0x41);
    assert_eq!(v.restage, 3);
}

#[test]
fn reaction_falls_through_to_the_second_alternative() {
    let mut v = seat(10);
    v.reaction_alt = 0;
    stage_reaction_bits(&mut v);
    assert_eq!(v.staged_anim, 0x42);
}

// -- per-body walks -----------------------------------------------------

#[test]
fn gimard_waits_out_a_dead_victims_fade() {
    let mut seats = row();
    let mut ctx = CastModuleCtx {
        phase: GIMARD_HIT_ARM,
        ..Default::default()
    };
    // A host that folds the damage itself passes no roll: the burn
    // presentation still lands on the victim.
    let (_, hits) = gimard_tick(&mut ctx, &mut seats, WHO, None);
    assert!(hits.is_empty());
    assert_eq!(seats[3].render_flag, 0);
    assert_eq!(seats[3].present_04, GIMARD_POSE_PRESENT_WORD);
    seats[3].hp = 0;
    seats[3].playing_anim = 0;
    gimard_tick(&mut ctx, &mut seats, WHO, None);
    assert_eq!(ctx.phase, GIMARD_SETTLE_ARM, "the fade has not run");
    seats[3].present_04 = 0;
    gimard_tick(&mut ctx, &mut seats, WHO, None);
    assert_eq!(ctx.phase, CHOREOGRAPHY_DONE_PHASE);
}

#[test]
fn gimard_hits_only_on_arm_eleven_and_settles_from_twelve() {
    let mut seats = row();
    let mut ctx = CastModuleCtx {
        phase: GIMARD_HIT_ARM,
        ..Default::default()
    };
    let (step, hits) = gimard_tick(&mut ctx, &mut seats, WHO, Some(30));
    assert_eq!(step, CastTickStep::Busy);
    assert_eq!(
        hits,
        vec![SweepHit {
            seat: 3,
            applied: 30
        }]
    );
    assert_eq!(seats[3].hp, 170);
    assert_eq!(seats[7].render_flag, GIMARD_DONE_RENDER_FLAG);
    assert_eq!(ctx.phase, GIMARD_SETTLE_ARM);

    // Arm 12 latches the terminal once the victim has stopped reacting.
    seats[3].playing_anim = 0;
    assert_eq!(seats[3].present_04, GIMARD_POSE_PRESENT_WORD);
    let (step, _) = gimard_tick(&mut ctx, &mut seats, WHO, None);
    assert_eq!(step, CastTickStep::Busy);
    assert_eq!(ctx.phase, CHOREOGRAPHY_DONE_PHASE);
    assert_eq!(seats[3].anim_rate, GIMARD_SETTLE_ANIM_RATE);
    let (step, _) = gimard_tick(&mut ctx, &mut seats, WHO, None);
    assert_eq!(step, CastTickStep::Done);
}

#[test]
fn gimard_skips_a_non_targetable_victim() {
    let mut seats = row();
    seats[3].flags = FLAG_NON_TARGETABLE;
    let mut ctx = CastModuleCtx {
        phase: GIMARD_HIT_ARM,
        ..Default::default()
    };
    let (_, hits) = gimard_tick(&mut ctx, &mut seats, WHO, Some(30));
    assert!(hits.is_empty());
    assert_eq!(seats[3].hp, 200);
    // The summon-seat and caster writes are outside the guard.
    assert_eq!(seats[7].target_code, 0);
    assert_eq!(seats[0].render_flag, 0);
}

#[test]
fn theeder_sweeps_the_monster_row_and_skips_a_reacting_seat() {
    let mut seats = row();
    seats[4].playing_anim = 9; // already reacting
    seats[5].hp = 0; // dead
    let mut ctx = CastModuleCtx {
        phase: THEEDER_SWEEP_ARM,
        ..Default::default()
    };
    let (_, hits) = theeder_tick(
        &mut ctx,
        &mut seats,
        WHO,
        TheederGeom::default(),
        &mut TheederFx::default(),
        |_| Some(25),
    );
    let seats_hit: Vec<u8> = hits.iter().map(|h| h.seat).collect();
    assert_eq!(seats_hit, vec![3, 6]);
    assert_eq!(seats[3].hp, 175);
    assert_eq!(seats[6].hp, 175);
    // The bound is the hard-coded 7, so seat 7 - the summon - is never in
    // the sweep even though the row is long enough.
    assert_eq!(seats[7].hp, 200);
}

/// Arm 11 ramps `ctx+0x6D8` by half the scalar a tick and holds below
/// `0x100` (64 ticks); its exit retargets both seats and zeroes the word.
#[test]
fn theeder_retargets_both_seats_once_its_ramp_reaches_0x100() {
    let mut seats = row();
    let mut ctx = CastModuleCtx {
        phase: THEEDER_RETARGET_ARM,
        ..Default::default()
    };
    let mut ticks = 0;
    while ctx.phase == THEEDER_RETARGET_ARM {
        theeder_tick(
            &mut ctx,
            &mut seats,
            WHO,
            TheederGeom::default(),
            &mut TheederFx::default(),
            |_| None,
        );
        ticks += 1;
        if ctx.phase == THEEDER_RETARGET_ARM {
            assert_eq!(seats[0].target_code, 0, "no retarget while holding");
        }
    }
    assert_eq!(ticks, 64);
    assert_eq!(ctx.phase, THEEDER_SWEEP_ARM);
    assert_eq!(ctx.ctx_6d8, 0, "the exit zeroes the word for the sweep");
    assert_eq!(seats[0].target_code, TARGET_CODE_ENEMY_ROW);
    assert_eq!(seats[7].target_code, TARGET_CODE_ENEMY_ROW);
}

/// Arm 12 runs its sweep word through one turn at eight times the scalar
/// a tick (64 ticks) and holds until it has; arm 10 zeroes the word first.
#[test]
fn theeder_packet_arms_draw_charge_sweep_and_retract() {
    let mut seats = row();
    let geom = TheederGeom {
        x: 0,
        z: 0,
        facing: 0,
        clip_loops: 0,
    };
    let mut fx = TheederFx::default();
    // Arm 8 seeds arm 9's countdown; arm 9 draws prongs while it drains.
    let mut ctx = CastModuleCtx {
        phase: THEEDER_PRONG_SEED_ARM,
        ..Default::default()
    };
    theeder_tick(&mut ctx, &mut seats, WHO, geom, &mut fx, |_| None);
    assert_eq!(ctx.phase, THEEDER_PRONG_ARM);
    let mut prong_ticks = 0;
    while ctx.phase == THEEDER_PRONG_ARM {
        theeder_tick(&mut ctx, &mut seats, WHO, geom, &mut fx, |_| None);
        if matches!(fx.packet, Some(TheederPacket::Prongs { .. })) {
            prong_ticks += 1;
        }
    }
    // 0x78 * scalar drained by the scalar a tick: 120 drawing ticks.
    assert_eq!(prong_ticks, 120);
    // Arm 10 empties the trail; arm 11 charges, then samples on exit.
    ctx.phase = THEEDER_RAMP_RESET_ARM;
    theeder_tick(&mut ctx, &mut seats, WHO, geom, &mut fx, |_| None);
    theeder_tick(&mut ctx, &mut seats, WHO, geom, &mut fx, |_| None);
    assert!(matches!(fx.packet, Some(TheederPacket::Charge { .. })));
    while ctx.phase == THEEDER_RETARGET_ARM {
        theeder_tick(&mut ctx, &mut seats, WHO, geom, &mut fx, |_| None);
    }
    assert!(matches!(
        fx.packet,
        Some(TheederPacket::Sweep { drawn: 0, .. })
    ));
    while ctx.phase == THEEDER_SWEEP_ARM {
        theeder_tick(&mut ctx, &mut seats, WHO, geom, &mut fx, |_| None);
    }
    // Sixteen passes cap the trail at 0xF.
    assert_eq!(fx.trail.n, THEEDER_TRAIL_CAP);
    // Arm 13 retracts one entry a pass and holds until none is left.
    while ctx.phase < THEEDER_RETRACT_ARM {
        theeder_tick(&mut ctx, &mut seats, WHO, geom, &mut fx, |_| None);
    }
    let mut retract_ticks = 0;
    while ctx.phase == THEEDER_RETRACT_ARM {
        theeder_tick(&mut ctx, &mut seats, WHO, geom, &mut fx, |_| None);
        assert!(matches!(fx.packet, Some(TheederPacket::Retract { .. })));
        retract_ticks += 1;
    }
    assert_eq!(fx.trail.n, 0);
    // Fifteen drops, one a pass: within a pass of fifteen passes.
    let pass = usize::from(THEEDER_TRAIL_PASS_TICKS);
    assert!(
        ((THEEDER_TRAIL_CAP - 1) * pass + 1..=THEEDER_TRAIL_CAP * pass).contains(&retract_ticks),
        "retract held {retract_ticks} ticks"
    );
}

#[test]
fn theeder_seat_stands_half_a_unit_short_of_the_victim_facing_it() {
    // Caster at the origin, victim straight down +Z.
    let (x, z, f) = theeder_seat_placement((0, 3000), (0, 0));
    assert_eq!(f, 0, "facing from the caster toward the victim");
    assert_eq!((x, z), (0, 3000 - 2048));
    // Victim along +X: a quarter turn.
    let (x, z, f) = theeder_seat_placement((3000, 0), (0, 0));
    assert_eq!(f, 0x400);
    assert_eq!((x, z), (3000 - 2048, 0));
}

#[test]
fn theeder_ray_swings_about_the_facing() {
    let mouth = theeder_mouth(0, 0, 0);
    assert_eq!(mouth, [0, THEEDER_MOUTH_Y, 0x5C]);
    // Phase 0: straight down the facing, 2/3 of a unit out.
    assert_eq!(
        theeder_ray_tip(mouth, 0, 0),
        [0, THEEDER_MOUTH_Y, 0x5C + 0xAAA]
    );
    // A quarter phase: swung by sin/16 = +0x100.
    let tip = theeder_ray_tip(mouth, 0, 0x400);
    let (s, c) = legaia_engine_system::trig::trig12(0x100);
    assert_eq!(tip[0], (i32::from(s) * 2 / 3) as i16);
    assert_eq!(tip[2], 0x5C + (i32::from(c) * 2 / 3) as i16);
}

#[test]
fn theeder_sweeps_for_one_turn_of_the_ray() {
    let mut seats = row();
    let mut ctx = CastModuleCtx {
        phase: THEEDER_RAMP_RESET_ARM,
        ctx_6d8: 0x1234,
        ..Default::default()
    };
    theeder_tick(
        &mut ctx,
        &mut seats,
        WHO,
        TheederGeom::default(),
        &mut TheederFx::default(),
        |_| None,
    );
    assert_eq!((ctx.phase, ctx.ctx_6d8), (THEEDER_RETARGET_ARM, 0));
    ctx.phase = THEEDER_SWEEP_ARM;
    let mut ticks = 0;
    while ctx.phase == THEEDER_SWEEP_ARM {
        assert_eq!(
            theeder_sweep_phase(ctx.ctx_6d8),
            ctx.ctx_6d8
                .wrapping_add(theeder_ramp_per_tick(THEEDER_SWEEP_ARM))
                & 0xFFF
        );
        theeder_tick(
            &mut ctx,
            &mut seats,
            WHO,
            TheederGeom::default(),
            &mut TheederFx::default(),
            |_| None,
        );
        ticks += 1;
    }
    assert_eq!(ticks, 64);
    assert_eq!(ctx.ctx_6d8, 0x1000);
}

#[test]
fn vera_restores_and_clamps_to_the_missing_half() {
    // Level 4 wants 0x20*4 + 0xE0 = 0x160 = 352.
    assert_eq!(vera_heal_amount(4, 100, 1000), 352);
    // Missing less than that clamps to missing.
    assert_eq!(vera_heal_amount(4, 900, 1000), 100);
    // Nothing missing restores nothing.
    assert_eq!(vera_heal_amount(4, 1000, 1000), 0);
}

#[test]
fn vera_heal_reproduces_the_curated_per_level_table() {
    // `legaia_gamedata::VERA_HEAL` is mined from public walkthroughs and
    // lists levels 1..=9 as 256, 288, ... 512. The bytes' formula
    // (`level << 5` then `+ 0xE0`, at 0x801F7C5C / 0x801F7C60) reproduces
    // it exactly, which is an independent check on the read that neither
    // source could give on its own.
    let curated = [256u16, 288, 320, 352, 384, 416, 448, 480, 512];
    for (i, want) in curated.iter().enumerate() {
        let level = (i + 1) as u8;
        // Plenty of headroom so the missing-HP clamp does not bind.
        assert_eq!(
            vera_heal_amount(level, 0x0001, 0x7000),
            *want,
            "magic level {level}"
        );
    }
}

#[test]
fn vera_writes_a_negative_hp_bar_delta_rather_than_accumulating() {
    let mut seats = row();
    seats[3].hp_bar_delta = 77;
    let mut ctx = CastModuleCtx {
        phase: VERA_RESTORE_ARM,
        ..Default::default()
    };
    let (_, out) = vera_tick(
        &mut ctx,
        &mut seats,
        WHO,
        Some(VeraRestore {
            magic_level: 1,
            max_hp: 1000,
            cure_tier: 1,
            apply_hp: true,
        }),
    );
    let out = out.unwrap();
    assert_eq!(out.restored, 0x100);
    assert_eq!(seats[3].hp, 200 + 0x100);
    // Stored, not accumulated - the 77 is gone.
    assert_eq!(seats[3].hp_bar_delta, -0x100);
}

/// The four tiers are the **light row** of the Seru side-effect table,
/// band by band - so a cure tier is a function of the caster's magic
/// level, and a damaging row's percent selects nothing.
#[test]
fn the_cure_tiers_are_the_light_rows_level_bands() {
    use legaia_game_tables::seru_side_effect::{
        RETAIL_CURE_CLASS_BY_BAND, RETAIL_PERCENT_BY_BAND, SideEffectKind, level_band,
    };
    assert_eq!(
        SideEffectKind::for_element(CURE_ELEMENT),
        SideEffectKind::Cure
    );
    // Level -> band -> tier -> mask, end to end. Levels 3..=9, the only
    // ones the stager services.
    let want: [(u8, usize, u8, u16); 7] = [
        (3, 0, 1, 0xFFFC),
        (4, 0, 1, 0xFFFC),
        (5, 1, 2, 0xFF84),
        (6, 1, 2, 0xFF84),
        (7, 2, 3, 0xFB84),
        (8, 2, 3, 0xFB84),
        (9, 3, 4, 0xFB84),
    ];
    for (level, band, tier, mask) in want {
        assert_eq!(level_band(level), Some(band), "level {level}");
        assert_eq!(RETAIL_CURE_CLASS_BY_BAND[band], tier, "level {level}");
        assert_eq!(VERA_CURE_MASKS[usize::from(tier - 1)], mask, "tier {tier}");
    }
    // Below the stager's own floor nothing stages at all, so the latch
    // holds `0` and `0 - 1` indexes nothing.
    assert_eq!(level_band(2), None);
    assert!(VERA_CURE_MASKS.get(0u8.wrapping_sub(1) as usize).is_none());
    // A damaging row leaves a percent in the same latch; none of the four
    // arms matches it, which is the element gate.
    for pct in RETAIL_PERCENT_BY_BAND {
        assert!(
            VERA_CURE_MASKS.get(pct.wrapping_sub(1) as usize).is_none(),
            "percent {pct} must not select a cure tier"
        );
    }
}

/// Tier 4 is the only arm that also doubles the AP gauge `+0x170`.
#[test]
fn tier_four_doubles_the_ap_gauge() {
    for (tier, want) in [(1u8, false), (2, false), (3, false), (4, true)] {
        let mut seats = row();
        seats[3].flags = 0x0003;
        seats[3].spirit_gauge = 17;
        let mut ctx = CastModuleCtx {
            phase: VERA_RESTORE_ARM,
            ..Default::default()
        };
        let (_, out) = vera_tick(
            &mut ctx,
            &mut seats,
            WHO,
            Some(VeraRestore {
                magic_level: 9,
                max_hp: 1000,
                cure_tier: tier,
                apply_hp: true,
            }),
        );
        let out = out.unwrap();
        assert!(out.cured, "tier {tier}");
        assert_eq!(out.doubled_ap, want, "tier {tier}");
        let ap = if want { 34 } else { 17 };
        assert_eq!(seats[3].spirit_gauge, ap, "tier {tier}");
    }
    assert_eq!(CURE_TIER4_AP_CAP, 0x64);
}

/// The doubling caps at 100, and the community-reported pairs hold: a
/// heal after the target's turn doubles `AP + 8`; one before it doubles
/// `AP` and the turn-end `+8` lands afterwards.
#[test]
fn cure_tier4_ap_doubles_and_caps_at_one_hundred() {
    assert_eq!(cure_tier4_ap(0), 0);
    assert_eq!(cure_tier4_ap(25), 50);
    assert_eq!(cure_tier4_ap(50), 100);
    assert_eq!(cure_tier4_ap(51), 100);
    assert_eq!(cure_tier4_ap(100), 100);
    for (ap, after_turn, before_turn) in [
        (17u16, 50u16, 42u16),
        (9, 34, 26),
        (42, 100, 92),
        (26, 68, 60),
        (34, 84, 76),
    ] {
        assert_eq!(cure_tier4_ap(ap + 8), after_turn, "after the turn, AP {ap}");
        assert_eq!(
            cure_tier4_ap(ap) + 8,
            before_turn,
            "before the turn, AP {ap}"
        );
    }
}

#[test]
fn vera_cures_only_from_magic_level_three() {
    let mut seats = row();
    seats[3].flags = 0x0003;
    let mut ctx = CastModuleCtx {
        phase: VERA_RESTORE_ARM,
        ..Default::default()
    };
    let (_, out) = vera_tick(
        &mut ctx,
        &mut seats,
        WHO,
        Some(VeraRestore {
            magic_level: 2,
            max_hp: 1000,
            cure_tier: 1,
            apply_hp: true,
        }),
    );
    assert!(!out.unwrap().cured);
    assert_eq!(seats[3].flags, 0x0003);

    let mut ctx = CastModuleCtx {
        phase: VERA_RESTORE_ARM,
        ..Default::default()
    };
    let (_, out) = vera_tick(
        &mut ctx,
        &mut seats,
        WHO,
        Some(VeraRestore {
            magic_level: VERA_CURE_MIN_LEVEL,
            max_hp: 1000,
            cure_tier: 1,
            apply_hp: true,
        }),
    );
    assert!(out.unwrap().cured);
    assert_eq!(seats[3].flags, 0x0000);
}

#[test]
fn gizam_walks_through_the_phase_five_hole() {
    let mut seats = row();
    let mut ctx = CastModuleCtx {
        phase: GIZAM_MISSING_ARM,
        ..Default::default()
    };
    let (step, _) = gizam_tick(&mut ctx, &mut seats, WHO, |_| None);
    assert_eq!(step, CastTickStep::Busy);
    assert_eq!(ctx.phase, GIZAM_MISSING_ARM + 1);
}

/// Arm 7's row relight skips a dead seat: both stores sit past the `beq`
/// on its HP (`0x801F6F74`, delay slot `nop`), so a faded-out foe keeps its
/// defeat-fade state and the zero colour word arm 14's poll waits on.
#[test]
fn gizam_relight_leaves_a_dead_seat_faded() {
    let mut seats = row();
    seats[3].hp = 0;
    seats[3].render_flag = 2;
    seats[3].present_04 = 0;
    seats[4].render_flag = 1;
    let mut ctx = CastModuleCtx {
        phase: GIZAM_STAGE_ARM,
        ..Default::default()
    };
    gizam_tick(&mut ctx, &mut seats, WHO, |_| None);
    assert_eq!((seats[3].render_flag, seats[3].present_04), (2, 0));
    assert_eq!(
        (seats[4].render_flag, seats[4].present_04),
        (0, GIZAM_ROW_LIT_WORD)
    );
}

#[test]
fn gizam_sweep_marks_every_seat_it_hits() {
    let mut seats = row();
    seats[5].flags = FLAG_NON_TARGETABLE;
    let mut ctx = CastModuleCtx {
        phase: GIZAM_SWEEP_ARM,
        ..Default::default()
    };
    let (_, hits) = gizam_tick(&mut ctx, &mut seats, WHO, |_| Some(60));
    assert_eq!(hits.len(), 3);
    for seat in [3u8, 4, 6] {
        assert_eq!(seats[seat as usize].hp, 140);
        assert_eq!(
            seats[seat as usize].flags & GIZAM_STATUS_BIT,
            GIZAM_STATUS_BIT
        );
        assert_eq!(seats[seat as usize].restage, 5);
    }
    assert_eq!(seats[5].hp, 200);
    assert_eq!(seats[7].render_flag, GIZAM_HIDDEN_RENDER_FLAG);
}

/// Arm 12's pass turns the living row blue; a dead seat keeps its fade.
#[test]
fn gizam_bubbles_tint_the_living_row_blue() {
    let mut seats = row();
    seats[4].hp = 0;
    seats[4].render_flag = 2;
    let mut ctx = CastModuleCtx {
        phase: GIZAM_BUBBLE_ARM,
        ..Default::default()
    };
    gizam_tick(&mut ctx, &mut seats, WHO, |_| None);
    assert_eq!(seats[3].render_flag, GIZAM_BUBBLE_RENDER_FLAG);
    assert_eq!(seats[4].render_flag, 2);
    assert_eq!(ctx.phase, GIZAM_BUBBLE_ARM + 1);
}

/// The sweep's presentation lands with the damage folded elsewhere: the
/// row comes off the blue and takes the hit's colour word.
#[test]
fn gizam_sweep_presents_without_a_roll() {
    let mut seats = row();
    seats[5].flags = FLAG_NON_TARGETABLE;
    for s in &mut seats[3..7] {
        s.render_flag = GIZAM_BUBBLE_RENDER_FLAG;
    }
    let mut ctx = CastModuleCtx {
        phase: GIZAM_SWEEP_ARM,
        ..Default::default()
    };
    let (_, hits) = gizam_tick(&mut ctx, &mut seats, WHO, |_| None);
    assert!(hits.is_empty());
    assert_eq!(seats[3].hp, 200);
    assert_eq!(
        (seats[3].render_flag, seats[3].present_04),
        (0, GIZAM_HIT_PRESENT_WORD)
    );
    assert_eq!(seats[5].render_flag, GIZAM_BUBBLE_RENDER_FLAG);
}

#[test]
fn gizam_sweep_is_kill_capable() {
    // `sltu a0, s1` at 0x801F739C is against live HP, not `HP - 1`: this
    // sweep empties the bar where PROT 0908's opener stops at 1.
    let mut seats = row();
    seats[3].hp = 7;
    let mut ctx = CastModuleCtx {
        phase: GIZAM_SWEEP_ARM,
        ..Default::default()
    };
    gizam_tick(&mut ctx, &mut seats, WHO, |_| Some(9999));
    assert_eq!(seats[3].hp, 0);
    // The knockdown fork fires because the hit killed.
    assert_eq!(seats[3].staged_anim, 0x40);
    assert_eq!(seats[3].restage, 1);
}

#[test]
fn nighto_kill_branch_zeroes_hp_and_clears_status() {
    let mut seats = row();
    seats[3].flags = 0xFFFF;
    let mut ctx = CastModuleCtx {
        phase: NIGHTO_FORK_ARM,
        ..Default::default()
    };
    nighto_tick(&mut ctx, &mut seats, WHO, NightoOutcome::Kill, true);
    assert_eq!(seats[3].hp, 0);
    assert_eq!(seats[3].flags, 0);
    assert_eq!(seats[3].render_flag, NIGHTO_KILL_RENDER_FLAG);
    assert_eq!(ctx.phase, NIGHTO_KILL_SETTLE_ARM);
}

#[test]
fn nighto_confuse_branch_jumps_the_settle_arm() {
    let mut seats = row();
    let mut ctx = CastModuleCtx {
        phase: NIGHTO_FORK_ARM,
        ..Default::default()
    };
    nighto_tick(&mut ctx, &mut seats, WHO, NightoOutcome::Confuse, true);
    assert_eq!(ctx.phase, NIGHTO_CONFUSE_ARM);
    assert_eq!(seats[3].hp, 200);
    nighto_tick(&mut ctx, &mut seats, WHO, NightoOutcome::Confuse, true);
    assert_eq!(seats[3].flags & NIGHTO_CONFUSE_BITS, NIGHTO_CONFUSE_BITS);
    assert_eq!(ctx.phase, CHOREOGRAPHY_DONE_PHASE);
    let step = nighto_tick(&mut ctx, &mut seats, WHO, NightoOutcome::Confuse, true);
    assert_eq!(step, CastTickStep::Done);
    assert_eq!(seats[3].anim_rate, NIGHTO_EXIT_ANIM_RATE);
}

#[test]
fn nighto_roll_is_one_in_eight_death_behind_a_level_scaled_resist() {
    let land = NightoRoll {
        kill_roll: 0,
        resist_roll: 0,
        magic_level: 1,
        target_immune: false,
        extra_roll: None,
    };
    assert_eq!(nighto_outcome(&land), NightoOutcome::Kill);
    // One off the 8 and it is the confuse branch instead.
    assert_eq!(
        nighto_outcome(&NightoRoll {
            kill_roll: 1,
            ..land
        }),
        NightoOutcome::Confuse
    );
    // The resist throw is `rand() % (0x13 - level)`, so the same throw
    // lands at a high level and is resisted at a low one.
    let throw = 9;
    assert_eq!(
        nighto_outcome(&NightoRoll {
            resist_roll: throw,
            magic_level: 0,
            ..land
        }),
        NightoOutcome::KillResisted
    );
    assert_eq!(
        nighto_outcome(&NightoRoll {
            resist_roll: throw,
            magic_level: 0x0A,
            ..land
        }),
        NightoOutcome::Kill
    );
    // The monster record's own immunity byte short-circuits everything.
    assert_eq!(
        nighto_outcome(&NightoRoll {
            target_immune: true,
            ..land
        }),
        NightoOutcome::KillResisted
    );
    // ...but only the WRITES: the kill word still decides which leg, so a
    // resisted landing roll is `KillResisted`, never `ConfuseResisted`.
    assert!(
        nighto_outcome(&NightoRoll {
            target_immune: true,
            ..land
        })
        .deadly()
    );
    // And the one character index that takes the extra roll loses it on a
    // multiple of three.
    assert_eq!(
        nighto_outcome(&NightoRoll {
            extra_roll: Some(6),
            ..land
        }),
        NightoOutcome::KillResisted
    );
    assert_eq!(
        nighto_outcome(&NightoRoll {
            extra_roll: Some(7),
            ..land
        }),
        NightoOutcome::Kill
    );
}

#[test]
fn nighto_resisted_writes_neither_outcome() {
    for outcome in [NightoOutcome::KillResisted, NightoOutcome::ConfuseResisted] {
        let mut seats = row();
        let mut ctx = CastModuleCtx {
            phase: NIGHTO_FORK_ARM,
            ..Default::default()
        };
        nighto_tick(&mut ctx, &mut seats, WHO, outcome, true);
        assert_eq!(seats[3].hp, 200, "{outcome:?} wrote HP");
        assert_eq!(seats[3].flags, 0, "{outcome:?} wrote +0x16E");
        assert_eq!(seats[3].render_flag, 0, "{outcome:?} wrote +0x21C");
    }
}

/// The capture's own shape: a resisted cast whose kill word was non-zero
/// leaves arm 13 on **15**, not on 14 - the port advanced to 14 for every
/// resist until this fixture. Arm 15 then writes nothing and latches
/// `0xFF`.
#[test]
fn retail_a_resisted_nighto_still_forks_on_the_kill_word() {
    let mut seats = row();
    let mut ctx = CastModuleCtx {
        phase: NIGHTO_FORK_ARM,
        ..Default::default()
    };
    nighto_tick(
        &mut ctx,
        &mut seats,
        WHO,
        NightoOutcome::ConfuseResisted,
        true,
    );
    assert_eq!(ctx.phase, NIGHTO_CONFUSE_ARM, "13 -> 15");
    nighto_tick(
        &mut ctx,
        &mut seats,
        WHO,
        NightoOutcome::ConfuseResisted,
        true,
    );
    assert_eq!(ctx.phase, CHOREOGRAPHY_DONE_PHASE);
    assert_eq!(
        seats[3].flags & NIGHTO_CONFUSE_BITS,
        0,
        "arm 15 wrote +0x16E"
    );
    // The death leg's resist goes the other way: 13 -> 14, still silent.
    let mut seats = row();
    let mut ctx = CastModuleCtx {
        phase: NIGHTO_FORK_ARM,
        ..Default::default()
    };
    nighto_tick(&mut ctx, &mut seats, WHO, NightoOutcome::KillResisted, true);
    assert_eq!(ctx.phase, NIGHTO_KILL_SETTLE_ARM, "13 -> 14");
    assert_eq!(seats[3].hp, 200);
}

#[test]
fn zenoir_opener_cannot_kill_and_lands_a_quarter() {
    let mut seats = row();
    let mut ctx = CastModuleCtx {
        phase: ZENOIR_OPENER_ARM,
        ..Default::default()
    };
    let (_, hits) = zenoir_tick(&mut ctx, &mut seats, WHO, |s| (s == 3).then_some(80));
    assert_eq!(
        hits,
        vec![SweepHit {
            seat: 3,
            applied: 20
        }]
    );
    assert_eq!(seats[3].hp, 180);
    assert_eq!(seats[0].target_code, TARGET_CODE_ENEMY_ROW);
}

#[test]
fn zenoir_finisher_splashes_every_other_live_monster_seat() {
    let mut seats = row();
    seats[5].render_flag = 3; // hidden -> skipped by the splash guard
    let mut ctx = CastModuleCtx {
        phase: ZENOIR_FINISH_ARM,
        ..Default::default()
    };
    let (_, hits) = zenoir_tick(&mut ctx, &mut seats, WHO, |_| Some(120));
    let seats_hit: Vec<u8> = hits.iter().map(|h| h.seat).collect();
    assert_eq!(seats_hit, vec![3, 4, 6]);
    // 120 * 3/4 on the primary, 120 * 2/3 on the splash.
    assert_eq!(hits[0].applied, 90);
    assert_eq!(hits[1].applied, 80);
    assert_eq!(ctx.ctx_278, ZENOIR_FINISH_CTX_278);
}

#[test]
fn every_body_reaches_done_from_phase_zero() {
    for (entry, last) in SERU_TICK_LAST_ARM {
        let mut seats = row();
        let (frames, phase) = walk(|ctx| match entry {
            903 => gimard_tick(ctx, &mut seats, WHO, None).0,
            904 => {
                // The host's anim tick commits the clip arm 6 stages
                // before arm 7 polls it.
                let s = &mut seats[WHO.summon as usize];
                s.playing_anim = s.staged_anim;
                theeder_tick(
                    ctx,
                    &mut seats,
                    WHO,
                    TheederGeom::default(),
                    &mut TheederFx::default(),
                    |_| None,
                )
                .0
            }
            905 => vera_tick(ctx, &mut seats, WHO, None).0,
            906 => gizam_tick(ctx, &mut seats, WHO, |_| None).0,
            907 => nighto_tick(ctx, &mut seats, WHO, NightoOutcome::ConfuseResisted, true),
            _ => zenoir_tick(ctx, &mut seats, WHO, |_| None).0,
        });
        // Every chain walks its whole arm run before the terminal, and
        // none of them wraps. PROT 0905's arm 5 steps the phase by three,
        // so its run skips arms 6 and 7.
        let skipped = if entry == 905 { 2 } else { 0 };
        assert!(
            frames + skipped >= u32::from(last),
            "{entry} finished in {frames}"
        );
        assert!(phase >= last, "{entry} ended at phase {phase}");
    }
}

#[test]
fn a_phase_past_the_chain_is_done_rather_than_busy_forever() {
    // Retail returns 1 without advancing here; the port reports Done so a
    // host cannot be parked by a phase no arm names.
    for (entry, last) in SERU_TICK_LAST_ARM {
        let mut seats = row();
        let mut ctx = CastModuleCtx {
            phase: last + 1,
            ..Default::default()
        };
        let step = match entry {
            903 => gimard_tick(&mut ctx, &mut seats, WHO, None).0,
            904 => {
                theeder_tick(
                    &mut ctx,
                    &mut seats,
                    WHO,
                    TheederGeom::default(),
                    &mut TheederFx::default(),
                    |_| None,
                )
                .0
            }
            905 => vera_tick(&mut ctx, &mut seats, WHO, None).0,
            906 => gizam_tick(&mut ctx, &mut seats, WHO, |_| None).0,
            907 => nighto_tick(
                &mut ctx,
                &mut seats,
                WHO,
                NightoOutcome::ConfuseResisted,
                true,
            ),
            _ => zenoir_tick(&mut ctx, &mut seats, WHO, |_| None).0,
        };
        assert_eq!(step, CastTickStep::Done, "PROT {entry}");
    }
}

#[test]
fn seru_last_arm_answers_for_every_entry_in_the_set() {
    assert_eq!(seru_last_arm(903), Some(12));
    assert_eq!(seru_last_arm(908), Some(12));
    assert_eq!(seru_last_arm(909), None);
}

//
// Retail fixtures: numbers read off live PCSX-Redux captures of each body
// driven end to end in an ordinary random encounter (one party seat, one
// monster seat). Each row is `(wrapper argument, wrapper return, HP the
// victim actually lost)` read at the wrapper's entry and its `jr ra`, so
// the site's baked power, its scale and its clamp are all pinned by the
// same observation. `docs/subsystems/cast-module.md` § "Frame gating,
// measured" carries the per-arm dwell these runs also produced.

/// Every player-Seru site passes the **summon seat** `7` as the attacker,
/// not the caster seat: retail's `addiu a1, zero, 7` at `0x801F74A8`
/// (PROT 0903) and `0x801F8880` (PROT 0910) are literal constants, and the
/// capture reads `a1 = 7` at all four wrapper entries with `ctx[+0x13] = 0`.
#[test]
fn retail_sites_bake_the_power_the_capture_read() {
    for (site, power) in [
        (0x801F_74ACu32, 0x12u16), // PROT 0903, a0 = 0x12 read live
        (0x801F_7D38, 0x11),       // PROT 0904, a0 = 0x11 read live
        (0x801F_76CC, 0x12),       // PROT 0908 opener
        (0x801F_7C14, 0x10),       // PROT 0908 second site
    ] {
        let s = seru_hit_site(site).expect("site is in the table");
        assert_eq!(s.power(), power, "site 0x{site:08X}");
    }
}

/// PROT 0903's single hit: the wrapper returned `247` against a victim on
/// `76` HP and retail applied `76`, emptying the bar; against a victim on
/// `9999` HP it returned `417` and applied all of it.
#[test]
fn retail_gimard_hit_is_unscaled_and_kill_capable() {
    let site = seru_hit_site(0x801F_74AC).unwrap();
    let mut v = seat(76);
    assert_eq!(site.apply(&mut v, 247), 76);
    assert_eq!(v.hp, 0);
    let mut v = seat(9999);
    assert_eq!(site.apply(&mut v, 417), 417);
    assert_eq!(v.hp, 9999 - 417);
}

/// PROT 0904's ring sweep: wrapper return `371`, applied `371`. The same
/// state produced the same roll on two separate runs, which is what makes
/// it usable as a fixture.
#[test]
fn retail_theeder_ring_hit_is_unscaled() {
    let site = seru_hit_site(0x801F_7D38).unwrap();
    let mut v = seat(9999);
    assert_eq!(site.apply(&mut v, 371), 371);
}

/// PROT 0908's two reachable sites, measured in one cast: the opener
/// returned `522` and applied `130` (a quarter), the second returned `541`
/// and applied `405` (three quarters). The third site `0x801F7DF8` never
/// fired in the capture.
#[test]
fn retail_zenoir_scales_its_two_reachable_sites() {
    let opener = seru_hit_site(0x801F_76CC).unwrap();
    let mut v = seat(9999);
    assert_eq!(opener.apply(&mut v, 522), 130);
    let second = seru_hit_site(0x801F_7C14).unwrap();
    let mut v = seat(9999 - 130);
    assert_eq!(second.apply(&mut v, 541), 405);
}

/// PROT 0905's restore, measured twice: magic level `3`, target on `42` of
/// `999` HP, retail restored exactly `320` = `3 * 0x20 + 0xE0` with the
/// missing-HP clamp slack.
#[test]
fn retail_vera_restores_three_hundred_and_twenty_at_level_three() {
    assert_eq!(vera_heal_amount(3, 42, 999), 320);
    // And the clamp really is the missing HP, not the base: the same level
    // against Vahn's own 128-point bar restores only what is missing.
    assert_eq!(vera_heal_amount(3, 42, 128), 86);
}

/// The measured phase walks end on the arm this table names: PROT 0903 and
/// 0908 both latched `0xFF` out of arm `12`, PROT 0904 out of `14`, PROT
/// 0905 out of `10` and PROT 0907 out of `15`.
#[test]
fn retail_phase_walks_end_on_the_tabled_last_arm() {
    assert_eq!(seru_last_arm(903), Some(12));
    assert_eq!(seru_last_arm(904), Some(14));
    assert_eq!(seru_last_arm(905), Some(10));
    assert_eq!(seru_last_arm(907), Some(15));
    assert_eq!(seru_last_arm(908), Some(12));
}

/// PROT 0907's fork, as the capture caught it: the kill roll
/// `0x801F8534` read `6` and the resist word `0x801F853C` read `1`, so
/// [`nighto_outcome`] classifies it `Resisted` - and retail left the
/// victim's HP and `+0x16E` untouched, which is what `Resisted` means.
///
/// What the capture also showed, and this model cannot express, is that
/// retail's *phase target* forks on the kill roll alone (`beqz` at
/// `0x801F7E04`): a resisted cast whose kill roll is non-zero still takes
/// the confuse path's `sb 0xF, 0x279` at `0x801F7E28` and lands on arm
/// `15`, never on arm `14`. See the FINDINGS note on this module.
#[test]
fn retail_nighto_roll_six_with_resist_is_resisted() {
    let roll = NightoRoll {
        kill_roll: 6,
        resist_roll: 9,
        magic_level: 3,
        target_immune: false,
        extra_roll: None,
    };
    assert_eq!(nighto_outcome(&roll), NightoOutcome::ConfuseResisted);
}

/// The boss immunity is **forced**, not rolled. On the Gaza 2 fight the
/// two inputs the gate at `0x801F6BF0..0x801F6C24` reads both hold -
/// `ctx[+0x287] = 4` and the monster record's `+0x20 = 1` - so retail
/// branches to `0x801F6CB8`, stores a literal `1` into `0x801F853C` and
/// never draws the throw at `0x801F6C28`. The cast driven there reads that
/// `1` on the first tick after arm `0` and leaves the boss on 15000 HP.
/// `target_immune` is that branch, and it wins over any roll.
#[test]
fn retail_nighto_boss_immunity_beats_a_landing_roll() {
    let land = NightoRoll {
        kill_roll: 0,
        resist_roll: 0,
        magic_level: 3,
        target_immune: false,
        extra_roll: None,
    };
    assert_eq!(nighto_outcome(&land), NightoOutcome::Kill);
    let immune = NightoRoll {
        target_immune: true,
        ..land
    };
    // The forced `1` in `0x801F853C` suppresses the writes; the kill word
    // `0x801F8534` is untouched, so the cast still takes the death leg
    // and settles through arm 14 - which is why the boss sits on 15000 HP
    // rather than being confused.
    assert_eq!(nighto_outcome(&immune), NightoOutcome::KillResisted);
    assert!(nighto_outcome(&immune).resisted());
}
