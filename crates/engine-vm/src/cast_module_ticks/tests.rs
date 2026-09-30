use super::*;

fn actor(hp: u16) -> CastActorState {
    CastActorState {
        hp,
        anim_rate: ANIM_RATE_NORMAL,
        knockdown_anim: 0x11,
        ..Default::default()
    }
}

// --- the two clamp shapes -------------------------------------------

#[test]
fn shape_a_clamps_to_hp_and_floors_at_zero() {
    let mut v = actor(100);
    assert_eq!(apply_hit_floor_zero(&mut v, 40), 40);
    assert_eq!(v.hp, 60);
    assert_eq!(v.hp_bar_delta, 40);
    // Over-damage is clamped to the remaining HP, so HP lands exactly 0.
    assert_eq!(apply_hit_floor_zero(&mut v, 1000), 60);
    assert_eq!(v.hp, 0);
    assert_eq!(v.hp_bar_delta, 100);
}

#[test]
fn shape_a_treats_a_negative_roll_as_a_kill() {
    // `sltu` is unsigned, so a negative wrapper return compares above any
    // HP and the clamp rewrites it to the whole bar. This is retail.
    let mut v = actor(250);
    assert_eq!(apply_hit_floor_zero(&mut v, -5), 250);
    assert_eq!(v.hp, 0);
}

#[test]
fn shape_b_never_kills() {
    let mut v = actor(100);
    assert_eq!(apply_hit_floor_one(&mut v, 1000), 99);
    assert_eq!(v.hp, 1, "the HP-1 cap leaves the victim alive");
    // ... and at 1 HP the cap is 0, so a further hit does nothing.
    assert_eq!(apply_hit_floor_one(&mut v, 1000), 0);
    assert_eq!(v.hp, 1);
}

#[test]
fn shape_b_heals_on_a_negative_roll() {
    // `slt` is signed, so a negative roll passes the cap unclamped and
    // the subtract raises HP - the opposite of shape A.
    let mut v = actor(100);
    assert_eq!(apply_hit_floor_one(&mut v, -20), -20);
    assert_eq!(v.hp, 120);
}

// --- the phase discipline -------------------------------------------

#[test]
fn a_tick_advances_the_phase_exactly_once_and_parks_on_the_terminal_arm() {
    let mut ctx = CastModuleCtx::default();
    let mut caster = actor(100);
    let mut victim = actor(500);
    for expect in 1..=ASTRAL_SLASH_TERMINAL_ARM {
        assert_eq!(
            astral_slash_tick(&mut ctx, &mut caster, &mut victim),
            CastTickStep::Busy
        );
        assert_eq!(ctx.phase, expect);
    }
    // Arm 4 holds: the tick still reports Busy, but the phase stays.
    for _ in 0..3 {
        assert_eq!(
            astral_slash_tick(&mut ctx, &mut caster, &mut victim),
            CastTickStep::Busy
        );
        assert_eq!(ctx.phase, ASTRAL_SLASH_TERMINAL_ARM);
    }
}

#[test]
fn a_phase_past_the_bound_falls_through_and_writes_nothing() {
    let mut ctx = CastModuleCtx {
        phase: 5,
        ..Default::default()
    };
    let mut caster = actor(100);
    let mut victim = actor(500);
    let before = (caster, victim, ctx);
    assert_eq!(
        astral_slash_tick(&mut ctx, &mut caster, &mut victim),
        CastTickStep::Done
    );
    assert_eq!((caster, victim, ctx), before);
}

#[test]
fn astral_slash_drops_and_restores_the_animation_rate() {
    let mut ctx = CastModuleCtx {
        phase: 2,
        ..Default::default()
    };
    let mut caster = actor(100);
    let mut victim = actor(500);
    astral_slash_tick(&mut ctx, &mut caster, &mut victim);
    assert_eq!(caster.staged_anim, ASTRAL_SLASH_ARM2_CLIP);
    assert_eq!(caster.restage, 1);
    assert_eq!((caster.anim_rate, victim.anim_rate), (1, 1));
    // Arm 3 restores the caster and leaves the victim slowed.
    astral_slash_tick(&mut ctx, &mut caster, &mut victim);
    assert_eq!(caster.staged_anim, 0);
    assert_eq!(caster.restage, 1, "arm 3's `+0x1DA` store is unpaired");
    assert_eq!((caster.anim_rate, victim.anim_rate), (ANIM_RATE_NORMAL, 2));
}

#[test]
fn the_confirm_gate_holds_the_phase_until_the_clip_plays() {
    let mut ctx = CastModuleCtx {
        phase: PLASMA_STRIKE_CONFIRM_PHASE,
        ..Default::default()
    };
    let mut caster = actor(100);
    let mut victim = actor(500);
    for _ in 0..4 {
        assert_eq!(
            plasma_strike_tick(&mut ctx, &mut caster, &mut victim, None),
            CastTickStep::Busy
        );
        assert_eq!(ctx.phase, PLASMA_STRIKE_CONFIRM_PHASE);
        assert_eq!(caster.staged_anim, PLASMA_STRIKE_CONFIRM_CLIP);
    }
    // Once the commit mirrors the id into `+0x1D9` the gate passes.
    caster.playing_anim = PLASMA_STRIKE_CONFIRM_CLIP;
    assert_eq!(
        plasma_strike_tick(&mut ctx, &mut caster, &mut victim, None),
        CastTickStep::Busy
    );
    assert_eq!(ctx.phase, PLASMA_STRIKE_CONFIRM_PHASE + 1);
}

#[test]
fn a_bypass_hit_lands_through_the_tick_and_clamps_at_zero() {
    let mut ctx = CastModuleCtx::default();
    let mut victim = actor(200);
    blazing_slash_tick(&mut ctx, &mut victim, Some((0, 90)));
    assert_eq!(victim.hp, 110);
    assert_eq!(victim.staged_anim, 0x11, "the victim's own +0x1F1 reaction");
    // A site index past the six baked powers stages nothing.
    blazing_slash_tick(&mut ctx, &mut victim, Some((9, 90)));
    assert_eq!(victim.hp, 110);
}

// --- the seven stagers ----------------------------------------------

#[test]
fn the_water_crystals_ramp_pairs_speed_with_rate() {
    // The literal arms as disassembled, not the implementation's own
    // formula restated: `(slot, +0x0C, +0x21D)` for table slots 0..7 at
    // `0x801F761C` / `7630` / `7644` / `7658` / `766C` / `7680` / `7694` /
    // `76A8`.
    const ARMS: [(u8, i32, u8); 8] = [
        (0, 0x200, 7),
        (1, 0x400, 6),
        (2, 0x600, 5),
        (3, 0x800, 4),
        (4, 0xA00, 3),
        (5, 0xC00, 2),
        (6, 0xE00, 1),
        (7, 0x1000, 0),
    ];
    for (arm, speed, rate) in ARMS {
        let mut v = actor(100);
        water_crystals_stager(&mut v, arm);
        assert_eq!(v.root_speed, speed, "arm {arm} +0x0C");
        assert_eq!(v.anim_rate, rate, "arm {arm} +0x21D");
    }
    // Arm 8 is past `sltiu a1, 8` and writes nothing.
    let mut v = actor(100);
    let before = v;
    water_crystals_stager(&mut v, 8);
    assert_eq!(v, before);
}

#[test]
fn the_puera_stager_is_arm_zero_only() {
    let mut ctx = CastModuleCtx::default();
    puera_stager(&mut ctx, 1);
    assert_eq!(ctx.ctx_278, 0, "`bnez a1` skips the whole body");
    puera_stager(&mut ctx, 0);
    assert_eq!(ctx.ctx_278, 3);
}

#[test]
fn the_gilium_stager_writes_ctx_278_on_arm_zero() {
    let mut ctx = CastModuleCtx::default();
    gilium_stager(&mut ctx, 2);
    assert_eq!(ctx.ctx_278, 0);
    gilium_stager(&mut ctx, 0);
    assert_eq!(ctx.ctx_278, 3);
}

#[test]
fn the_gizam_stager_splits_the_pose_and_the_advance() {
    let mut ctx = CastModuleCtx::default();
    let mut seat = CastActorState {
        render_flag: 0xFF,
        ..actor(1)
    };
    gizam_stager(&mut ctx, &mut seat, 1);
    assert_eq!((seat.render_flag, seat.root_speed), (0, 0x1000));
    assert_eq!(ctx.phase, 0, "arm 1 poses only");
    gizam_stager(&mut ctx, &mut seat, 2);
    assert_eq!(ctx.phase, 1, "arm 2 is the advance");
}

#[test]
fn the_viguro_stager_retargets_the_summon_seat() {
    let mut ctx = CastModuleCtx::default();
    let mut seat = CastActorState {
        target_code: 2,
        render_flag: 0xFF,
        ..actor(1)
    };
    assert_eq!(viguro_stager(&mut ctx, &mut seat, 0), Some(2));
    assert_eq!(seat.target_code, TARGET_CODE_ENEMY_ROW);
    assert_eq!(seat.render_flag, 0);
    assert_eq!(seat.root_speed, 0x1000);
    assert_eq!(ctx.phase, 1);
    // Any other arm leaves the seat alone.
    assert_eq!(viguro_stager(&mut ctx, &mut seat, 3), None);
}

#[test]
fn the_esm_sweep_covers_the_whole_table_and_leaves_everyone_alive() {
    let ctx = CastModuleCtx {
        party_count: 5,
        caster_seat: 3,
        ..Default::default()
    };
    let mut seats = vec![actor(100), actor(100), actor(0), actor(100), actor(100)];
    // Seat 3 is non-targetable, seat 2 is dead: both are skipped.
    seats[3].flags = FLAG_NON_TARGETABLE;
    let hits = evil_seru_magic_stager(&ctx, &mut seats, 4, |_| 9999);
    assert_eq!(
        hits.iter().map(|h| h.seat).collect::<Vec<_>>(),
        vec![0, 1, 4]
    );
    for seat in [0usize, 1, 4] {
        assert_eq!(seats[seat].hp, 1, "the HP-1 cap");
        assert_eq!(seats[seat].staged_anim, 0x11, "its own +0x1F1 reaction");
        assert_eq!(seats[seat].restage, 1);
        assert_eq!(seats[seat].anim_rate, 2);
    }
    assert_eq!(seats[2].hp, 0, "a dead seat is untouched");
    assert_eq!(seats[3].hp, 100, "a non-targetable seat is untouched");
    // Arm 9 is past `sltiu a1, 9`.
    let mut fresh = vec![actor(100); 5];
    assert!(evil_seru_magic_stager(&ctx, &mut fresh, 9, |_| 50).is_empty());
    assert_eq!(fresh[0].hp, 100);
}

#[test]
fn the_juggernaut_sweep_starts_at_seat_three() {
    let ctx = CastModuleCtx {
        monster_count: 2,
        ..Default::default()
    };
    let mut seats = vec![actor(100); 6];
    let hits = juggernaut_stager(&ctx, &mut seats, 0, |_| 50);
    assert_eq!(hits.iter().map(|h| h.seat).collect::<Vec<_>>(), vec![3, 4]);
    for seat in seats.iter().take(3) {
        assert_eq!(seat.hp, 100, "the party row is not swept");
    }
    assert_eq!(seats[3].hp, 50);
    assert_eq!(seats[4].hp, 50);
    assert_eq!(seats[5].hp, 100, "bounded by ctx[+1]");
    // Unlike ESM, Juggernaut stages no reaction clip.
    assert_eq!(seats[3].staged_anim, 0);
    assert_eq!(seats[3].anim_rate, ANIM_RATE_NORMAL);
}

// --- the tables ------------------------------------------------------

#[test]
fn every_damage_shape_names_a_band_entry_and_at_least_one_power() {
    for s in CAST_DAMAGE_SHAPES {
        assert!((903..=966).contains(&s.prot_entry), "{s:?}");
        assert!(!s.powers.is_empty(), "{s:?}");
        assert!(s.routine >= CAST_MODULE_LINK_BASE, "{s:?}");
    }
    assert_eq!(baked_power_for(960), Some(0x1C0), "Plasma Strike's burst");
    // Megaton Press: three bypass-wrapper sites, `0x80 / 0x80 / 0x30`,
    // the first being the seed a single-hit fold uses.
    assert_eq!(baked_power_for(959), Some(0x80));
    assert_eq!(damage_shape_for(959).unwrap().powers, &MEGATON_PRESS_POWERS);
    assert!(!damage_shape_for(959).unwrap().never_kills);
    assert_eq!(baked_power_for(958), Some(0x30));
    assert_eq!(damage_shape_for(958).unwrap().powers, &BLAZING_SLASH_POWERS);
    assert_eq!(baked_power_for(903), None, "not a PORT row");
    // Only the two AoE stagers use the never-kill clamp.
    let never: Vec<u32> = CAST_DAMAGE_SHAPES
        .iter()
        .filter(|s| s.never_kills)
        .map(|s| s.prot_entry)
        .collect();
    assert_eq!(never, vec![927, 966]);
}

#[test]
fn every_tick_shape_names_a_band_entry() {
    for s in CAST_TICK_SHAPES {
        assert!((903..=966).contains(&s.prot_entry), "{s:?}");
        assert!(s.phase_arms > 0, "{s:?}");
    }
    assert_eq!(tick_shape_for(0x801F_6DD8).unwrap().phase_arms, 0x100);
    assert!(tick_shape_for(0x801F_6DD8).unwrap().table_head);
    assert!(!tick_shape_for(0x801F_74E4).unwrap().table_head);
    assert!(tick_shape_for(0x801F_9999).is_none());
}
