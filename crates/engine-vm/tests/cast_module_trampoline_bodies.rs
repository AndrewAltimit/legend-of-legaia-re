//! Property tests for the twelve trampoline-reached slot-B tick bodies and
//! the battle overlay's effect-child hit arm.
//!
//! Each assertion is the *intent* of one instruction run, read off the owning
//! image's bytes at slot-B base `0x801F69D8` (and, for the hit arm, the
//! battle overlay at `0x801CE818`):
//!
//! * the phase byte advances **once** per tick, and only a terminal arm
//!   reports done;
//! * a damage body's clamp is shape A (`sltu`), which kills;
//! * a status writer sets exactly the bits the bytes set;
//! * a buff / debuff writes **both** halfwords of every stat pair it touches.
//!
//! Disc-free by construction - these are kernels over plain structs.

use legaia_engine_vm::battle_cast_census::{
    EFFECT_CHILD_STONE, EffectChildVictim, RESTAGE_BIT_FACE, RESTAGE_BIT_REACTION, effect_child_hit,
};
use legaia_engine_vm::cast_module_ticks::*;

fn ctx(phase: u8, actors: u8) -> CastModuleCtx {
    CastModuleCtx {
        actor_count: actors,
        monster_count: actors.saturating_sub(3),
        caster_seat: 3,
        phase,
        ..Default::default()
    }
}

fn actor(hp: u16) -> CastActorState {
    CastActorState {
        hp,
        anim_rate: ANIM_RATE_NORMAL,
        knockdown_anim: 0x0B,
        reaction_alt: 0x02,
        reaction_alt2: 0x03,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// The phase discipline
// ---------------------------------------------------------------------------

/// Every one of these bodies seeds its busy register with `1` and only a
/// terminal arm zeroes it, so a phase the dispatch never names still reports
/// busy - the opposite of "past the bound means done".
#[test]
fn an_unnamed_phase_reports_busy_not_done() {
    let mut c = ctx(0x20, 7);
    let mut caster = actor(100);
    let mut seats = vec![actor(100); 7];
    let (step, hits) = chaos_breath_tick(&mut c, &mut caster, &mut seats, |_| 0, |_| (1, 1));
    assert_eq!(step, CastTickStep::Busy);
    assert!(hits.is_empty());
    assert_eq!(c.phase, 0x21, "and it still advanced once");

    let mut c = ctx(0x40, 7);
    let mut v = actor(100);
    assert_eq!(
        chaos_flare_tick(&mut c, &mut v, None),
        CastTickStep::Busy,
        "past the twelve-arm table, retail's beqz lands past the clear"
    );
    assert_eq!(c.phase, 0x40, "and out of range it holds instead");
}

/// One `ctx[+0x279] += 1` per tick, never two - the property that makes the
/// module phase walk one step per frame.
#[test]
fn the_phase_advances_exactly_once_per_tick() {
    for arm in 0..6u8 {
        let mut c = ctx(arm, 7);
        let mut caster = actor(100);
        let mut seats = vec![actor(100); 7];
        let before = c.phase;
        let (_, _) = chaos_breath_tick(&mut c, &mut caster, &mut seats, |_| 5, |_| (1, 1));
        assert_eq!(c.phase, before.wrapping_add(1), "arm {arm}");
    }
}

/// The terminal arm is the only one that reports done, and PROT 0938's also
/// undoes the rate halving its arm 0 applied.
#[test]
fn the_terminal_arm_restores_the_rate_and_finishes() {
    let mut c = ctx(0, 7);
    let mut caster = actor(100);
    let mut seats = vec![actor(100); 7];
    chaos_breath_tick(&mut c, &mut caster, &mut seats, |_| 0, |_| (1, 1));
    assert_eq!(caster.anim_rate, ANIM_RATE_NORMAL / 2, "arm 0 halves it");
    assert_eq!(caster.staged_anim, CHAOS_BREATH_ARM0_CLIP);

    let mut c = ctx(0xFF, 7);
    let (step, _) = chaos_breath_tick(&mut c, &mut caster, &mut seats, |_| 0, |_| (1, 1));
    assert_eq!(step, CastTickStep::Done);
    assert_eq!(
        caster.anim_rate, ANIM_RATE_NORMAL,
        "and the 0xFF arm doubles"
    );
    assert_eq!(c.phase, 0xFF, "a finishing arm does not advance");
}

// ---------------------------------------------------------------------------
// The clamp shape
// ---------------------------------------------------------------------------

/// Shape A: unsigned `sltu` against live HP, so an over-roll takes the whole
/// bar and a **negative** roll reads as huge and kills outright. All six
/// damage bodies in this section use it.
#[test]
fn every_trampoline_body_clamps_shape_a() {
    // Over-roll -> the bar, not more.
    let mut c = ctx(0, 7);
    let mut v = actor(40);
    chaos_flare_tick(&mut c, &mut v, Some(500));
    assert_eq!((v.hp, v.hp_bar_delta), (0, 40));

    // Negative roll -> compares above any HP unsigned, so it kills.
    let mut c = ctx(0, 7);
    let mut v = actor(4000);
    scythe_wind_tick(&mut c, &mut v, Some(-1));
    assert_eq!(v.hp, 0, "shape A kills on a negative roll");

    // The physical-wrapper body takes the same shape.
    let mut c = ctx(0, 7);
    let mut caster = actor(100);
    let mut v = actor(30);
    bloody_horns_tick(&mut c, &mut caster, &mut v, Some(31));
    assert_eq!(v.hp, 0);
    assert_eq!(caster.staged_anim, 0, "and it clears the caster's +0x1DA");
}

/// The three whole-row sweeps kill, which is what separates them from the two
/// AoE **stagers** (`0x801F85A8` / `0x801F8D64`) and their `HP - 1` clamp.
#[test]
fn the_whole_row_sweeps_kill() {
    let mut c = ctx(CHAOS_BREATH_SWEEP_ARM, 7);
    let mut caster = actor(100);
    let mut seats = vec![actor(10); 7];
    let (_, hits) = chaos_breath_tick(&mut c, &mut caster, &mut seats, |_| 999, |_| (1, 1));
    assert_eq!(hits.len(), 7);
    assert!(seats.iter().all(|s| s.hp == 0), "the sweep wipes the row");
    assert!(hits.iter().all(|h| h.applied == 10));

    let mut c = ctx(DOOMSDAY_SWEEP_ARM, 7);
    let mut seats = vec![actor(10); 7];
    let (_, hits) = doomsday_tick(&mut c, &mut seats, true, |_| 999);
    assert_eq!(hits.len(), 7);
    assert!(seats.iter().all(|s| s.hp == 0));
    assert_eq!(
        (c.ctx_278, c.ctx_27a),
        (0, 0),
        "and it clears both scratch bytes"
    );
}

/// PROT 0938's `0x4E` body skips a Stone seat; its `0xB7` sibling does not -
/// the one sweep in the band with no `+0x16E & 4` guard.
#[test]
fn only_one_sweep_skips_a_stone_seat() {
    let mut stone = actor(50);
    stone.flags = FLAG_NON_TARGETABLE;

    let mut c = ctx(CHAOS_BREATH_SWEEP_ARM, 2);
    let mut caster = actor(100);
    let mut seats = vec![stone, actor(50)];
    let (_, hits) = chaos_breath_tick(&mut c, &mut caster, &mut seats, |_| 10, |_| (1, 1));
    assert_eq!(hits.len(), 1, "the Stone seat drew nothing");
    assert_eq!(seats[0].hp, 50);

    let mut c = ctx(MYSTIC_CIRCLE_SWEEP_ARM, 2);
    let mut seats = vec![stone, actor(50)];
    let (_, hits) = mystic_circle_tick(&mut c, &mut seats, true, |_| 10);
    assert_eq!(hits.len(), 2, "Mystic Circle hits it anyway");
    assert_eq!(seats[0].hp, 40);
    assert_eq!(seats[0].anim_rate, MYSTIC_CIRCLE_HIT_ANIM_RATE);
}

/// A dead seat is skipped by every sweep, and draws nothing - the guard runs
/// ahead of the wrapper call, so the RNG cursor does not move for it either.
#[test]
fn a_dead_seat_is_skipped_by_every_sweep() {
    let mut c = ctx(DOOMSDAY_SWEEP_ARM, 3);
    let mut seats = vec![actor(0), actor(10), actor(0)];
    let mut draws = 0;
    let (_, hits) = doomsday_tick(&mut c, &mut seats, true, |_| {
        draws += 1;
        5
    });
    assert_eq!((hits.len(), draws), (1, 1));
}

// ---------------------------------------------------------------------------
// The status / buff writers
// ---------------------------------------------------------------------------

/// Chaos Breath's two 1-in-8 rolls: the first sets Venom, and the second only
/// gets a chance when the first misses. Never both.
#[test]
fn chaos_breath_sets_exactly_the_bit_the_bytes_set() {
    // First roll hits (`& 7 == 0`) -> Venom, and the second is not consulted.
    let mut c = ctx(CHAOS_BREATH_SWEEP_ARM, 1);
    let mut caster = actor(100);
    let mut seats = vec![actor(50)];
    chaos_breath_tick(&mut c, &mut caster, &mut seats, |_| 1, |_| (8, 8));
    assert_eq!(seats[0].flags, FLAG_VENOM);

    // First misses, second hits -> Toxic only.
    let mut c = ctx(CHAOS_BREATH_SWEEP_ARM, 1);
    let mut seats = vec![actor(50)];
    chaos_breath_tick(&mut c, &mut caster, &mut seats, |_| 1, |_| (1, 16));
    assert_eq!(seats[0].flags, FLAG_TOXIC);

    // Both miss -> no status at all.
    let mut c = ctx(CHAOS_BREATH_SWEEP_ARM, 1);
    let mut seats = vec![actor(50)];
    chaos_breath_tick(&mut c, &mut caster, &mut seats, |_| 1, |_| (1, 3));
    assert_eq!(seats[0].flags, 0);
}

/// Kiss of Death's coin flip: odd marks and steals the turn, even applies one
/// point of damage and clears the status block.
#[test]
fn kiss_of_death_marks_on_a_miss_and_hits_for_one() {
    let mut c = ctx(4, 7);
    c.turn_cursor = 2;
    let mut v = actor(500);
    v.init_key = 40;
    v.action_category = ACTION_CATEGORY_ITEM;
    v.queued_action = 0x21;
    let (_, refund) = kiss_of_death_tick(&mut c, &mut v, Some(1));
    assert_eq!(v.flags, FLAG_KISS_OF_DEATH_MARK);
    assert_eq!(v.hp, 500, "the miss deals nothing");
    assert_eq!(refund, Some(0x21), "and refunds the queued item");
    assert_eq!((v.init_key, c.turn_cursor), (0, 3), "turn consumed");

    let mut c = ctx(4, 7);
    let mut v = actor(500);
    // Every bit but Stone - a Stone victim is skipped outright, tested below.
    let all_but_stone: u16 = !FLAG_NON_TARGETABLE;
    v.flags = all_but_stone;
    let (_, refund) = kiss_of_death_tick(&mut c, &mut v, Some(2));
    assert_eq!(v.hp, 499, "exactly one point");
    assert_eq!(v.hp_bar_delta, 1);
    assert_eq!(v.flags, all_but_stone & KISS_OF_DEATH_KEEP_MASK);
    assert_eq!(refund, None);
}

/// A Stone victim absorbs the hit arm entirely.
#[test]
fn kiss_of_death_skips_a_stone_victim() {
    let mut c = ctx(4, 7);
    let mut v = actor(500);
    v.flags = FLAG_NON_TARGETABLE;
    kiss_of_death_tick(&mut c, &mut v, Some(0));
    assert_eq!((v.hp, v.flags), (500, FLAG_NON_TARGETABLE));
}

/// Terror Scream writes no stat and no status bit - it consumes the turn.
#[test]
fn terror_scream_steals_the_turn_and_nothing_else() {
    let mut c = ctx(3, 7);
    c.turn_cursor = 1;
    let mut v = actor(300);
    v.init_key = 17;
    v.action_category = ACTION_CATEGORY_ITEM;
    v.queued_action = 0x40;
    let (step, refund) = terror_scream_tick(&mut c, &mut v);
    assert_eq!(step, CastTickStep::Busy);
    assert_eq!((v.hp, v.flags), (300, 0));
    assert_eq!((v.init_key, c.turn_cursor), (0, 2));
    assert_eq!(refund, Some(0x40));

    // No queued item -> no refund, but the turn still goes.
    let mut c = ctx(3, 7);
    let mut v = actor(300);
    v.init_key = 9;
    let (_, refund) = terror_scream_tick(&mut c, &mut v);
    assert_eq!(refund, None);
    assert_eq!(v.init_key, 0);
}

/// White Shield writes **both** halfwords of both defence pairs, from the
/// record base - so recasting is idempotent rather than compounding.
#[test]
fn white_shield_writes_both_halves_and_is_idempotent() {
    assert_eq!(white_shield_defence(100, 80), (150, 120));
    let mut c = ctx(3, 7);
    let mut caster = actor(500);
    let step = white_shield_tick(&mut c, &mut caster, (100, 80));
    assert_eq!(step, CastTickStep::Done);
    assert_eq!((caster.udf, caster.udf_base), (150, 150));
    assert_eq!((caster.ldf, caster.ldf_base), (120, 120));
    assert_eq!(c.ctx_0d, 0);

    // Recast from the same record: the same product, not 225.
    let mut c = ctx(3, 7);
    white_shield_tick(&mut c, &mut caster, (100, 80));
    assert_eq!(caster.udf, 150);

    // Arm 1 / arm 2 are the render-flag pair.
    let mut c = ctx(1, 7);
    white_shield_tick(&mut c, &mut caster, (100, 80));
    assert_eq!(caster.render_flag, WHITE_SHIELD_ARM1_RENDER_FLAG);
    let mut c = ctx(2, 7);
    white_shield_tick(&mut c, &mut caster, (100, 80));
    assert_eq!(caster.render_flag, 0);
}

/// Power Charge is `+25%` with a hard `999` ceiling on both halves.
#[test]
fn power_charge_raises_both_halves_and_caps_at_999() {
    assert_eq!(power_charge_step(100), 125);
    assert_eq!(power_charge_step(0), 0);
    assert_eq!(power_charge_step(800), 999, "1000 would exceed the cap");
    assert_eq!(power_charge_step(999), 999);

    let mut c = ctx(3, 7);
    let mut caster = actor(500);
    caster.atk = 200;
    caster.atk_base = 200;
    power_charge_tick(&mut c, &mut caster);
    assert_eq!((caster.atk, caster.atk_base), (250, 250));
    assert_eq!(caster.render_flag, 0);

    let mut c = ctx(4, 7);
    assert_eq!(power_charge_tick(&mut c, &mut caster), CastTickStep::Done);
}

/// Melt Spray's step is `x - (x + 9)/5` with a floor of one, and it lands on
/// ten halfwords - five stats, both halves each.
#[test]
fn melt_spray_debuffs_five_stats_on_both_halves() {
    assert_eq!(melt_spray_step(100), 79, "100 - (109 / 5)");
    // The floor fires only on an exact zero, because retail's `bnez` tests
    // the full 32-bit difference and the store is a 16-bit `sh`.
    assert_eq!(melt_spray_step(2), 1, "2 - 2 == 0, so the floor writes 1");
    assert_eq!(melt_spray_step(1), 0xFFFF, "1 - 2 == -1: it underflows");
    assert_eq!(melt_spray_step(0), 0xFFFF, "0 - 1 == -1, likewise");

    let mut c = ctx(MELT_SPRAY_DEBUFF_ARM, 7);
    let mut v = actor(500);
    for f in [
        &mut v.atk,
        &mut v.atk_base,
        &mut v.udf,
        &mut v.udf_base,
        &mut v.ldf,
        &mut v.ldf_base,
        &mut v.spd,
        &mut v.spd_base,
        &mut v.intel,
        &mut v.intel_base,
    ] {
        *f = 100;
    }
    melt_spray_tick(&mut c, &mut v, true);
    for got in [
        v.atk,
        v.atk_base,
        v.udf,
        v.udf_base,
        v.ldf,
        v.ldf_base,
        v.spd,
        v.spd_base,
        v.intel,
        v.intel_base,
    ] {
        assert_eq!(got, 79);
    }
    assert_eq!(v.restage, 1);
}

/// Void Accessories strips one of three slots, and only on the second coin
/// flip landing even with a non-empty slot.
#[test]
fn void_accessories_strips_one_slot_at_most() {
    // slot 1 picked, keep roll even -> voided.
    let mut c = ctx(3, 7);
    let mut v = actor(500);
    let (_, out) = void_accessories_tick(&mut c, &mut v, [0x10, 0x11, 0x12], Some((4, 2)));
    assert_eq!(
        out,
        Some(VoidAccessoriesOutcome {
            slot: 1,
            voided: Some(0x11)
        })
    );
    assert_eq!(v.staged_anim, v.knockdown_anim);

    // Same slot, odd keep roll -> refused.
    let mut c = ctx(3, 7);
    let mut v = actor(500);
    let (_, out) = void_accessories_tick(&mut c, &mut v, [0x10, 0x11, 0x12], Some((4, 3)));
    assert_eq!(out.unwrap().voided, None);

    // Empty slot -> nothing to void even on an even roll.
    let mut c = ctx(3, 7);
    let mut v = actor(500);
    let (_, out) = void_accessories_tick(&mut c, &mut v, [0, 0, 0], Some((0, 0)));
    assert_eq!(out.unwrap().voided, None);
}

// ---------------------------------------------------------------------------
// The battle overlay's effect-child hit arm
// ---------------------------------------------------------------------------

/// The safe applier's invariant: one clamped value reaches both `+0x10` and
/// `+0x14C`, so the readout can never be asked to travel further than HP
/// moved. That is what the action band's accumulating seed does not
/// guarantee.
#[test]
fn the_effect_child_clamp_moves_the_bar_exactly_as_far_as_hp() {
    let mut cursor = 0;
    for (hp, dmg) in [(100u16, 30i32), (100, 500), (5, 5), (0, 7)] {
        let mut v = EffectChildVictim {
            hp,
            knockdown_anim: 0x0B,
            reaction_alt: 2,
            reaction_alt2: 3,
            ..Default::default()
        };
        let hit = effect_child_hit(&mut v, dmg, &mut cursor);
        let moved = i32::from(hp) - i32::from(v.hp);
        assert_eq!(hit.applied, moved, "hp={hp} dmg={dmg}");
        assert_eq!(v.hp_bar_delta, moved);
    }
}

/// The readout cursor is eight-wide and wraps, matching `& 7`.
#[test]
fn the_readout_cursor_wraps_at_eight() {
    let mut cursor = 6;
    let mut v = EffectChildVictim {
        hp: 900,
        ..Default::default()
    };
    assert_eq!(effect_child_hit(&mut v, 1, &mut cursor).readout_slot, 6);
    assert_eq!(effect_child_hit(&mut v, 1, &mut cursor).readout_slot, 7);
    assert_eq!(effect_child_hit(&mut v, 1, &mut cursor).readout_slot, 0);
}

/// The reaction pick has three legs, and the `+0x1DC` writes are bit ORs -
/// not the increment the slot-B modules use.
#[test]
fn the_effect_child_reaction_pick_has_three_legs() {
    let mut cursor = 0;
    // Dead victim -> `+0x1F1` regardless of the gate.
    let mut v = EffectChildVictim {
        hp: 5,
        knockdown_anim: 0x0B,
        reaction_alt: 2,
        reaction_alt2: 3,
        ..Default::default()
    };
    effect_child_hit(&mut v, 5, &mut cursor);
    assert_eq!(v.staged_anim, 0x0B);
    assert_eq!(v.restage, RESTAGE_BIT_FACE, "no reaction bit on this leg");

    // Alive, gate set -> `+0x1F1`.
    let mut v = EffectChildVictim {
        hp: 100,
        reaction_gate: 1,
        knockdown_anim: 0x0B,
        reaction_alt: 2,
        ..Default::default()
    };
    effect_child_hit(&mut v, 1, &mut cursor);
    assert_eq!(v.staged_anim, 0x0B);

    // Alive, gate clear, `+0x1EF` non-zero -> `+0x1EF`, plus the reaction bit.
    let mut v = EffectChildVictim {
        hp: 100,
        reaction_alt: 2,
        reaction_alt2: 3,
        ..Default::default()
    };
    effect_child_hit(&mut v, 1, &mut cursor);
    assert_eq!(v.staged_anim, 2);
    assert_eq!(v.restage, RESTAGE_BIT_REACTION | RESTAGE_BIT_FACE);

    // ... and a zero `+0x1EF` falls on to `+0x1F0`.
    let mut v = EffectChildVictim {
        hp: 100,
        reaction_alt2: 3,
        ..Default::default()
    };
    effect_child_hit(&mut v, 1, &mut cursor);
    assert_eq!(v.staged_anim, 3);

    // A Stone victim skips the face leg's bit.
    let mut v = EffectChildVictim {
        hp: 100,
        flags: EFFECT_CHILD_STONE,
        reaction_alt: 2,
        ..Default::default()
    };
    effect_child_hit(&mut v, 1, &mut cursor);
    assert_eq!(v.restage & RESTAGE_BIT_FACE, 0);
}

/// The `(entry, body)` pairs this crate carries a tick kernel for. A body VA
/// alone is **not** a key: `0x801F6A20` is PROT 0951's Chaos Flare and also
/// PROT 0963's only arm, and `0x801F69D8` - the slot-B load base itself - is
/// a tick body in six different images.
const PORTED_BODIES: [(u32, u32); 21] = [
    (938, CHAOS_BREATH_TICK),
    (938, MYSTIC_CIRCLE_TICK),
    (942, POWER_UP_TICK),
    (945, WATER_COLUMN_TICK),
    (945, ALL_STATS_SURGE_TICK),
    (951, CHAOS_FLARE_TICK),
    (951, SCYTHE_WIND_TICK),
    (952, ASTRAL_SLASH_TICK),
    (952, BLOODY_HORNS_TICK),
    (955, WHITE_SHIELD_TICK),
    (955, KISS_OF_DEATH_TICK),
    (955, MELT_SPRAY_TICK),
    (955, TERROR_SCREAM_TICK),
    (955, POWER_CHARGE_TICK),
    (955, VOID_ACCESSORIES_TICK),
    (957, SUMMON_EFFECT_TICK_A),
    (957, SUMMON_EFFECT_TICK_B),
    (958, BLAZING_SLASH_TICK),
    (960, PLASMA_STRIKE_TICK),
    (964, ELEMENT_CHANGE_TICK),
    (965, DOOMSDAY_TICK),
];

/// The whole capture-class trampoline map, as the bytes carry it: twenty-one
/// of the thirty-two `0x801CF56C` arms are a trampoline, and between them
/// they name forty-eight `(id -> body)` arms.
#[test]
fn the_trampoline_map_is_the_whole_band() {
    assert_eq!(CAPTURE_TRAMPOLINES.len(), 21);
    let arms: usize = CAPTURE_TRAMPOLINES.iter().map(|t| t.arms.len()).sum();
    assert_eq!(arms, 48);
    for t in CAPTURE_TRAMPOLINES {
        for (id, body) in t.arms {
            assert_eq!(capture_tick_body(t.prot_entry, *id), Some(*body));
        }
    }
    // Every ported pair is an arm some trampoline really names.
    for (entry, body) in PORTED_BODIES {
        let t = capture_trampoline_for(entry).expect("ported entry has a trampoline");
        assert!(
            t.arms.iter().any(|(_, b)| *b == body),
            "PROT {entry} has no arm for {body:#010X}"
        );
    }
}

/// The defect the `(entry, body)` key fixes: PROT 0945 and PROT 0960 each
/// hold **two** choreographies, so a dispatcher keyed on the entry alone runs
/// one of them for both of the module's ids.
#[test]
fn a_two_spell_cell_resolves_a_different_body_per_id() {
    // PROT 0945 - `0x54` Water Column, `0xBA` its unported sibling.
    assert_eq!(capture_tick_body(945, 0x54), Some(WATER_COLUMN_TICK));
    let sibling = capture_tick_body(945, 0xBA).expect("0xBA is a named arm");
    assert_ne!(sibling, WATER_COLUMN_TICK);
    assert_eq!(sibling, ALL_STATS_SURGE_TICK);

    // PROT 0960 - `0x7B` Plasma Strike, `0xA6` Neo Star Slash.
    assert_eq!(capture_tick_body(960, 0x7B), Some(PLASMA_STRIKE_TICK));
    let neo = capture_tick_body(960, 0xA6).expect("0xA6 is a named arm");
    assert_ne!(neo, PLASMA_STRIKE_TICK);
    assert!(!PORTED_BODIES.contains(&(960, neo)));

    // ...and an id neither module names still ticks nothing.
    assert_eq!(capture_tick_body(945, 0x7B), None);
    assert_eq!(capture_tick_body(960, 0x54), None);
}

/// Two body VAs are shared across images, which is why nothing may key on the
/// VA alone. Both pairs are read off the owning images' own bytes.
#[test]
fn body_vas_collide_across_images() {
    // The slot-B load base is a tick body in six images at once.
    let at_base: Vec<u32> = CAPTURE_TRAMPOLINES
        .iter()
        .filter(|t| t.arms.iter().any(|(_, b)| *b == CAST_MODULE_LINK_BASE))
        .map(|t| t.prot_entry)
        .collect();
    assert_eq!(at_base, vec![956, 960, 961, 962, 964, 965]);
    // ...and only PROT 0965's copy of it is Doomsday.
    assert_eq!(DOOMSDAY_TICK, CAST_MODULE_LINK_BASE);
    assert!(!PORTED_BODIES.contains(&(960, CAST_MODULE_LINK_BASE)));

    // PROT 0963's single arm wears Chaos Flare's VA in a different image.
    assert_eq!(capture_tick_body(963, 0xB3), Some(CHAOS_FLARE_TICK));
    assert!(!PORTED_BODIES.contains(&(963, CHAOS_FLARE_TICK)));
}

/// PROT 0942's Power Up: four arms, and only arm 3 both writes the AGL base
/// and reports done.
#[test]
fn power_up_writes_only_the_agl_base_and_only_on_arm_three() {
    // `record[+0x0E] * 3 / 2`, the `sll`/`addu`/`sra 1` at `0x801F8068`.
    assert_eq!(power_up_agl(40), 60);
    assert_eq!(power_up_agl(41), 61);
    assert_eq!(power_up_agl(0), 0);

    let mut c = ctx(0, 4);
    let mut a = actor(100);
    a.agl = 7;
    a.agl_base = 7;
    // Arms 0..2 leave the gauge alone.
    for phase in 0..3u8 {
        c.phase = phase;
        assert_eq!(power_up_tick(&mut c, &mut a, 40), CastTickStep::Busy);
        assert_eq!(c.phase, phase + 1);
        assert_eq!((a.agl, a.agl_base), (7, 7));
    }
    assert_eq!(a.render_flag, 0, "arm 2 clears the charge flag again");

    // Arm 3 commits, and reports done rather than advancing.
    assert_eq!(c.phase, POWER_UP_COMMIT_ARM);
    assert_eq!(power_up_tick(&mut c, &mut a, 40), CastTickStep::Done);
    assert_eq!(a.agl_base, 60);
    assert_eq!(
        a.agl, 7,
        "the working gauge is untouched - no `+0x154` store"
    );
    assert_eq!(c.phase, POWER_UP_COMMIT_ARM, "the terminal arm holds");
    assert_eq!(c.ctx_0d, 0);
}

/// The charge flag arm 1 puts on the caster.
#[test]
fn power_up_arm_one_raises_the_charge_render_flag() {
    let mut c = ctx(1, 4);
    let mut a = actor(100);
    assert_eq!(power_up_tick(&mut c, &mut a, 40), CastTickStep::Busy);
    assert_eq!(a.render_flag, POWER_UP_CHARGE_RENDER_FLAG);
}

/// PROT 0945's `0xBA` body raises **all ten** stat halfwords by `x + (x>>2)`
/// and writes the AGL base off the record, on arm 2 only.
#[test]
fn all_stats_surge_raises_ten_halfwords_by_a_quarter() {
    let mut c = ctx(0, 4);
    let mut a = actor(100);
    a.atk = 100;
    a.atk_base = 100;
    a.udf = 80;
    a.udf_base = 80;
    a.ldf = 40;
    a.ldf_base = 40;
    a.spd = 20;
    a.spd_base = 20;
    a.intel = 4;
    a.intel_base = 4;
    a.agl_base = 7;

    // Arms 0 and 1 leave the block alone.
    for phase in 0..ALL_STATS_SURGE_ARM {
        c.phase = phase;
        assert_eq!(all_stats_surge_tick(&mut c, &mut a, 40), CastTickStep::Busy);
    }
    assert_eq!((a.atk, a.intel_base, a.agl_base), (100, 4, 7));

    assert_eq!(c.phase, ALL_STATS_SURGE_ARM);
    assert_eq!(all_stats_surge_tick(&mut c, &mut a, 40), CastTickStep::Busy);
    assert_eq!((a.atk, a.atk_base), (125, 125));
    assert_eq!((a.udf, a.udf_base), (100, 100));
    assert_eq!((a.ldf, a.ldf_base), (50, 50));
    assert_eq!((a.spd, a.spd_base), (25, 25));
    // `4 + (4 >> 2)` is 5 - the shift floors, it does not round.
    assert_eq!((a.intel, a.intel_base), (5, 5));
    // The same `record * 3 / 2` PROT 0942's Power Up writes.
    assert_eq!(a.agl_base, power_up_agl(40));

    // Arm 3 is the terminal one.
    assert_eq!(c.phase, 3);
    assert_eq!(all_stats_surge_tick(&mut c, &mut a, 40), CastTickStep::Done);
    assert_eq!(c.ctx_0d, 0);
    assert_eq!(c.phase, 3);
}

/// The surge is unsigned throughout (`lhu` + `srl`), so a maxed stat wraps.
#[test]
fn all_stats_surge_wraps_rather_than_saturating() {
    let mut c = ctx(ALL_STATS_SURGE_ARM, 4);
    let mut a = actor(100);
    a.atk = 0xFFFF;
    a.atk_base = 0xFFFF;
    all_stats_surge_tick(&mut c, &mut a, 0);
    assert_eq!(a.atk, 0xFFFFu16.wrapping_add(0xFFFF >> 2));
}

/// PROT 0964's Element Change never re-picks what the record already holds,
/// and it maps the accepted draw through the module's own three-byte table.
#[test]
fn element_change_rerolls_until_the_element_differs() {
    // A rigged RNG that keeps handing back the current element's index first.
    let draws = [0u32, 0, 0, 2];
    let mut it = draws.into_iter();
    let mut c = ctx(0, 4);
    let mut seats = vec![actor(100); 4];
    let (step, outcome) =
        element_change_tick(&mut c, &mut seats, 0, || it.next().unwrap_or_default());
    assert_eq!(step, CastTickStep::Busy);
    let out = outcome.expect("arm 0 commits");
    assert_eq!(out.roll, 2);
    assert_eq!(out.element, ELEMENT_CHANGE_ELEMENTS[2]);
    assert_eq!(out.group, 2 + ELEMENT_CHANGE_GROUP_BASE);
    assert_eq!(c.phase, 1);
}

/// A `last_roll` outside `0..3` matches nothing, so the first draw stands -
/// what a never-written `0x801C8FE4` does.
#[test]
fn element_change_accepts_the_first_draw_when_nothing_matches() {
    let mut it = [1u32].into_iter();
    let mut c = ctx(0, 4);
    let mut seats = vec![actor(100); 4];
    let (_, outcome) =
        element_change_tick(&mut c, &mut seats, 0xFF, || it.next().unwrap_or_default());
    let out = outcome.expect("arm 0 commits");
    assert_eq!(out.roll, 1);
    assert_eq!(out.element, ELEMENT_CHANGE_ELEMENTS[1]);
}

/// Arm 1 hides every seat `ctx[+0]` covers and no more; arm 2 is the only one
/// that reports done.
#[test]
fn element_change_hides_the_row_then_finishes() {
    let mut c = ctx(1, 3);
    let mut seats = vec![actor(100); 5];
    let (step, outcome) = element_change_tick(&mut c, &mut seats, 0xFF, || 0);
    assert_eq!(step, CastTickStep::Busy);
    assert!(outcome.is_none());
    for seat in seats.iter().take(3) {
        assert_eq!(seat.render_flag, ELEMENT_CHANGE_HIDE_RENDER_FLAG);
    }
    for seat in seats.iter().skip(3) {
        assert_eq!(seat.render_flag, 0, "seats past ctx[+0] are untouched");
    }
    assert_eq!(c.phase, 2);

    let (step, _) = element_change_tick(&mut c, &mut seats, 0xFF, || 0);
    assert_eq!(step, CastTickStep::Done);
    assert_eq!(c.ctx_0d, 0);
    assert_eq!(c.phase, 2, "the terminal arm holds");

    // Past the last arm the body falls out of its own dispatch.
    c.phase = 3;
    let (step, _) = element_change_tick(&mut c, &mut seats, 0xFF, || 0);
    assert_eq!(step, CastTickStep::Done);
}

/// The six wrapper-calling bodies' baked powers, keyed by body VA because
/// four of them share a PROT entry with a sibling that bakes a different
/// constant.
#[test]
fn body_damage_shapes_are_keyed_by_body_not_entry() {
    assert_eq!(
        body_damage_shape(CHAOS_BREATH_TICK).unwrap().powers,
        &[CHAOS_BREATH_POWER]
    );
    assert_eq!(
        body_damage_shape(MYSTIC_CIRCLE_TICK).unwrap().powers,
        &[MYSTIC_CIRCLE_POWER]
    );
    // Same owner, different constants - which is why the entry-keyed table
    // cannot answer for a tick.
    assert_eq!(
        body_damage_shape(CHAOS_BREATH_TICK).unwrap().prot_entry,
        body_damage_shape(MYSTIC_CIRCLE_TICK).unwrap().prot_entry
    );
    assert_ne!(CHAOS_BREATH_POWER, MYSTIC_CIRCLE_POWER);
    assert_eq!(
        body_damage_shape(SCYTHE_WIND_TICK).unwrap().powers,
        &[SCYTHE_WIND_POWER],
        "set in the jal delay slot at 0x801F7F8C"
    );
    // Not one of the six is a never-kill site.
    assert!(TRAMPOLINE_BODY_SHAPES.iter().all(|s| !s.never_kills));
    // The PROT 0955 cell calls no wrapper at all.
    for body in [
        WHITE_SHIELD_TICK,
        KISS_OF_DEATH_TICK,
        MELT_SPRAY_TICK,
        TERROR_SCREAM_TICK,
        POWER_CHARGE_TICK,
        VOID_ACCESSORIES_TICK,
        ASTRAL_SLASH_TICK,
    ] {
        assert!(body_damage_shape(body).is_none(), "{body:#010X}");
    }
}
