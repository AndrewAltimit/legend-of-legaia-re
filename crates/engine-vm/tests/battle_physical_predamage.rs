//! Hand-checked cases for the melee pre-damage kernel
//! ([`legaia_engine_vm::battle_formulas::physical_predamage`], the port of
//! `FUN_801EC3E4`'s roll pair).
//!
//! Every expected value below is computed by hand from the disassembly's
//! stages with a fixed `rand()` so the arithmetic - not just the shape - is
//! pinned. Disc-free.

use legaia_engine_vm::battle_formulas::{
    COMMAND_POWER_SCALARS, PhysicalHit, command_power_scalar, physical_defense_is_udf,
    physical_predamage,
};

/// A `rand()` that always answers `0` - every `% n` term drops out, so the
/// remaining arithmetic is the deterministic skeleton of each stage.
fn zero_rand() -> impl FnMut() -> u16 {
    || 0
}

/// Counts its draws so the RNG call-count can be asserted against retail's.
fn counting_rand(count: &mut u32) -> impl FnMut() -> u16 + '_ {
    move || {
        *count += 1;
        0
    }
}

fn hit(atk: u16, def: u16) -> PhysicalHit {
    PhysicalHit {
        attacker_atk: atk,
        defender_def: def,
        ..Default::default()
    }
}

#[test]
fn a_clearing_swing_is_the_attack_roll_minus_the_guard_roll() {
    // atk 100, arm-command scalar 12, def 10, rand 0:
    //   raw   = ((100 + 0) * 12) >> 4 = 75
    //   guard = 10 + 0                = 10
    //   clears: guard + ((raw*12)>>6) = 10 + 14 = 24 < 75
    //   damage = 75 - 10 = 65
    assert_eq!(physical_predamage(&hit(100, 10), &mut zero_rand()), 65);
}

#[test]
fn the_attacker_hp_term_is_hp_over_256() {
    let mut h = hit(100, 10);
    h.attacker_hp = 1024; // 1024 >> 8 = 4
    assert_eq!(physical_predamage(&h, &mut zero_rand()), 69);
}

#[test]
fn an_underdog_swing_is_rewritten_rather_than_floored_at_one() {
    // atk 10 vs def 400 - the shape a flat `attack - defense` model chipped
    // for exactly 1, which is what made real fights unwinnable.
    //   raw   = (10*12) >> 4 = 7
    //   guard = 400
    //   7 does not clear -> rewrite: guard + ((((7*3)>>2) + 0) * 12 >> 6) = 400
    //   plain swing within guard+3 -> chip floor: guard + (0%3 + 3) = 403
    //   damage = 3
    assert_eq!(physical_predamage(&hit(10, 400), &mut zero_rand()), 3);

    // The rewrite scales with the attacker: a bigger underdog roll keeps more
    // of its 3/4 share, so damage grows well past the chip floor.
    //   atk 400: raw = (400*12)>>4 = 300; guard = 4000; does not clear
    //   rewrite: 4000 + ((((300*3)>>2)=225) * 12 >> 6) = 4000 + 42 = 4042
    //   damage = 42
    assert_eq!(physical_predamage(&hit(400, 4000), &mut zero_rand()), 42);
}

#[test]
fn damage_is_monotone_in_attack_and_in_defense() {
    let mut prev = 0;
    for atk in [10u16, 40, 100, 400, 1000] {
        let d = physical_predamage(&hit(atk, 200), &mut zero_rand());
        assert!(d >= prev, "atk {atk} gave {d}, below the previous {prev}");
        prev = d;
    }
    let mut prev = u16::MAX;
    for def in [0u16, 50, 200, 800, 4000] {
        let d = physical_predamage(&hit(500, def), &mut zero_rand());
        assert!(d <= prev, "def {def} gave {d}, above the previous {prev}");
        prev = d;
    }
}

#[test]
fn the_art_arms_scale_the_roll_by_thirteen_tenths() {
    // Same numbers as the clearing-swing case, staged as an art (id > 0x10):
    //   raw = 75 -> 75*13/10 = 97 -> affinity x2 (100%) = 97
    //   damage = 97 - 10 = 87
    let mut h = hit(100, 10);
    h.staged_anim = 0x1B;
    assert_eq!(physical_predamage(&h, &mut zero_rand()), 87);
    // Ability bit 0x1000 picks 14/10 instead: 75*14/10 = 105 -> 95.
    h.art_power_bit = true;
    assert_eq!(physical_predamage(&h, &mut zero_rand()), 95);
}

#[test]
fn the_spirit_stance_triples_the_guard_roll() {
    let mut h = hit(100, 10);
    h.defender_guarding = true;
    // guard = 10 * 3 = 30; raw = 75 still clears (30 + 14 = 44 < 75).
    assert_eq!(physical_predamage(&h, &mut zero_rand()), 45);
}

#[test]
fn element_affinity_scales_the_attack_roll() {
    let mut h = hit(100, 10);
    h.affinity_pct = 50; // raw 75 -> 37; guard 10; clears -> 27
    assert_eq!(physical_predamage(&h, &mut zero_rand()), 27);
}

#[test]
fn the_status_bits_scale_each_side() {
    let mut h = hit(100, 10);
    h.attacker_status = 0x1; // raw 75 -> 67
    assert_eq!(physical_predamage(&h, &mut zero_rand()), 57);
    h.attacker_status = 0x2; // raw 75 -> 52
    assert_eq!(physical_predamage(&h, &mut zero_rand()), 42);
}

#[test]
fn damage_is_capped_at_9999() {
    // raw = (60000*12)>>4 = 45000 against a zero guard.
    assert_eq!(physical_predamage(&hit(60_000, 0), &mut zero_rand()), 9999);
}

#[test]
fn a_clearing_swing_draws_exactly_two_rands() {
    let mut n = 0;
    physical_predamage(&hit(100, 10), &mut counting_rand(&mut n));
    assert_eq!(n, 2, "attack roll + guard roll");

    // The underdog arm adds the rewrite draw, and the chip floor one more.
    let mut n = 0;
    physical_predamage(&hit(10, 400), &mut counting_rand(&mut n));
    assert_eq!(n, 4, "attack + guard + rewrite + chip floor");
}

#[test]
fn the_command_scalar_and_defence_half_key_on_the_staged_id() {
    // 0x0C..=0x10 index the five-entry scalar table in order.
    for (i, id) in (0x0Cu8..=0x10).enumerate() {
        assert_eq!(command_power_scalar(id), COMMAND_POWER_SCALARS[i]);
    }
    // `(id - 0x0C) % 10 < 5` picks UDF: the first five commands, then LDF.
    for id in 0x0Cu8..=0x10 {
        assert!(physical_defense_is_udf(id), "0x{id:02X} reads UDF");
    }
    for id in 0x11u8..=0x15 {
        assert!(!physical_defense_is_udf(id), "0x{id:02X} reads LDF");
    }
    assert!(physical_defense_is_udf(0x16));
}
