//! Casts gated on the kind of fight: Dead End Crisis outside the Cort fight and
//! the Spikefish flute.

use super::*;

/// A world with Koru-style casting set up for PROT 0961: party of three at
/// seats `0..3`, the caster at seat 3 (formation slot 0) carrying `lead`.
fn dead_end_crisis_world(lead: u16) -> World {
    use legaia_game_tables::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};
    let mut world = module_code_world();
    world.party.party_count = 3;
    world.battle_ctx.active_actor = 3;
    world.actors[3].battle_monster_id = Some(lead);
    world.set_battle_attack(3, 300);
    for i in 0..8 {
        world.actors[i].battle.hp = 9000;
        world.actors[i].battle.max_hp = 9000;
    }
    let mut entries = vec![SpellEntry::default(); 0x100];
    entries[0xA1].class = CAPTURE_CLASS;
    entries[0xA1].sub_class = 0x1A;
    entries[0xA1].mp = 0;
    world.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
        spell_names: Some(SpellNameTable::from_entries(entries)),
        ..Default::default()
    });
    world
}

/// PROT 0961 outside the evolved-Cort fight is Koru's scripted wipe: arm 3
/// keeps the `9999` it loaded (`0x801F739C`) and calls no wrapper, so the
/// fold empties every party seat and draws nothing from the RNG.
#[test]
fn dead_end_crisis_wipes_the_party_outside_the_cort_fight() {
    let mut world = dead_end_crisis_world(0xB6);
    assert_eq!(world.cast_module_for(0xA1), Some(961));
    assert!(world.dead_end_crisis_wipes(0xA1));
    let rng_before = world.rng_state;
    world.casting.pending_cast = Some(crate::world::PendingCast {
        caster: 3,
        spell_id: 0xA1,
        targets: vec![0, 1, 2],
        on_flight: false,
    });
    world.fold_pending_cast();
    for seat in 0..3 {
        assert_eq!(world.actors[seat].battle.hp, 0, "seat {seat} survived");
    }
    assert_eq!(world.rng_state, rng_before, "the wipe drew from the RNG");
}

/// In the evolved-Cort fight (`0x8007BD0C == 0xB5`) the same body rolls its
/// wrapper instead.
#[test]
fn dead_end_crisis_rolls_in_the_cort_fight() {
    let world = dead_end_crisis_world(0xB5);
    assert!(!world.dead_end_crisis_wipes(0xA1));
}

/// A three-member party for PROT 0925's outcome arm: three living members,
/// none of them with a turn still due.
fn flute_world(no_escape: bool) -> World {
    let mut world = module_code_world();
    world.party.party_count = 3;
    world.battle.no_escape = no_escape;
    world.battle_ctx.turn_cursor = 1;
    world.actors[1].battle.init_key = 4;
    world
}

/// The Spikefish flute (spell `0x97`, PROT 0925) on a scripted fight cannot
/// flee; its arm-8 round tail moves the turn cursor by the living party
/// minus one and spends every party seat's initiative key
/// (`0x801F79B4..0x801F7A88`) - which ends a round before the monsters still
/// due to act have moved (the Koru flute glitch, retail behaviour).
#[test]
fn the_spikefish_flute_on_a_scripted_fight_skips_the_round() {
    let mut world = flute_world(true);
    if world.cast_module_for(0x97) != Some(925) {
        eprintln!("[skip] spell 0x97 does not resolve to PROT 0925 without a disc");
        return;
    }
    world.casting.module_phase = 8;
    world.run_cast_module_code(0x97, 8).unwrap();
    assert_eq!(world.casting.module_phase, 9);
    assert_eq!(world.battle_ctx.turn_cursor, 3);
    assert!(world.actors[..3].iter().all(|a| a.battle.init_key == 0));
    assert!(!world.casting.module_flee, "a scripted fight cannot flee");
}

/// On an ordinary fight the same arm stages the party's flee and hands the
/// action SM the run band's wait with the escape outcome.
#[test]
fn the_spikefish_flute_on_an_ordinary_fight_flees() {
    let mut world = flute_world(false);
    if world.cast_module_for(0x97) != Some(925) {
        eprintln!("[skip] spell 0x97 does not resolve to PROT 0925 without a disc");
        return;
    }
    world.actors[2].battle.hp = 0;
    world.casting.module_phase = 8;
    world.run_cast_module_code(0x97, 8).unwrap();
    assert!(world.casting.module_flee);
    assert_eq!(
        world.actors[2].battle.hp, 1,
        "the flee floors party HP at 1"
    );
    assert_eq!(world.actors[0].battle.facing_angle, 0x800, "flee staging");
}
