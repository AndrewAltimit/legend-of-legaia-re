use super::*;

fn ctx_with(phase: u8, actors: u8, monsters: u8) -> CastModuleCtx {
    CastModuleCtx {
        party_count: actors,
        monster_count: monsters,
        caster_seat: 3,
        phase,
        ..Default::default()
    }
}

fn actor(hp: u16) -> CastActorState {
    CastActorState {
        hp,
        anim_rate: ANIM_RATE_NORMAL,
        knockdown_anim: 0x0C,
        ..Default::default()
    }
}

#[test]
fn the_three_shared_body_vas_are_one_value_and_three_arms() {
    assert_eq!(STEAL_SWEEP_TICK, CURSE_MP_DRAIN_TICK);
    assert_eq!(STEAL_SWEEP_TICK, GUILTY_CROSS_TICK);
    // ...and the damage shape is keyed on the pair, so the same VA gives
    // three different answers.
    assert_eq!(
        arm_damage_shape_for(941, STEAL_SWEEP_TICK).map(|s| s.powers[0]),
        Some(0x00C0)
    );
    assert!(arm_damage_shape_for(943, CURSE_MP_DRAIN_TICK).is_none());
    assert_eq!(
        arm_damage_shape_for(944, GUILTY_CROSS_TICK).map(|s| s.powers[0]),
        Some(0x038E)
    );
}

#[test]
fn steal_carries_no_damage_wrapper() {
    // The doc's fourteen-arm table grades `0x801F730C` as one wrapper;
    // the body reaches none.
    assert!(arm_damage_shape_for(941, STEAL_TICK).is_none());
}

#[test]
fn the_glare_blind_arm_blanks_the_first_monster_seats_reaction_run() {
    let mut ctx = ctx_with(4, 4, 1);
    let mut seat = CastActorState {
        reaction_alt: 3,
        reaction_alt2: 4,
        knockdown_anim: 5,
        reaction_gate: 6,
        ..Default::default()
    };
    let mut ext = CastArmExtState {
        reaction_extra: 7,
        ..Default::default()
    };
    assert_eq!(
        glare_divide_blind_tick(&mut ctx, &mut seat, &mut ext),
        CastTickStep::Busy
    );
    assert_eq!(seat.reaction_alt, 0);
    assert_eq!(seat.reaction_alt2, 0);
    assert_eq!(seat.knockdown_anim, 0);
    assert_eq!(seat.reaction_gate, 0);
    assert_eq!(ext.reaction_extra, 0);
    assert_eq!(ctx.phase, 5, "the arm advances");
}

#[test]
fn the_glare_blind_terminal_arm_parks_and_reports_done() {
    let mut ctx = ctx_with(7, 4, 1);
    ctx.ctx_0d = 1;
    let mut seat = actor(100);
    let mut ext = CastArmExtState::default();
    assert_eq!(
        glare_divide_blind_tick(&mut ctx, &mut seat, &mut ext),
        CastTickStep::Done
    );
    assert_eq!(ctx.phase, 7, "the terminal arm does not advance");
    assert_eq!(ctx.ctx_0d, 0);
}

#[test]
fn the_split_allocates_the_next_monster_seat_and_burns_a_turn() {
    let mut ctx = ctx_with(2, 5, 2);
    let mut caster = actor(250);
    caster.target_code = 1;
    let (step, split) = glare_divide_split_tick(
        &mut ctx,
        &mut caster,
        &CastArmExtState::default(),
        0x50,
        0,
        None,
    );
    let split = split.expect("arm 2 splits");
    assert_eq!(step, CastTickStep::Busy);
    assert_eq!(split.clone_seat, FIRST_MONSTER_SEAT + 2);
    assert_eq!(split.clone_hp, 250);
    assert_eq!(split.saved_caster_target, 1);
    assert_eq!(split.weakened, None, "0x50 splits at full HP");
    assert_eq!(ctx.monster_count, 3);
    assert_eq!(ctx.turn_cursor, 1);
    assert_eq!(caster.target_code, 9);
    assert_eq!(caster.render_flag, 0);
}

#[test]
fn the_0xae_split_can_weaken_the_caster_itself() {
    let mut ctx = ctx_with(2, 5, 1);
    let mut caster = actor(250);
    caster.atk = 90;
    caster.agl = 400;
    let (_, split) = glare_divide_split_tick(
        &mut ctx,
        &mut caster,
        &CastArmExtState::default(),
        0xAE,
        0,
        Some(1),
    );
    assert_eq!(split.unwrap().weakened, Some(SplitWeakened::Caster));
    assert_eq!(caster.hp, 1, "the coin landed on the original");
    assert_eq!(caster.atk, 1);
    assert_eq!(caster.agl, SPLIT_WEAK_AGL);
    assert_eq!(caster.agl_base, SPLIT_WEAK_AGL);

    let mut ctx = ctx_with(2, 5, 1);
    let mut caster = actor(250);
    let (_, split) = glare_divide_split_tick(
        &mut ctx,
        &mut caster,
        &CastArmExtState::default(),
        0xAE,
        0,
        Some(0),
    );
    assert_eq!(split.unwrap().weakened, Some(SplitWeakened::Clone));
    assert_eq!(caster.hp, 250, "the coin landed on the copy");
}

#[test]
fn the_split_latches_ff_and_restores_the_casters_target() {
    let mut ctx = ctx_with(3, 5, 1);
    let mut caster = actor(250);
    caster.target_code = 9;
    let (step, _) = glare_divide_split_tick(
        &mut ctx,
        &mut caster,
        &CastArmExtState::default(),
        0x50,
        4,
        None,
    );
    assert_eq!(step, CastTickStep::Busy);
    assert_eq!(ctx.phase, LATCHED_DONE_PHASE, "arm 3 latches, not advances");
    let (step, _) = glare_divide_split_tick(
        &mut ctx,
        &mut caster,
        &CastArmExtState::default(),
        0x50,
        4,
        None,
    );
    assert_eq!(step, CastTickStep::Done);
    assert_eq!(caster.target_code, 4);
}

#[test]
fn the_steal_sweep_kills_and_stages_the_knockdown() {
    let mut ctx = ctx_with(2, 4, 1);
    let mut seats = vec![actor(40), actor(0), actor(500), actor(9000)];
    seats[3].flags = crate::cast_module_ticks::FLAG_NON_TARGETABLE;
    let (step, hits) = steal_sweep_tick(&mut ctx, &mut seats, true, |_| 60);
    assert_eq!(step, CastTickStep::Busy);
    // Seat 1 is dead and seat 3 is non-targetable, so the sweep visits 0
    // and 2 only.
    assert_eq!(hits.len(), 2);
    assert_eq!(seats[0].hp, 0, "the unsigned clamp lets the hit kill");
    assert_eq!(hits[0].applied, 40, "clamped to the victim's live HP");
    assert_eq!(seats[2].hp, 440);
    assert_eq!(seats[0].staged_anim, 0x0C);
    assert_eq!(seats[1].anim_rate, 2, "even a skipped seat gets +0x21D");
}

#[test]
fn a_negative_roll_kills_on_this_bands_unsigned_clamp() {
    let mut v = actor(300);
    let applied = apply_arm_hit(&mut v, -5, 1, 2);
    assert_eq!(applied, 300);
    assert_eq!(v.hp, 0);
}

#[test]
fn the_mp_drain_empties_every_seat_and_stashes_the_old_value() {
    let mut ctx = ctx_with(2, 4, 1);
    let mut seats = vec![actor(10), actor(0), actor(80), actor(90)];
    let mut exts = vec![
        CastArmExtState {
            mp: 30,
            mp_base: 30,
            ..Default::default()
        },
        CastArmExtState {
            mp: 12,
            mp_base: 12,
            ..Default::default()
        },
        CastArmExtState::default(),
        CastArmExtState {
            mp: 7,
            mp_base: 7,
            ..Default::default()
        },
    ];
    let (step, drained) = curse_mp_drain_tick(&mut ctx, &mut seats, &mut exts);
    assert_eq!(step, CastTickStep::Busy);
    // No liveness guard: seat 1 is dead and still drains.
    assert_eq!(drained.len(), 4);
    assert_eq!(drained[1], (1, 12));
    assert!(exts.iter().all(|e| e.mp == 0 && e.mp_base == 0));
    assert_eq!(exts[0].mp_stash, 30);
}

#[test]
fn the_scoped_curse_marks_the_row_the_target_code_names() {
    let mut seats = vec![actor(10), actor(20), actor(30), actor(40), actor(0)];
    // `8` - the low row `0 .. ctx[+0]`.
    let mut ctx = ctx_with(3, 3, 2);
    let mut caster = actor(99);
    let (_, marked) = guilty_cross_curse_tick(&mut ctx, 8, &mut caster, &mut seats);
    assert_eq!(marked, vec![0, 1, 2]);
    // `9` - the monster row, and the dead seat 4 is skipped.
    let mut ctx = ctx_with(3, 3, 2);
    let mut seats = vec![actor(10), actor(20), actor(30), actor(40), actor(0)];
    let (_, marked) = guilty_cross_curse_tick(&mut ctx, 9, &mut caster, &mut seats);
    assert_eq!(marked, vec![3]);
    assert_eq!(seats[3].flags & FLAG_CURSED, FLAG_CURSED);
    // A code below 7 is one seat, with no liveness guard.
    let mut ctx = ctx_with(3, 3, 2);
    let mut seats = vec![actor(10), actor(20), actor(30), actor(40), actor(0)];
    let (_, marked) = guilty_cross_curse_tick(&mut ctx, 4, &mut caster, &mut seats);
    assert_eq!(marked, vec![4]);
}

#[test]
fn guilty_cross_applies_its_latched_roll_on_arm_three_only() {
    let mut ctx = ctx_with(0, 4, 1);
    let mut caster = actor(500);
    let mut victim = actor(800);
    for _ in 0..3 {
        guilty_cross_tick(&mut ctx, &mut caster, &mut victim, Some(200));
    }
    assert_eq!(victim.hp, 800, "arms 0..2 do not apply");
    assert_eq!(ctx.phase, 3);
    guilty_cross_tick(&mut ctx, &mut caster, &mut victim, Some(200));
    assert_eq!(victim.hp, 600);
    assert_eq!(victim.restage, 1, "an assignment, not a bump");
    assert_eq!(victim.staged_anim, 0x0C);
}

#[test]
fn rolling_flares_tail_waits_on_the_victims_clip() {
    let mut ctx = ctx_with(4, 4, 1);
    let mut caster = actor(500);
    let mut victim = actor(120);
    victim.playing_anim = 3;
    assert_eq!(
        rolling_flare_tick(&mut ctx, &mut caster, &mut victim, None),
        CastTickStep::Busy
    );
    assert_eq!(ctx.phase, 4, "it holds");
    victim.playing_anim = 0;
    assert_eq!(
        rolling_flare_tick(&mut ctx, &mut caster, &mut victim, None),
        CastTickStep::Done
    );
    // A dead victim waits for clip 8, not 0.
    let mut ctx = ctx_with(4, 4, 1);
    let mut victim = actor(0);
    victim.playing_anim = 0;
    assert_eq!(
        rolling_flare_tick(&mut ctx, &mut caster, &mut victim, None),
        CastTickStep::Busy
    );
    victim.playing_anim = 8;
    assert_eq!(
        rolling_flare_tick(&mut ctx, &mut caster, &mut victim, None),
        CastTickStep::Done
    );
}

#[test]
fn the_rolling_flare_sweep_runs_on_arm_ten() {
    assert_eq!(arm_sweep_arm(950, ROLLING_FLARE_SWEEP_TICK), Some(10));
    let mut ctx = ctx_with(10, 3, 0);
    let mut caster = actor(500);
    let mut seats = vec![actor(100), actor(100), actor(100)];
    let (step, hits) = rolling_flare_sweep_tick(&mut ctx, &mut caster, &mut seats, true, |_| 25);
    assert_eq!(step, CastTickStep::Busy);
    assert_eq!(hits.len(), 3);
    assert!(seats.iter().all(|s| s.hp == 75 && s.anim_rate == 4));
    assert_eq!(caster.anim_rate, 2);
}

#[test]
fn water_hazard_picks_its_row_off_the_target_code() {
    // The monster leg skips the caster's own seat.
    let mut ctx = ctx_with(2, 3, 3);
    let mut caster = actor(900);
    let mut seats = vec![
        actor(50),
        actor(50),
        actor(50),
        actor(500),
        actor(500),
        actor(500),
    ];
    let (step, hits) = water_hazard_tick(
        &mut ctx,
        4,
        9,
        &mut caster,
        &mut seats,
        |_| 100,
        |_| 1, // never 0 mod 8: no status
    );
    assert_eq!(step, CastTickStep::Busy);
    assert_eq!(
        hits.iter().map(|h| h.seat).collect::<Vec<_>>(),
        vec![3, 5],
        "seat 4 is the caster"
    );
    assert!(
        seats
            .iter()
            .all(|s| s.flags & FLAG_WATER_HAZARD_STATUS == 0)
    );
    // The low leg sweeps `0 .. ctx[+0]` and stages the knockdown.
    let mut ctx = ctx_with(2, 3, 3);
    let mut seats = vec![actor(50), actor(50), actor(50)];
    let (_, hits) = water_hazard_tick(
        &mut ctx,
        4,
        8,
        &mut caster,
        &mut seats,
        |_| 20,
        |_| 8, // 8 & 7 == 0: the status lands
    );
    assert_eq!(hits.len(), 3);
    assert!(seats.iter().all(|s| s.hp == 30 && s.restage == 1));
    assert!(
        seats
            .iter()
            .all(|s| s.flags & FLAG_WATER_HAZARD_STATUS != 0)
    );
}

#[test]
fn water_hazards_monster_leg_picks_the_alternate_reaction() {
    let mut ctx = ctx_with(2, 3, 2);
    let mut caster = actor(900);
    let mut seats = vec![
        actor(10),
        actor(10),
        actor(10),
        CastActorState {
            hp: 400,
            knockdown_anim: 0x0C,
            reaction_alt: 0x07,
            reaction_gate: 0,
            anim_rate: ANIM_RATE_NORMAL,
            ..Default::default()
        },
        CastActorState {
            hp: 400,
            knockdown_anim: 0x0C,
            reaction_alt: 0x07,
            reaction_gate: 1,
            anim_rate: ANIM_RATE_NORMAL,
            ..Default::default()
        },
    ];
    water_hazard_tick(&mut ctx, 9, 9, &mut caster, &mut seats, |_| 10, |_| 1);
    assert_eq!(seats[3].staged_anim, 0x07, "gate clear takes +0x1EF");
    assert_eq!(seats[3].restage, 5);
    assert_eq!(seats[4].staged_anim, 0x0C, "gate set takes +0x1F1");
    assert_eq!(seats[4].restage, 1);
}

#[test]
fn the_three_blade_breaths_differ_only_in_four_constants() {
    assert_eq!(BLADE_BREATH_A.windup_clip, 5);
    assert_eq!(BLADE_BREATH_B.windup_clip, 6);
    assert_eq!(BLADE_BREATH_C.windup_clip, 7);
    for (entry, body) in [
        (962, BLADE_BREATH_A_TICK),
        (962, BLADE_BREATH_B_TICK),
        (962, BLADE_BREATH_C_TICK),
    ] {
        assert_eq!(
            arm_damage_shape_for(entry, body).map(|s| s.powers[0]),
            Some(BLADE_BREATH_POWER)
        );
    }
}

#[test]
fn blade_breath_skips_the_walk_when_the_caster_has_arrived() {
    let mut ctx = ctx_with(0, 4, 1);
    let mut caster = actor(500);
    let mut victim = actor(700);
    // Not arrived: arm 0 stores phase 1 and holds there with the run clip.
    blade_breath_a_tick(&mut ctx, &mut caster, &mut victim, false, None);
    assert_eq!(ctx.phase, 1);
    assert_eq!(caster.staged_anim, 1);
    // Arrived: arm 0 stores 1 and advances past it.
    let mut ctx = ctx_with(0, 4, 1);
    blade_breath_a_tick(&mut ctx, &mut caster, &mut victim, true, None);
    assert_eq!(ctx.phase, 2);
}

#[test]
fn blade_breath_applies_its_0x200_hit_and_latches_ff() {
    let mut ctx = ctx_with(3, 4, 1);
    let mut caster = actor(500);
    let mut victim = actor(700);
    blade_breath_c_tick(&mut ctx, &mut caster, &mut victim, true, Some(250));
    assert_eq!(victim.hp, 450);
    assert_eq!(victim.anim_rate, 4);
    assert_eq!(ctx.phase, 4);
    victim.playing_anim = 0;
    let step = blade_breath_c_tick(&mut ctx, &mut caster, &mut victim, true, None);
    assert_eq!(step, CastTickStep::Busy);
    assert_eq!(ctx.phase, LATCHED_DONE_PHASE);
    let step = blade_breath_c_tick(&mut ctx, &mut caster, &mut victim, true, None);
    assert_eq!(step, CastTickStep::Done);
}

#[test]
fn a_phase_past_a_table_bodys_bound_reports_done() {
    let mut ctx = ctx_with(9, 4, 1);
    let mut seat = actor(10);
    let mut ext = CastArmExtState::default();
    assert_eq!(
        glare_divide_blind_tick(&mut ctx, &mut seat, &mut ext),
        CastTickStep::Done,
        "retail reports Busy here; the port reports Done (anti-softlock)"
    );
}

#[test]
fn the_steal_bag_draw_rejects_empty_and_invalid_slots() {
    let mut bag = vec![(0u8, 0u8); 256];
    bag[7] = (0x20, 3);
    bag[9] = (0x21, 0); // a zero count is rejected
    bag[11] = (0x22, 4); // an invalid item id is rejected
    let mut seq = [11u32, 9, 7].into_iter().cycle();
    let slot = steal_pick_bag_slot(&bag, None, || seq.next().unwrap(), |id| id != 0x22);
    assert_eq!(slot, Some(7));
    // An all-empty bag exhausts the 0x400 budget.
    let empty = vec![(0u8, 0u8); 256];
    assert_eq!(
        steal_pick_bag_slot(&empty, None, || 5, |_| true),
        None,
        "0x400 draws and no valid slot"
    );
}

#[test]
fn the_steal_body_reports_its_outcome_on_arm_one() {
    let mut ctx = ctx_with(0, 4, 1);
    let mut caster = actor(400);
    let out = StealOutcome::FromBag { item: 0x20 };
    let (step, taken) = steal_tick(&mut ctx, &mut caster, 6, Some(out));
    assert_eq!(step, CastTickStep::Busy);
    assert_eq!(taken, None, "arm 0 only stages");
    assert_eq!(caster.staged_anim, 6);
    assert_eq!(caster.restage & 1, 1, "an OR, not a bump");
    assert_eq!(ctx.ctx_0d, 1);
    let (_, taken) = steal_tick(&mut ctx, &mut caster, 6, Some(out));
    assert_eq!(taken, Some(out));
}

#[test]
fn every_arm_maps_to_exactly_one_owner() {
    // The (entry, body) pairs the trampoline table names for the seven
    // modules this file carries.
    use crate::cast_module_ticks::capture_tick_body;
    for (entry, id, body) in [
        (940u32, 0xACu8, GLARE_DIVIDE_BLIND_TICK),
        (940, 0x50, GLARE_DIVIDE_SPLIT_TICK),
        (940, 0xAE, GLARE_DIVIDE_SPLIT_TICK),
        (941, 0x51, STEAL_TICK),
        (941, 0xB9, STEAL_SWEEP_TICK),
        (943, 0x40, CURSE_SINGLE_TICK),
        (943, 0xB5, CURSE_MP_DRAIN_TICK),
        (944, 0x37, GUILTY_CROSS_TICK),
        (944, 0x53, GUILTY_CROSS_CURSE_TICK),
        (950, 0x5A, ROLLING_FLARE_TICK),
        (950, 0xAB, ROLLING_FLARE_SWEEP_TICK),
        (956, 0x71, WATER_HAZARD_TICK),
        (962, 0xA2, BLADE_BREATH_A_TICK),
        (962, 0xA3, BLADE_BREATH_B_TICK),
        (962, 0xA4, BLADE_BREATH_C_TICK),
    ] {
        assert_eq!(
            capture_tick_body(entry, id),
            Some(body),
            "PROT {entry} id {id:#04X}"
        );
    }
}
