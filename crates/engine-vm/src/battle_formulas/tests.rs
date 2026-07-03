//! Unit tests for the battle-formula kernels. Split out of `battle_formulas.rs`.

use super::*;

/// Pin the spirit-damage formula on a few hand-checked points. The
/// `cap = 0x120 = 288` cap is the larger of the two ceilings observed
/// in the state machine.
#[test]
fn spirit_damage_matches_doc() {
    assert_eq!(spirit_damage(0, 288), 8); // floor at +8
    assert_eq!(spirit_damage(100, 288), 148); // 100*7/5 + 8 = 148
    assert_eq!(spirit_damage(200, 288), 288); // 200*7/5+8 = 288 = cap
    assert_eq!(spirit_damage(500, 288), 288); // overflow → cap
    assert_eq!(spirit_damage(50, 100), 78); // smaller cap (100), under
    assert_eq!(spirit_damage(150, 100), 100); // smaller cap, clipped
}

#[test]
fn mp_cost_modifier_resolves() {
    assert_eq!(
        MpCostModifier::from_ability_flags(0x00),
        MpCostModifier::Full
    );
    assert_eq!(
        MpCostModifier::from_ability_flags(0x10),
        MpCostModifier::Quarter
    );
    assert_eq!(
        MpCostModifier::from_ability_flags(0x20),
        MpCostModifier::Half
    );
    // Half wins when both bits set (matches the `if/else if` chain).
    assert_eq!(
        MpCostModifier::from_ability_flags(0x30),
        MpCostModifier::Half
    );
}

#[test]
fn mp_cost_arithmetic() {
    assert_eq!(mp_cost_after_ability_bits(40, MpCostModifier::Full), 40);
    // Half = cost - cost>>1; Quarter = cost - cost>>2 (shave 25%, not "/4").
    assert_eq!(mp_cost_after_ability_bits(40, MpCostModifier::Half), 20);
    assert_eq!(mp_cost_after_ability_bits(40, MpCostModifier::Quarter), 30);
    // Odd cost: Half rounds UP (7 - 7>>1 = 7 - 3 = 4); Quarter 7 - 1 = 6.
    assert_eq!(mp_cost_after_ability_bits(7, MpCostModifier::Half), 4);
    assert_eq!(mp_cost_after_ability_bits(7, MpCostModifier::Quarter), 6);
}

#[test]
fn psyq_rand_is_deterministic_from_seed() {
    let mut a = 0x12345678;
    let mut b = 0x12345678;
    // Ten draws from identical seeds produce identical values.
    for _ in 0..10 {
        assert_eq!(psyq_rand_step(&mut a), psyq_rand_step(&mut b));
    }
    // ...but two draws are not equal in general.
    let mut s = 0x12345678;
    let r1 = psyq_rand_step(&mut s);
    let r2 = psyq_rand_step(&mut s);
    assert_ne!(r1, r2);
}

#[test]
fn psyq_rand_in_range() {
    let mut seed = 1;
    for _ in 0..1000 {
        let r = psyq_rand_step(&mut seed);
        assert!(r <= 0x7FFF);
    }
}

#[test]
fn accuracy_roll_zero_stats_auto_hits() {
    let mut s = 0;
    assert!(accuracy_roll(0, 0, &mut s));
}

#[test]
fn accuracy_roll_high_caster_hits_more() {
    // 100 vs 1: the roll is `rand % 101`; we need `target < roll`,
    // i.e. `1 < roll`, which is true except when `roll = 0` or `1`.
    // Two failures in 101 outcomes - over many seeds we should land
    // close to >=98% hit rate.
    let mut hits = 0;
    let mut s = 1;
    for _ in 0..1000 {
        if accuracy_roll(100, 1, &mut s) {
            hits += 1;
        }
    }
    assert!(hits > 950, "expected >95% hit rate, got {}", hits / 10);
}

#[test]
fn buff_ramp_increments_by_20pct_then_clamps() {
    assert_eq!(buff_ramp(0), 0);
    assert_eq!(buff_ramp(100), 120);
    assert_eq!(buff_ramp(50_000), 60_000);
    // Just under the clamp threshold.
    assert_eq!(buff_ramp(54_613), 65_535);
    // Over → clamp at 0xFFFF.
    assert_eq!(buff_ramp(60_000), 0xFFFF);
    assert_eq!(buff_ramp(0xFFFF), 0xFFFF);
}

#[test]
fn art_strike_damage_basic_arithmetic() {
    // attack=64, def=10, mult=16, div=16 → 64 - 10 = 54.
    assert_eq!(art_strike_damage(64, 10, 16, 16, 1), 54);
    // attack=64, def=10, mult=12 → (64 * 12) / 16 = 48; 48 - 10 = 38.
    assert_eq!(art_strike_damage(64, 10, 12, 16, 1), 38);
    // attack=64, def=10, mult=28 → (64 * 28)/16 = 112; 112-10 = 102.
    assert_eq!(art_strike_damage(64, 10, 28, 16, 1), 102);
}

#[test]
fn art_strike_damage_floor_when_def_exceeds_attack() {
    // High defense should clamp to floor, not underflow.
    assert_eq!(art_strike_damage(10, 100, 16, 16, 1), 1);
    // Custom floor.
    assert_eq!(art_strike_damage(10, 100, 16, 16, 5), 5);
}

#[test]
fn art_strike_damage_zero_divisor_returns_floor() {
    assert_eq!(art_strike_damage(64, 10, 16, 0, 1), 1);
}

#[test]
fn art_strike_damage_saturates_at_u16_max() {
    // attack=0xFFFF, mult=28, div=1 -> raw overflows u16 -> clamps.
    assert_eq!(art_strike_damage(0xFFFF, 0, 28, 1, 1), 0xFFFF);
}

#[test]
fn art_strike_damage_default_uses_div_16_floor_1() {
    assert_eq!(
        art_strike_damage_default(64, 10, 16),
        art_strike_damage(64, 10, 16, 16, 1)
    );
}

#[test]
fn damage_cap_clamps_to_party_slots() {
    let caps = [200, 250, 300, 999, 999, 999];
    assert_eq!(damage_cap_for_party_slot(&caps, 0), 200);
    assert_eq!(damage_cap_for_party_slot(&caps, 2), 300);
    // Out-of-range falls back to the last entry.
    assert_eq!(damage_cap_for_party_slot(&caps, 10), 999);
}

#[test]
fn summon_attacker_roll_matches_disasm() {
    // rand % (agl+1) + hp + caster_agl*2.
    // 5 % 21 = 5; + 200 + 16*2(=32) = 237.
    assert_eq!(summon_attacker_roll(200, 20, 16, 5), 237);
    // 25 % 21 = 4; + 200 + 32 = 236.
    assert_eq!(summon_attacker_roll(200, 20, 16, 25), 236);
    // agl = 0 -> modulus 1, rand contributes nothing (no div-by-zero).
    assert_eq!(summon_attacker_roll(50, 0, 0, 12345), 50);
}

#[test]
fn summon_defender_roll_matches_disasm() {
    let d = SummonRollActor {
        hp: 1000,
        agl: 18,
        stat_a: 64,
        stat_b: 48,
        ..Default::default()
    };
    // modulus = (18>>1)+1 = 10; 7 % 10 = 7.
    // (1000>>8)=3 + (64>>4)=4 + (48>>4)=3 + 18*2=36 = 53.
    assert_eq!(summon_defender_roll(&d, 7), 53);
}

#[test]
fn element_affinity_scales_by_percent() {
    assert_eq!(apply_element_affinity(100, 100), 100); // neutral
    assert_eq!(apply_element_affinity(100, 200), 200); // weakness (double)
    assert_eq!(apply_element_affinity(100, 50), 50); // resist
    assert_eq!(apply_element_affinity(100, 0), 0); // immune
}

#[test]
fn status_weaken_applies_bits_in_order() {
    assert_eq!(apply_status_weaken(100, 0), 100); // no bits
    assert_eq!(apply_status_weaken(100, 0x1), 90); // 9/10
    assert_eq!(apply_status_weaken(100, 0x2), 70); // 7/10
    // Both: 9/10 first (90), then 7/10 (63).
    assert_eq!(apply_status_weaken(100, 0x3), 63);
}

#[test]
fn magic_power_scales_roll() {
    assert_eq!(apply_magic_power(80, 1), 80); // power 1 = no change
    assert_eq!(apply_magic_power(80, 0), 80); // power 0 guarded to no change
    // power 9: 80 + (80*8 >> 3) = 80 + 80 = 160.
    assert_eq!(apply_magic_power(80, 9), 160);
}

#[test]
fn heal_summon_amount_matches_disasm() {
    // (power<<5) + 0xE0.
    assert_eq!(heal_summon_amount(0), 0xE0); // 224 floor
    assert_eq!(heal_summon_amount(10), 544); // 320 + 224
    assert_eq!(heal_summon_amount(255), 8384); // 8160 + 224
}

#[test]
fn summon_predamage_takes_bonus_when_attacker_is_weak() {
    let i = SummonPredamage {
        summon: SummonRollActor {
            hp: 200,
            agl: 20,
            ..Default::default()
        },
        caster_agl: 16,
        target: SummonRollActor {
            hp: 1000,
            agl: 18,
            stat_a: 64,
            stat_b: 48,
            ..Default::default()
        },
        element_affinity_pct: 100,
        magic_power_byte: 1,
        rng: [5, 7, 5],
    };
    // attacker initial 237, neutral affinity/status/power -> 237.
    // defender 53. 53 + 200 = 253 > 237 -> bonus re-roll:
    // 53 + (5 % 11 = 5) + 200 = 258.
    assert_eq!(summon_predamage(&i), (258, 53));
}

#[test]
fn summon_predamage_skips_bonus_on_elemental_weakness() {
    let i = SummonPredamage {
        summon: SummonRollActor {
            hp: 200,
            agl: 20,
            ..Default::default()
        },
        caster_agl: 16,
        target: SummonRollActor {
            hp: 1000,
            agl: 18,
            stat_a: 64,
            stat_b: 48,
            ..Default::default()
        },
        element_affinity_pct: 200, // double damage -> attacker dominates
        magic_power_byte: 1,
        rng: [5, 7, 5],
    };
    // attacker 237 * 200/100 = 474. defender 53. 53 + 200 = 253 <= 474 -> no bonus.
    assert_eq!(summon_predamage(&i), (474, 53));
}

#[test]
fn summon_predamage_doubles_defender_on_guard() {
    let i = SummonPredamage {
        summon: SummonRollActor {
            hp: 200,
            agl: 20,
            ..Default::default()
        },
        caster_agl: 16,
        target: SummonRollActor {
            hp: 1000,
            agl: 18,
            stat_a: 64,
            stat_b: 48,
            guard: 4, // doubles defender roll
            ..Default::default()
        },
        element_affinity_pct: 100,
        magic_power_byte: 1,
        rng: [5, 7, 5],
    };
    // defender 53 * 2 = 106. 106 + 200 = 306 > 237 -> bonus:
    // 106 + (5 % 11 = 5) + 200 = 311.
    assert_eq!(summon_predamage(&i), (311, 106));
}

#[test]
fn summon_predamage_lazy_draws_bonus_only_when_arm_fires() {
    use std::cell::Cell;

    // (a) reproduce the eager result exactly and (b) invoke the bonus
    // closure exactly when the bonus arm fires - zero times on the
    // no-bonus path (shared RNG cursor advances by two, not three) and
    // once on the bonus path, mirroring FUN_801dd0ac's in-arm rand().
    let strong_summon = SummonRollActor {
        hp: 500,
        agl: 40,
        ..Default::default()
    };
    let weak_target = SummonRollActor {
        hp: 100,
        agl: 4,
        ..Default::default()
    };
    // attacker = 0%41 + 500 + 32 = 532; defender = 0%3 + 0 + 8 = 8.
    // 8 + 500 = 508 <= 532 -> no bonus.
    let calls = Cell::new(0u32);
    let out = summon_predamage_lazy(&strong_summon, 16, &weak_target, 100, 1, [0, 0], || {
        calls.set(calls.get() + 1);
        0
    });
    assert_eq!(calls.get(), 0, "no-bonus path must not draw");
    assert_eq!(
        out,
        summon_predamage(&SummonPredamage {
            summon: strong_summon,
            caster_agl: 16,
            target: weak_target,
            element_affinity_pct: 100,
            magic_power_byte: 1,
            rng: [0, 0, 0],
        }),
        "matches the eager result"
    );

    // Bonus path: the guard-doubled defender overwhelms the attacker
    // (the summon_predamage_doubles_defender_on_guard setup).
    let summon = SummonRollActor {
        hp: 200,
        agl: 20,
        ..Default::default()
    };
    let target = SummonRollActor {
        hp: 1000,
        agl: 18,
        stat_a: 64,
        stat_b: 48,
        guard: 4,
        ..Default::default()
    };
    let calls = Cell::new(0u32);
    let out = summon_predamage_lazy(&summon, 16, &target, 100, 1, [5, 7], || {
        calls.set(calls.get() + 1);
        5
    });
    assert_eq!(calls.get(), 1, "bonus path draws exactly once");
    assert_eq!(out, (311, 106), "matches the eager bonus result");
}

#[test]
fn damage_finish_lazy_draws_floor_rand_only_when_zeroed() {
    use std::cell::Cell;

    // Non-zero damage: the floor closure must never run (retail draws no
    // RNG in FUN_801ddb30 unless mitigation zeroed the hit).
    let calls = Cell::new(0u32);
    let i = DamageFinish {
        predamage: 100,
        attacker_slot: 3,
        defender_slot: 4,
        attacker_element: 7,
        defender_resist: DefenderResist::default(),
        defender_guarding: false,
        enemy_defender_halve: false,
        bypass_party_resist: false,
        summon_power_pct: 100,
        floor_rand: 0,
    };
    let out = damage_finish_lazy(&i, || {
        calls.set(calls.get() + 1);
        4
    });
    assert_eq!(out, 100);
    assert_eq!(calls.get(), 0, "non-zero damage must not draw");

    // Zeroed damage: exactly one draw, rand%9 + 8.
    let calls = Cell::new(0u32);
    let z = DamageFinish { predamage: 0, ..i };
    let out = damage_finish_lazy(&z, || {
        calls.set(calls.get() + 1);
        13
    });
    assert_eq!(out, 13 % 9 + 8);
    assert_eq!(calls.get(), 1, "zeroed damage draws exactly once");
}

#[test]
fn arts_attacker_roll_matches_disasm() {
    // power 20, attacker hp 200 / agl 20, rng [0, 0]:
    //   rand0 % ((20>>2)+1 = 6) = 0
    // + rand1 % ((20>>1)+1 = 11) = 0
    // + (200 >> 8 = 0) + power 20 + agl*2 (40) = 60.
    assert_eq!(arts_attacker_roll(20, 200, 20, [0, 0]), 60);
    // rng [5, 7]: 5%6 (5) + 7%11 (7) + 0 + 20 + 40 = 72.
    assert_eq!(arts_attacker_roll(20, 200, 20, [5, 7]), 72);
    // power 0 -> modulus_power (0>>2)+1 = 1, rand%1 = 0; hp 0xFFFF>>8 = 0xFF.
    assert_eq!(arts_attacker_roll(0, 0xFFFF, 0, [123, 0]), 0xFF);
}

#[test]
fn arts_bonus_roll_matches_disasm() {
    // defender 30, power 10, agl 0, rng [0, 0]:
    //   30 + (10>>1 = 5) + 0%((10>>3)+1=2) + (0>>1 = 0) + 0%((0>>3)+1=1) = 35.
    assert_eq!(arts_bonus_roll(30, 10, 0, [0, 0]), 35);
    // rng [1, 0]: 30 + 5 + 1%2 (1) + 0 + 0 = 36.
    assert_eq!(arts_bonus_roll(30, 10, 0, [1, 0]), 36);
}

#[test]
fn arts_physical_predamage_skips_bonus_when_attacker_dominates() {
    let i = ArtsPredamage {
        power: 20,
        attacker: SummonRollActor {
            hp: 200,
            agl: 20,
            ..Default::default()
        },
        target: SummonRollActor {
            hp: 100,
            agl: 16,
            stat_a: 32,
            stat_b: 48,
            ..Default::default()
        },
        element_affinity_pct: 100,
        rng: [0, 0, 0, 0, 0],
    };
    // attacker 60 (see arts_attacker_roll test), defender 0%9 + 0 + 2 + 3 + 32 = 37.
    // threshold = 37 + (20>>1=10) + (20>>1=10) = 57; attacker 60 >= 57 -> no bonus.
    assert_eq!(arts_physical_predamage(&i), (60, 37));
}

#[test]
fn arts_physical_predamage_takes_bonus_when_attacker_is_weak() {
    let i = ArtsPredamage {
        power: 10,
        attacker: SummonRollActor {
            hp: 0,
            agl: 0,
            ..Default::default()
        },
        target: SummonRollActor {
            hp: 0,
            agl: 0,
            stat_a: 0xFF,
            stat_b: 0xFF,
            ..Default::default()
        },
        element_affinity_pct: 100,
        rng: [0, 0, 0, 0, 0],
    };
    // attacker = 0%3 + 0%1 + 0 + 10 + 0 = 10. defender = 0 + 0 + 15 + 15 + 0 = 30.
    // threshold = 30 + (10>>1=5) + (0>>1=0) = 35; attacker 10 < 35 -> bonus:
    //   30 + 5 + 0%2 + 0 + 0%1 = 35.
    assert_eq!(arts_physical_predamage(&i), (35, 30));
}

#[test]
fn arts_physical_predamage_lazy_draws_bonus_only_when_arm_fires() {
    use std::cell::Cell;

    // Reuses the eager tests' two setups. The lazy variant must (a) reproduce
    // the eager result exactly and (b) invoke the bonus closure exactly when
    // the bonus arm fires - zero times on the no-bonus path (so a shared RNG
    // cursor advances by three, not five) and once on the bonus path.
    let dominant_attacker = SummonRollActor {
        hp: 200,
        agl: 20,
        ..Default::default()
    };
    let dominant_target = SummonRollActor {
        hp: 100,
        agl: 16,
        stat_a: 32,
        stat_b: 48,
        ..Default::default()
    };
    let weak_attacker = SummonRollActor::default();
    let weak_target = SummonRollActor {
        stat_a: 0xFF,
        stat_b: 0xFF,
        ..Default::default()
    };

    // No-bonus path: attacker dominates, closure never runs.
    let calls = Cell::new(0u32);
    let out = arts_physical_predamage_lazy(
        20,
        &dominant_attacker,
        &dominant_target,
        100,
        [0; 3],
        || {
            calls.set(calls.get() + 1);
            [0, 0]
        },
    );
    assert_eq!(out, (60, 37), "matches the eager no-bonus result");
    assert_eq!(calls.get(), 0, "bonus pair not drawn on the no-bonus path");

    // Bonus path: weak attacker, closure runs exactly once.
    let calls = Cell::new(0u32);
    let out = arts_physical_predamage_lazy(10, &weak_attacker, &weak_target, 100, [0; 3], || {
        calls.set(calls.get() + 1);
        [0, 0]
    });
    assert_eq!(out, (35, 30), "matches the eager bonus result");
    assert_eq!(
        calls.get(),
        1,
        "bonus pair drawn exactly once when the arm fires"
    );
}

#[test]
fn arts_physical_predamage_scales_by_affinity() {
    let i = ArtsPredamage {
        power: 20,
        attacker: SummonRollActor {
            hp: 200,
            agl: 20,
            ..Default::default()
        },
        target: SummonRollActor {
            hp: 100,
            agl: 16,
            stat_a: 32,
            stat_b: 48,
            ..Default::default()
        },
        element_affinity_pct: 200, // weakness -> double the attacker roll
        rng: [0, 0, 0, 0, 0],
    };
    // attacker 60 * 200/100 = 120. defender 37. threshold 57; 120 >= 57 -> no bonus.
    assert_eq!(arts_physical_predamage(&i), (120, 37));
}

#[test]
fn victory_gold_matches_runtime_gimard() {
    // Lone Gimard: record gold 60 -> 60>>1 = 30 accumulated -> 30 - (30>>1) = 15.
    let acc = victory_gold_per_monster(60);
    assert_eq!(acc, 30);
    assert_eq!(victory_gold_finalize(acc, false), 15);
    // +25% "extra gold" bonus: 30 + (30>>2 = 7) = 37 -> 37 - (37>>1 = 18) = 19.
    assert_eq!(victory_gold_finalize(acc, true), 19);
    // Multi-enemy accumulation rounds per monster: gold 60 + 61 -> 30 + 30 = 60.
    let two = victory_gold_per_monster(60) + victory_gold_per_monster(61);
    assert_eq!(two, 60);
    assert_eq!(victory_gold_finalize(two, false), 30);
}

#[test]
fn victory_exp_per_member_scales_and_ceils() {
    // 100 exp, 3 alive: 100 - (100>>2 = 25) = 75; ceil(75/3) = 25.
    assert_eq!(victory_exp_per_member(100, 3), 25);
    // 100 exp, 1 alive: 75 -> ceil(75/1) = 75.
    assert_eq!(victory_exp_per_member(100, 1), 75);
    // Ceiling: 10 exp, 3 alive: 10 - 2 = 8; ceil(8/3) = 3 (floor would give 2).
    assert_eq!(victory_exp_per_member(10, 3), 3);
    // alive 0 -> 0 (guard).
    assert_eq!(victory_exp_per_member(100, 0), 0);
}

// -- FUN_801ddb30 finisher -------------------------------------------------

fn finish(predamage: u32) -> DamageFinish {
    DamageFinish {
        predamage,
        attacker_slot: 3, // enemy attacker
        defender_slot: 0, // party defender
        attacker_element: 0,
        defender_resist: DefenderResist::default(),
        defender_guarding: false,
        enemy_defender_halve: false,
        bypass_party_resist: false,
        summon_power_pct: 0,
        floor_rand: 0,
    }
}

#[test]
fn defender_resist_passive_index_layout() {
    // The elemental-guard passives are contiguous at ability-bit index
    // `0x1D + element` (Earth..Dark); words 0/1 of the bitfield split
    // them across the +0xF4/+0xF8 boundary exactly as FUN_801ddb30's
    // ladder reads them. All Guard (0x24) is the absorb gate.
    for element in 0..=6u8 {
        let index = 0x1D + element as u32;
        let (w0, w1) = if index < 32 {
            (1u32 << index, 0)
        } else {
            (0, 1u32 << (index - 32))
        };
        let r = DefenderResist::from_ability_words(w0, w1);
        for probe in 0..=6u8 {
            assert_eq!(
                r.resists(probe),
                probe == element,
                "guard passive 0x{index:02X} must resist element {element} only"
            );
        }
    }
    let all_guard = DefenderResist::from_ability_words(0, 1 << (0x24 - 32));
    assert_eq!(all_guard.hi & 0x10, 0x10, "All Guard = the absorb gate bit");
}

#[test]
fn damage_finish_passthrough_when_no_mitigation() {
    // No resistance, no guard, no cap: over passes through unchanged.
    assert_eq!(damage_finish(&finish(500)), 500);
}

#[test]
fn damage_finish_halves_on_matching_element_resist() {
    let mut i = finish(500);
    // Defender resists element 0 / Earth Guard (word-0 bit 0x20000000);
    // attacker is element 0.
    i.defender_resist = DefenderResist {
        lo: 0x2000_0000,
        hi: 0,
    };
    i.attacker_element = 0;
    assert_eq!(damage_finish(&i), 250);
    // A non-matching attacker element (1) is not resisted -> full damage.
    i.attacker_element = 1;
    assert_eq!(damage_finish(&i), 500);
}

#[test]
fn damage_finish_low_word_elements_and_absorb_gate() {
    let mut i = finish(800);
    // Element 3 / Wind Guard lives in the +0xF8 word bit 0x1.
    i.defender_resist = DefenderResist { lo: 0, hi: 0x1 };
    i.attacker_element = 3;
    assert_eq!(damage_finish(&i), 400);
    // All-Guard gate (hi & 0x10) set + elemental attacker -> 3/4 scale,
    // ignoring the per-element ladder: 800 * 3 >> 2 = 600.
    i.defender_resist = DefenderResist {
        lo: 0,
        hi: 0x1 | 0x10,
    };
    assert_eq!(damage_finish(&i), 600);
    // ...but a non-elemental attacker (7) bypasses the gate -> ladder runs,
    // and element 7 resists nothing -> full damage.
    i.attacker_element = 7;
    assert_eq!(damage_finish(&i), 800);
}

#[test]
fn damage_finish_guard_and_resist_stack() {
    let mut i = finish(800);
    i.defender_resist = DefenderResist {
        lo: 0x2000_0000,
        hi: 0,
    }; // element 0 resist -> /2
    i.defender_guarding = true; // -> /2 again
    // 800 -> 400 -> 200.
    assert_eq!(damage_finish(&i), 200);
}

#[test]
fn damage_finish_enemy_defender_halve() {
    let mut i = finish(500);
    i.defender_slot = 3; // enemy defender -> party-resist block skipped
    i.attacker_slot = 0; // party attacker
    i.defender_resist = DefenderResist {
        lo: 0xFFFF_FFFF,
        hi: 0xFFFF_FFFF,
    }; // ignored for enemy defender
    assert_eq!(damage_finish(&i), 500, "no halve flag -> full");
    i.enemy_defender_halve = true;
    assert_eq!(damage_finish(&i), 250);
}

#[test]
fn damage_finish_bypass_party_resist() {
    let mut i = finish(500);
    i.defender_resist = DefenderResist {
        lo: 0x2000_0000,
        hi: 0,
    };
    i.attacker_element = 0;
    i.bypass_party_resist = true; // resistance block skipped entirely
    assert_eq!(damage_finish(&i), 500);
}

#[test]
fn damage_finish_no_damage_floor_uses_rand() {
    // Mitigation reduces over to 0 -> floor rand()%9 + 8.
    let mut i = finish(1);
    i.defender_resist = DefenderResist {
        lo: 0x2000_0000,
        hi: 0,
    };
    i.attacker_element = 0; // 1 >> 1 = 0 -> floor fires
    i.floor_rand = 0; // 0 % 9 + 8 = 8
    assert_eq!(damage_finish(&i), 8);
    i.floor_rand = 17; // 17 % 9 = 8 -> 8 + 8 = 16
    assert_eq!(damage_finish(&i), 16);
    // A predamage of 0 also triggers the floor.
    assert_eq!(damage_finish(&finish(0)), 8);
}

#[test]
fn damage_finish_summon_power_scale() {
    let mut i = finish(400);
    i.attacker_slot = 7; // summon body
    i.summon_power_pct = 150; // 400 * 150 / 100 = 600
    assert_eq!(damage_finish(&i), 600);
    i.summon_power_pct = 50; // 400 * 50 / 100 = 200
    assert_eq!(damage_finish(&i), 200);
}

#[test]
fn damage_finish_caps_at_9999() {
    assert_eq!(damage_finish(&finish(50_000)), 9999);
    // Exactly 9999 passes; 10000 caps.
    assert_eq!(damage_finish(&finish(9999)), 9999);
    assert_eq!(damage_finish(&finish(10_000)), 9999);
}

#[test]
fn spirit_gauge_fill_basic_accrual() {
    // over 50 of maxhp 500 -> pct = 10; gauge 0 -> 10.
    assert_eq!(
        spirit_gauge_fill(50, 500, 0, DefenderResist::default(), true),
        10
    );
    // pct floors at 1 even for tiny hits.
    assert_eq!(
        spirit_gauge_fill(1, 500, 0, DefenderResist::default(), true),
        1
    );
    // Clamps to 100.
    assert_eq!(
        spirit_gauge_fill(500, 500, 50, DefenderResist::default(), true),
        100
    );
}

#[test]
fn spirit_gauge_fill_gain_up_bits_party_only() {
    // pct = 40 (over 200 / max 500). hi & 0x200 -> +pct>>2 (=10); base +pct.
    let resist = DefenderResist { lo: 0, hi: 0x200 };
    // 0 + 10 + 40 = 50.
    assert_eq!(spirit_gauge_fill(200, 500, 0, resist, true), 50);
    // hi & 0x100 -> +pct/10 (=4); 0 + 4 + 40 = 44.
    let resist = DefenderResist { lo: 0, hi: 0x100 };
    assert_eq!(spirit_gauge_fill(200, 500, 0, resist, true), 44);
    // Both bits: +10 +4 +40 = 54.
    let resist = DefenderResist { lo: 0, hi: 0x300 };
    assert_eq!(spirit_gauge_fill(200, 500, 0, resist, true), 54);
    // Enemy defender (not party): gain-up bits ignored -> just +pct.
    assert_eq!(spirit_gauge_fill(200, 500, 0, resist, false), 40);
}

#[test]
fn spirit_gauge_fill_zero_maxhp_guard() {
    // Retail traps; the kernel returns the (clamped) gauge unchanged.
    assert_eq!(
        spirit_gauge_fill(100, 0, 73, DefenderResist::default(), true),
        73
    );
}

// --- summon spell XP + level-up (FUN_801ddb30 tail / FUN_801e70bc) -----

#[test]
fn summon_spell_xp_gain_non_kill_is_damage_proportional() {
    // damage * 12 / max_hp single-target; * 4 group-target.
    assert_eq!(summon_spell_xp_gain(100, 500, 600, false), 2); // 1200/600
    assert_eq!(summon_spell_xp_gain(100, 500, 600, true), 0); // 400/600
    assert_eq!(summon_spell_xp_gain(300, 500, 600, true), 2); // 1200/600
    // Integer floor, exactly as the MIPS divide.
    assert_eq!(summon_spell_xp_gain(149, 500, 600, false), 2); // 1788/600
}

#[test]
fn summon_spell_xp_gain_kill_is_flat_unit() {
    // damage >= target_hp -> flat 12 (single) / 4 (group), no division.
    assert_eq!(summon_spell_xp_gain(500, 500, 600, false), 12);
    assert_eq!(summon_spell_xp_gain(9999, 2, 600, true), 4);
}

#[test]
fn summon_spell_xp_gain_low_hp_target_grants_nothing() {
    // Both retail branches gate on target_hp >= 2.
    assert_eq!(summon_spell_xp_gain(1, 1, 600, false), 0);
    assert_eq!(summon_spell_xp_gain(9999, 1, 600, false), 0);
    assert_eq!(summon_spell_xp_gain(9999, 0, 600, true), 0);
    // Zero max HP on a non-kill: retail traps, engine returns 0.
    assert_eq!(summon_spell_xp_gain(1, 500, 0, false), 0);
}

#[test]
fn summon_magic_level_threshold_default_mult_is_raw_table() {
    // mult = 2 -> (t * 2) >> 1 == t.
    let table = [17u16, 50, 92, 144, 208, 288, 392, 536];
    assert_eq!(summon_magic_level_threshold(0x81, 1, &table), Some(17));
    assert_eq!(summon_magic_level_threshold(0x81, 8, &table), Some(536));
}

#[test]
fn summon_magic_level_threshold_triple_ids_scale_1_5x() {
    // mult = 3 -> (t * 3) >> 1.
    let table = [17u16, 50, 92, 144, 208, 288, 392, 536];
    for id in SUMMON_XP_TRIPLE_THRESHOLD_IDS {
        assert_eq!(summon_magic_level_threshold(id, 1, &table), Some(25)); // 51 >> 1
        assert_eq!(summon_magic_level_threshold(id, 2, &table), Some(75)); // 150 >> 1
    }
}

#[test]
fn summon_magic_level_up_is_strict_greater_and_caps_at_9() {
    let table = [17u16, 50, 92, 144, 208, 288, 392, 536];
    // Strict: xp == threshold does NOT level.
    assert!(!summon_magic_levels_up(0x81, 1, 17, &table));
    assert!(summon_magic_levels_up(0x81, 1, 18, &table));
    // Level cap: 9 never levels (retail pre-increment `< 9` guard).
    assert!(!summon_magic_levels_up(0x81, 9, 99999, &table));
    // Level 0 guarded (retail would read table[-1]).
    assert!(!summon_magic_levels_up(0x81, 0, 99999, &table));
    // Short table: level 8 needs table[7].
    assert!(!summon_magic_levels_up(0x81, 8, 99999, &table[..7]));
}

#[test]
fn escape_scores_weight_speed_and_missing_hp() {
    // Party: (SPD*3)>>1 + missing>>4. Enemy: SPD + missing>>5.
    let a = EscapeActor {
        speed: 20,
        hp: 100,
        max_hp: 260,
    };
    assert_eq!(escape_party_score(&[a]), 30 + 10); // 20*3/2 + 160/16
    assert_eq!(escape_enemy_score(&[a]), 20 + 5); // 20 + 160/32
    // A downed member still contributes (retail iterates every slot).
    let downed = EscapeActor {
        speed: 20,
        hp: 0,
        max_hp: 320,
    };
    assert_eq!(escape_party_score(&[downed]), 30 + 20);
    // Full-HP actor contributes SPD only.
    let full = EscapeActor {
        speed: 31,
        hp: 500,
        max_hp: 500,
    };
    assert_eq!(escape_party_score(&[full]), 46); // (31*3)>>1
    assert_eq!(escape_enemy_score(&[full]), 31);
}

#[test]
fn escape_roll_compare_is_roll_p_lt_roll_e() {
    // party_score 100, enemy_score 50: rand[0]=70 -> roll_p=70,
    // rand[1]=120 -> roll_e=120%50=20. 70 >= 20 -> escaped.
    let f = EscapeFlags::default();
    assert!(escape_roll(100, 50, f, [70, 120]));
    // rand[0]=10 -> roll_p=10 < 20 -> caught.
    assert!(!escape_roll(100, 50, f, [10, 120]));
    // Boundary: equal rolls escape (fail is strict `<`).
    assert!(escape_roll(100, 50, f, [20, 120]));
}

#[test]
fn escape_boost_scales_party_roll_1_5x() {
    // roll_p=14 -> boosted 14+7=21 >= roll_e=20 -> escaped where the
    // unboosted 14 < 20 would be caught.
    let mut f = EscapeFlags::default();
    assert!(!escape_roll(100, 50, f, [14, 120]));
    f.escape_boost = true;
    assert!(escape_roll(100, 50, f, [14, 120]));
}

#[test]
fn assured_escape_wins_the_compare_but_not_no_escape() {
    // Great Escape forces roll_p = roll_e: worst rand still escapes.
    let f = EscapeFlags {
        assured: true,
        ..Default::default()
    };
    assert!(escape_roll(100, 50, f, [0, 49]));
    // ...but the scripted no-escape flag still blocks it (Chicken King
    // is "assured escape (non-boss)").
    let f = EscapeFlags {
        assured: true,
        no_escape: true,
        ..Default::default()
    };
    assert!(!escape_roll(100, 50, f, [0, 49]));
    // The forced battle flag bypasses even no_escape.
    let f = EscapeFlags {
        forced: true,
        no_escape: true,
        ..Default::default()
    };
    assert!(escape_roll(100, 50, f, [0, 49]));
}

#[test]
fn escape_flags_fold_ability_word1_bits_52_and_55() {
    let mut f = EscapeFlags::default();
    f.fold_ability_word1(0);
    assert!(!f.escape_boost && !f.assured);
    f.fold_ability_word1(EscapeFlags::ESCAPE_BOOST_WORD1);
    assert!(f.escape_boost && !f.assured);
    f.fold_ability_word1(EscapeFlags::GREAT_ESCAPE_WORD1);
    assert!(f.escape_boost && f.assured);
    // OR-fold: later zero words don't clear earlier bits.
    f.fold_ability_word1(0);
    assert!(f.escape_boost && f.assured);
}
