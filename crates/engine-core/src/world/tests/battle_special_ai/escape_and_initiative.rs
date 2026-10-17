//! The escape roll, its accessories and formation latches, the side lockout,
//! initiative keys and the AP-boost gauge.

use super::*;
use legaia_engine_mode::mode::SceneMode;

/// The two "spirit gain up" finisher bits are the AP Boost accessory passives
/// (`0x28`/`0x29`): a wearer's spirit-art gauge charges faster from the same
/// hit, read off the rebuilt ability bitfield via `World::defender_resist`.
#[test]
fn ap_boost_accessory_accelerates_spirit_gauge() {
    use legaia_engine_battle::accessory_passives::AccessoryPassives;

    fn gauge_after_hit(ap_boost_passive: Option<u8>) -> u16 {
        let mut world = World {
            party: legaia_engine_menus::party_state::PartyState {
                party_count: 1,
                ..Default::default()
            },
            ..World::default()
        };
        world.actors[0].battle.max_hp = 500;
        world.actors[0].battle.hp = 500;
        world.actors[0].battle.liveness = 1;
        let mut party = legaia_save::Party::zeroed(1);
        if let Some(idx) = ap_boost_passive {
            world.set_accessory_passives(AccessoryPassives::from_entries([(0x50, idx)], []));
            let mut eq = party.members[0].equipment();
            eq.slots[7] = 0x50;
            party.members[0].set_equipment(eq);
        }
        world.party.roster = party;
        world.refresh_party_ability_bits();
        world.accrue_spirit_gauge(0, 200); // pct = 200*100/500 = 40
        world.actors[0].battle.spirit_gauge
    }

    assert_eq!(gauge_after_hit(None), 40, "base accrual is pct");
    // AP Boost 1 (0x28 -> word1 0x100): +pct/10 = +4.
    assert_eq!(gauge_after_hit(Some(0x28)), 44);
    // AP Boost 2 (0x29 -> word1 0x200): +pct>>2 = +10.
    assert_eq!(gauge_after_hit(Some(0x29)), 50);
}

/// One-party-member battle world for the Run escape roll (`FUN_801E791C`):
/// party SPD vs enemy SPD, both sides at full HP, optional accessory passive
/// on the member's slot-7 equip.
fn escape_world(party_speed: u16, enemy_speed: u16, passive: Option<u8>) -> World {
    use legaia_engine_battle::accessory_passives::AccessoryPassives;

    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.max_hp = 300;
    world.actors[1].battle.hp = 300;
    world.actors[1].battle.liveness = 1;
    world.battle.speed[0] = party_speed;
    world.battle.speed[1] = enemy_speed;
    let mut party = legaia_save::Party::zeroed(1);
    if let Some(idx) = passive {
        world.set_accessory_passives(AccessoryPassives::from_entries([(0x50, idx)], []));
        let mut eq = party.members[0].equipment();
        eq.slots[7] = 0x50;
        party.members[0].set_equipment(eq);
    }
    world.party.roster = party;
    world.refresh_party_ability_bits();
    world
}

/// The Run command's escape roll follows the retail `FUN_801E791C` score
/// compare: a fast party vs a slow enemy escapes on every seed (the enemy
/// score of 1 makes `roll_e` always 0 and the fail compare is strict `<`),
/// while a slow party vs a fast enemy is caught on essentially every seed
/// (`roll_p` pinned at 0 by a party score of ~1).
#[test]
fn run_escape_roll_follows_speed_and_hp_scores() {
    let mut always = 0;
    let mut rarely = 0;
    for seed in 0..50u32 {
        let mut fast = escape_world(1000, 1, None);
        fast.rng_state = seed;
        always += u32::from(fast.roll_battle_escape());

        let mut slow = escape_world(1, 1000, None);
        slow.rng_state = seed;
        rarely += u32::from(slow.roll_battle_escape());
    }
    assert_eq!(always, 50, "overwhelming speed advantage always escapes");
    assert!(
        rarely <= 2,
        "pinned-at-0 party roll is caught on almost every seed (got {rarely}/50 escapes)"
    );
}

/// Chicken King (Great Escape, passive `0x37` -> ability bit 55) forces the
/// party roll equal to the enemy roll, so even the worst matchup escapes;
/// Chicken Heart (Escape Boost, passive `0x34` -> bit 52) scales the party
/// roll 1.5x, raising the escape rate over the unboosted baseline.
#[test]
fn escape_accessories_fold_from_the_ability_bitfield() {
    for seed in 0..50u32 {
        let mut w = escape_world(1, 1000, Some(0x37));
        w.rng_state = seed;
        assert!(w.roll_battle_escape(), "Great Escape wins every compare");
    }

    let mut base = 0;
    let mut boosted = 0;
    for i in 0..200u32 {
        // Spread the seeds over the state space: the shaped draw is the
        // state's high half, which consecutive small seeds barely move (one
        // seed step moves the first state by `0x41C64E6D`, about 16838 in the high
        // half), so `0..200` would replay nearly one first draw 200 times.
        let seed = i.wrapping_mul(0x9E37_79B9);
        let mut w = escape_world(30, 45, None);
        w.rng_state = seed;
        base += u32::from(w.roll_battle_escape());

        let mut w = escape_world(30, 45, Some(0x34));
        w.rng_state = seed;
        boosted += u32::from(w.roll_battle_escape());
    }
    assert!(
        boosted > base,
        "Escape Boost raises the escape rate ({boosted} vs {base} of 200)"
    );
}

/// **Wiring oracle for the `ctx+0x290` -> `ctx+0x291` formation latch.**
///
/// A pre-emptive strike (`ctx+0x291 == 2`) makes `FUN_801E791C` set the party
/// roll equal to the enemy roll, so the `roll_p < roll_e` compare can no longer
/// fail. The worst possible matchup - party score pinned at ~1 against an enemy
/// score of 1000 - is caught on essentially every seed without it, so if
/// `roll_battle_escape` stops folding [`World::battle_formation_latched`] into
/// the escape flags this flips from 50/50 escapes to ~0/50.
///
/// This is the assertion that was missing while the latch existed as a
/// write-only field: latching `+0x290` into `+0x291` and never reading it back
/// is indistinguishable from not latching at all.
#[test]
fn latched_preemptive_strike_makes_the_escape_compare_unfailable() {
    use legaia_engine_battle_vm::battle_formulas::FormationAdvantage;

    let mut caught_without = 0;
    let mut escaped_with = 0;
    for seed in 0..50u32 {
        let mut w = escape_world(1, 1000, None);
        w.rng_state = seed;
        caught_without += u32::from(!w.roll_battle_escape());

        let mut w = escape_world(1, 1000, None);
        w.rng_state = seed;
        w.set_battle_formation_latched(FormationAdvantage::Preemptive);
        escaped_with += u32::from(w.roll_battle_escape());
    }
    assert!(
        caught_without >= 48,
        "baseline: the hopeless matchup is caught (got {caught_without}/50)"
    );
    assert_eq!(
        escaped_with, 50,
        "a latched pre-emptive strike escapes every seed"
    );
}

/// A **back attack** (`ctx+0x291 == 1`) is not the mirror of a pre-emptive
/// strike: `FUN_801E791C` compares the latched byte against `2` only, so a back
/// attack has no effect on the escape roll at all. Its cost is paid entirely in
/// the initiative lockout.
#[test]
fn latched_back_attack_does_not_touch_the_escape_roll() {
    use legaia_engine_battle_vm::battle_formulas::FormationAdvantage;

    for seed in 0..50u32 {
        let mut plain = escape_world(30, 45, None);
        plain.rng_state = seed;
        let a = plain.roll_battle_escape();

        let mut backed = escape_world(30, 45, None);
        backed.rng_state = seed;
        backed.set_battle_formation_latched(FormationAdvantage::BackAttack);
        let b = backed.roll_battle_escape();

        assert_eq!(a, b, "seed {seed}: back attack must not change the roll");
    }
}

/// "Escape assured" overstates the pre-emptive arm. Retail sets `roll_p =
/// roll_e` *before* testing `ctx+0x287` (`801e7af0` then `801e7b14`), so a
/// scripted no-flee battle still catches the party even after a pre-emptive
/// strike. Only the forced-flee arm bypasses `+0x287`.
#[test]
fn latched_preemptive_strike_still_loses_to_the_no_escape_flag() {
    use legaia_engine_battle_vm::battle_formulas::FormationAdvantage;

    for seed in 0..25u32 {
        let mut w = escape_world(1000, 1, None);
        w.rng_state = seed;
        w.set_battle_formation_latched(FormationAdvantage::Preemptive);
        w.battle.no_escape = true;
        assert!(
            !w.roll_battle_escape(),
            "seed {seed}: ctx+0x287 outranks the pre-emptive arm"
        );
    }
}

/// **Wiring oracle for the latch ordering.** The initiative seeder is the only
/// reader of the *unlatched* `ctx+0x290`, and the escape roll is the only
/// reader of the latched `ctx+0x291`, so the two passes must run in that order.
/// This drives the real sequence and checks both consumers saw their copy:
/// the monsters lose their round-one keys (the lockout ran against `+0x290`)
/// **and** the advantage survives into `+0x291` (the latch ran after it).
#[test]
fn seed_then_latch_feeds_both_formation_consumers() {
    use legaia_engine_battle_vm::battle_formulas::FormationAdvantage;

    let mut w = escape_world(40, 40, None);
    w.party.party_count = 3;
    for slot in 0..7 {
        w.actors[slot].battle.liveness = 1;
        w.actors[slot].battle.hp = 100;
        w.actors[slot].battle.max_hp = 100;
        w.battle.speed[slot] = 40;
    }
    w.set_battle_formation(FormationAdvantage::Preemptive);

    w.seed_battle_initiative();
    // Lockout consumer: monster slots 3..=6 sat out round one.
    for slot in 3..7 {
        assert_eq!(
            w.actors[slot].battle.init_key, 0,
            "slot {slot}: pre-emptive strike zeroes the monster keys"
        );
    }
    // Party slots 1 and 2 keep real keys (slot 0's is consumed to open).
    assert!(w.actors[1].battle.init_key > 0);
    assert!(w.actors[2].battle.init_key > 0);

    w.run_round_state_zero();
    assert_eq!(
        w.battle_formation_latched(),
        FormationAdvantage::Preemptive,
        "the advantage must survive into +0x291"
    );
    assert_eq!(
        w.battle_formation(),
        FormationAdvantage::None,
        "+0x290 is cleared by the latch"
    );
    // And the escape roll now sees it.
    w.rng_state = 7;
    assert!(w.roll_battle_escape());
}

/// The side lockout splits on the engine's `party_count`, not on retail's fixed
/// slot-3 boundary. Retail always reserves three party slots, so
/// `apply_side_lockout` can hardcode `0..=2` / `3..=6`; the engine compacts and
/// seats the first monster at `party_count`. With a one-member party, slot 1 is
/// a **monster**, and a back attack must lock out slot 0 alone - the fixed
/// split would zero the monsters' keys instead, handing round one to the side
/// that was supposed to lose it.
#[test]
fn side_lockout_follows_party_count_not_the_fixed_retail_boundary() {
    use legaia_engine_battle_vm::battle_formulas::FormationAdvantage;

    for (advantage, party_locked) in [
        (FormationAdvantage::BackAttack, true),
        (FormationAdvantage::Preemptive, false),
    ] {
        let mut w = escape_world(40, 40, None);
        w.party.party_count = 1; // slot 0 party, slots 1..=6 monsters
        for slot in 0..7 {
            w.actors[slot].battle.liveness = 1;
            w.actors[slot].battle.hp = 100;
            w.actors[slot].battle.max_hp = 100;
            w.battle.speed[slot] = 40;
        }
        w.set_battle_formation(advantage);
        w.rng_state = 3;
        w.reseed_initiative();

        assert_eq!(
            w.actors[0].battle.init_key == 0,
            party_locked,
            "{advantage:?}: party slot 0 lockout"
        );
        for slot in 1..7 {
            assert_eq!(
                w.actors[slot].battle.init_key == 0,
                !party_locked,
                "{advantage:?}: monster slot {slot} lockout"
            );
        }
    }
}

/// **Wiring oracle for the initiative kernel.** `FUN_801DA780` adds a
/// wounded-HP bonus to the rolled key, and for a party slot below a quarter HP
/// that bonus is `(max_hp - hp) >> 4` - far larger than the whole SPD roll.
///
/// With both slots at SPD 10 the rolled key spans `[11, 16]` (`rand % 6`); a party member at
/// 1/1000 HP adds `999 >> 4 = 62`, so its key lands in `[73, 78]` and beats the
/// healthy slot on *every* seed. The pre-remediation
/// `speed + rng % (speed/2 + 1) + 1` inline formula has no such term, so both
/// slots drew from the same `[11, 16]` band and this assertion fails on most
/// seeds.
#[test]
fn initiative_keys_carry_the_wounded_bonus_from_the_kernel() {
    for seed in 0..40u32 {
        let mut w = escape_world(10, 10, None);
        w.party.party_count = 2;
        w.rng_state = seed;
        for slot in 0..2 {
            w.actors[slot].battle.liveness = 1;
            w.actors[slot].battle.max_hp = 1000;
            w.battle.speed[slot] = 10;
        }
        w.actors[0].battle.hp = 1; // near death
        w.actors[1].battle.hp = 1000; // untouched
        w.set_battle_formation(legaia_engine_battle_vm::battle_formulas::FormationAdvantage::None);

        w.reseed_initiative();

        let wounded = w.actors[0].battle.init_key;
        let healthy = w.actors[1].battle.init_key;
        assert!(
            (73..=78).contains(&wounded),
            "seed {seed}: wounded key {wounded} outside the kernel's [73,78] band"
        );
        assert!(
            (11..=16).contains(&healthy),
            "seed {seed}: healthy key {healthy} outside the plain [11,16] band"
        );
        assert!(
            wounded > healthy,
            "seed {seed}: the wounded bonus must win ({wounded} vs {healthy})"
        );
    }
}

/// Retail folds the escape accessories only over party members with live HP
/// (`+0x14C != 0`): a downed Chicken King wearer contributes nothing.
#[test]
fn downed_wearer_does_not_fold_escape_accessories() {
    let mut caught = 0;
    for seed in 0..50u32 {
        let mut w = escape_world(1, 1000, Some(0x37));
        w.actors[0].battle.liveness = 0;
        w.rng_state = seed;
        caught += u32::from(!w.roll_battle_escape());
    }
    assert!(
        caught >= 48,
        "downed wearer's assured-escape bit is ignored (got {caught}/50 caught)"
    );
}

/// Retail runs action state `0x00` at every round's `0xFE` (`0x801D3224`), so
/// a pre-emptive strike's assured escape (`+0x291 == 2`) lasts one round:
/// round two's pass copies the `+0x290` round one cleared. The port ran the
/// pass once per battle and kept the latch for the whole fight.
#[test]
fn the_pre_emptive_escape_latch_lasts_one_round() {
    use legaia_engine_battle_vm::battle_formulas::FormationAdvantage;

    let mut w = escape_world(40, 40, None);
    w.set_battle_formation(FormationAdvantage::Preemptive);
    w.run_round_state_zero();
    assert_eq!(w.battle_formation_latched(), FormationAdvantage::Preemptive);
    // A second pass in the same round is a no-op.
    w.run_round_state_zero();
    assert_eq!(w.battle_formation_latched(), FormationAdvantage::Preemptive);
    // Round boundary: the next round's pass latches the cleared byte.
    w.end_battle_round();
    w.run_round_state_zero();
    assert_eq!(w.battle_formation_latched(), FormationAdvantage::None);
}

/// `FUN_801DA780` halves a key whose actor's `+0x16E` word is exactly
/// `0x1000` (`lhu` / `li 0x1000` / `bne` at `0x801DA8F8..0x801DA900`,
/// `srl 1` at `0x801DA910`): an actor carrying that bit alone rolls into
/// half the band, and one carrying it beside another bit is untouched.
#[test]
fn initiative_halves_a_key_whose_status_word_is_exactly_0x1000() {
    let key_with = |status: u16| {
        let mut w = escape_world(40, 40, None);
        w.party.party_count = 1;
        w.rng_state = 11;
        w.actors[0].battle.liveness = 1;
        w.actors[0].battle.hp = 100;
        w.actors[0].battle.max_hp = 100;
        w.battle.speed[0] = 40;
        w.actors[0].battle.field_flags = status;
        w.set_battle_formation(legaia_engine_battle_vm::battle_formulas::FormationAdvantage::None);
        w.reseed_initiative();
        w.actors[0].battle.init_key
    };
    let plain = key_with(0);
    assert_eq!(key_with(0x1000), plain >> 1);
    assert_eq!(
        key_with(0x1000 | 0x1),
        plain,
        "not an exact match: no halving"
    );
}
