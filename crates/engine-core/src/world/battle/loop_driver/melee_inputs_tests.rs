//! The live inputs the melee kernel `FUN_801EC3E4` folds into a hit beyond
//! ATK and DEF: the juggle counter `ctx[+0x0A]` (`0x801ECA20..0x801ECA80`),
//! the element pass (`0x801ED13C..0x801ED1B4`), War Soul
//! (`0x801ED0F8..0x801ED104`) and the Venom / Toxic scales
//! (`0x801ED254..0x801ED304`). Each test compares two hits drawn from the
//! same rand stream, so only the input under test differs.

use super::*;

/// One party member and one catalogued monster, neither with a block entry
/// (no action clips), so a swing draws only the two damage rolls.
fn duel() -> World {
    let mut w = World::new();
    w.enter_battle(1, 1);
    for i in 0..2 {
        w.actors[i].battle.liveness = 1;
        w.actors[i].battle.hp = 5000;
        w.actors[i].battle.max_hp = 5000;
    }
    w.set_battle_attack(0, 200);
    w.set_battle_attack(1, 200);
    w.actors[0].battle.active_target = 1;
    w.actors[1].battle.active_target = 0;
    w.tables
        .monster_catalog
        .insert(legaia_engine_battle::monster_catalog::MonsterDef::new(
            0x40, "Foe", 1, 10,
        ));
    w.actors[1].battle_monster_id = Some(0x40);
    w.rng_state = 0x1234_5678;
    w
}

/// Damage one swing of `committed` from seat 0 lands on seat 1.
fn swing(mut w: World, committed: u8) -> u32 {
    w.land_melee_hit(0, 1, BASIC_ATTACK_COMMAND, committed, false, false);
    w.actors[1].battle.damage_accum
}

#[test]
fn the_juggle_counter_climbs_only_inside_the_window() {
    let mut w = duel();
    // Window down: every hit is a chain's first, `Juggle = 1`.
    w.battle.juggle = 7;
    w.actors[1].battle_juggle_window = false;
    assert_eq!(w.next_juggle(1), 1);
    // Window up: one more than the previous hit's.
    w.actors[1].battle_juggle_window = true;
    w.battle.juggle = 1;
    w.land_melee_hit(0, 1, BASIC_ATTACK_COMMAND, 0, false, false);
    assert_eq!(w.battle.juggle, 2);
    w.land_melee_hit(0, 1, BASIC_ATTACK_COMMAND, 0, false, false);
    assert_eq!(w.battle.juggle, 3);
    // The window drops: back to 1.
    w.actors[1].battle_juggle_window = false;
    w.land_melee_hit(0, 1, BASIC_ATTACK_COMMAND, 0, false, false);
    assert_eq!(w.battle.juggle, 1);
}

#[test]
fn a_juggled_hit_adds_juggle_times_atk_over_64() {
    let opening = swing(duel(), 0);
    let mut w = duel();
    w.actors[1].battle_juggle_window = true;
    w.battle.juggle = 4; // this hit is the fifth of the chain
    let fifth = swing(w, 0);
    // `(juggle * atk) >> 6` on the offense: (5 - 1) * 200 / 64 = 12 more
    // against a zero defence that never reaches the rewrite.
    assert_eq!(fifth - opening, ((5 * 200) >> 6) - (200 >> 6));
}

#[test]
fn the_element_pass_scales_a_swing() {
    let with = |pct: u8| {
        let mut w = duel();
        let mut matrix = [[100u8; 8]; 8];
        matrix[3][7] = pct; // party element 3 striking the record's 7
        w.tables.element_affinity = Some(legaia_game_tables::element_affinity::ElementAffinity {
            matrix,
            character_elements: vec![3; 8],
            summon_power: [[100; 8]; 3],
        });
        w
    };
    let neutral = swing(with(100), 0);
    let halved = swing(with(50), 0);
    assert!(halved * 2 <= neutral + 1 && halved * 2 + 2 >= neutral);
    // An Art takes the factor twice (`0x801ED13C` and `0x801ED178`).
    let art_neutral = swing(with(100), 0x11);
    let art_halved = swing(with(50), 0x11);
    assert!(art_halved * 4 <= art_neutral + 3 && art_halved * 4 + 4 >= art_neutral);
}

#[test]
fn war_soul_lifts_an_art_hit_to_fourteen_tenths() {
    let with = |war_soul: bool| {
        let mut w = duel();
        w.load_party(legaia_save::Party::zeroed(1));
        w.party.party_count = 1;
        w.set_battle_attack(0, 200);
        let rec = &mut w.party.roster.members[0];
        let mut bits = rec.ability_bits();
        if war_soul {
            // `+0xF8` bit `0x1000`: byte `+0xF9`, bit 4.
            bits[5] |= 0x10;
        }
        rec.set_ability_bits(bits);
        w
    };
    let plain = swing(with(false), 0x11);
    let soul = swing(with(true), 0x11);
    assert!(soul > plain, "War Soul {soul} vs {plain}");
    // A direction hit is not an Art: the bit changes nothing.
    assert_eq!(swing(with(true), 0), swing(with(false), 0));
}

#[test]
fn venom_and_toxic_scale_both_sides_of_a_swing() {
    let plain = swing(duel(), 0);
    let mut w = duel();
    w.actors[0].battle.field_flags |= 0x2; // attacker Toxic: offense x7/10
    let weak = swing(w, 0);
    assert!(weak < plain, "toxic attacker {weak} vs {plain}");

    let defended = |status: u16| {
        let mut w = duel();
        w.set_battle_defense(1, 150);
        w.actors[1].battle.field_flags |= status;
        swing(w, 0)
    };
    // A venomed defender's guard falls to 9/10: more gets through.
    assert!(defended(0x1) > defended(0));
}

#[test]
fn safe_escape_triples_a_fleeing_members_guard() {
    use crate::battle_round::PendingPartyAction;
    let hit_on_member = |run: bool, safe_escape: bool| {
        let mut w = duel();
        w.load_party(legaia_save::Party::zeroed(1));
        w.party.party_count = 1;
        w.actors[0].battle.hp = 5000;
        w.set_battle_defense(0, 30);
        let rec = &mut w.party.roster.members[0];
        let mut bits = rec.ability_bits();
        if safe_escape {
            // `+0xF8` bit `0x200000`: byte `+0xFA`, bit 5.
            bits[6] |= 0x20;
        }
        rec.set_ability_bits(bits);
        if run {
            w.battle.round_flow.pending[0] = Some(PendingPartyAction::Run);
        }
        w.land_melee_hit(1, 0, BASIC_ATTACK_COMMAND, 0, false, false);
        w.actors[0].battle.damage_accum
    };
    let plain = hit_on_member(false, false);
    // Fleeing alone, or the passive alone, changes nothing.
    assert_eq!(hit_on_member(true, false), plain);
    assert_eq!(hit_on_member(false, true), plain);
    // Both: the guard roll is tripled (`0x801ED22C`), so less gets through.
    assert!(hit_on_member(true, true) < plain);
}
