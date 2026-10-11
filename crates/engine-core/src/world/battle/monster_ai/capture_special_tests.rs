use crate::world::World;
use legaia_asset::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};

const GUILTY_CROSS: u8 = 0x37;

fn world_with_capture_special() -> World {
    let mut entries = vec![SpellEntry::default(); 0x100];
    entries[GUILTY_CROSS as usize] = SpellEntry {
        class: CAPTURE_CLASS,
        sub_class: 2,
        mp: 12,
        name: Some("Guilty Cross".into()),
        ..SpellEntry::default()
    };
    let mut world = World::default();
    world.menu.text = Some(crate::pause_screens::MenuTextTables {
        spell_names: Some(SpellNameTable::from_entries(entries)),
        ..Default::default()
    });
    world
}

/// A capture-class special is kept out of the spell catalog, so the
/// monster turn's cast record comes off the disc table - without it the
/// turn found no record and struck instead of casting.
#[test]
fn a_capture_special_has_a_cast_record_the_catalog_does_not_carry() {
    let world = world_with_capture_special();
    assert!(world.tables.spell_catalog.get(GUILTY_CROSS).is_none());
    let def = world.monster_cast_def(GUILTY_CROSS).expect("a cast record");
    assert_eq!(
        (def.id, def.mp_cost, def.effect_class),
        (GUILTY_CROSS, 12, 2)
    );
    assert!(
        World::default().monster_cast_def(GUILTY_CROSS).is_none(),
        "no disc table, no record"
    );
}

fn cort_under_shield(monster_id: u16, hp: u16) -> World {
    let mut world = World::default();
    world.party.party_count = 3;
    for _ in 0..4 {
        world.actors.push(crate::world::Actor::default());
    }
    let cort = &mut world.actors[3];
    cort.battle_monster_id = Some(monster_id);
    cort.battle.max_hp = 50_000;
    cort.battle.hp = hp;
    world.battle.monster_ai_state.flag_bd84 = 1;
    world
}

/// Cort's Mystic Shield holds until he is worn to half HP
/// (`FUN_8004CE2C`, `0x8004D5CC..0x8004D5D4`), then the pass restores
/// the reaction run arm 4 blanked and clears the shield word.
#[test]
fn the_mystic_shield_breaks_at_half_hp() {
    let mut world = cort_under_shield(0xB4, 25_001);
    world.tick_mystic_shield_break();
    assert!(world.mystic_shield_up(), "above half HP it holds");

    world.actors[3].battle.hp = 25_000;
    world.tick_mystic_shield_break();
    assert!(!world.mystic_shield_up(), "at half HP it breaks");
    let p = &world.actors[3].battle.params;
    assert_eq!(
        [
            p[0x1EF - 0x1DF],
            p[0x1F0 - 0x1DF],
            p[0x1F1 - 0x1DF],
            p[0x1F2 - 0x1DF]
        ],
        [3, 2, 4, 5]
    );

    let mut other = cort_under_shield(0xB5, 1);
    other.tick_mystic_shield_break();
    assert!(
        other.mystic_shield_up(),
        "the arm is keyed on formation 0xB4"
    );
}
