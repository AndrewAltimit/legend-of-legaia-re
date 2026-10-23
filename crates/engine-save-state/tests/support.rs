//! Fixtures the LGSF round-trip tests share (a copy of the unit tests'
//! `capture_world`, built from the public API only).

use legaia_engine_core::world::World;

/// A registry whose Seru hits its learn threshold in one capture, and a
/// monster catalog linking monster id 7 -> Seru 1.
pub fn capture_world(party_count: u8) -> World {
    use legaia_engine_battle::monster_catalog::{MonsterCatalog, MonsterDef};
    use legaia_engine_battle::seru_learning::{SeruDef, SeruRegistry};

    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count,
            ..Default::default()
        },
        ..World::default()
    };
    // Zeroed roster (empty spell lists) so `build_battle_spell_session`
    // resolves a member per party slot; learned spells come from the log.
    world.party.roster = legaia_save::Party::zeroed(party_count.max(1) as usize);
    let mut reg = SeruRegistry::new();
    reg.insert(SeruDef {
        id: 1,
        name: "Spark".into(),
        spell_id: 0x20,
        capture_points: 100,
        learnable_mask: 0b0000_0111,
        learn_threshold: 100,
    });
    reg.insert(SeruDef {
        id: 2,
        name: "Slow".into(),
        spell_id: 0x21,
        capture_points: 40, // below threshold in one capture
        learnable_mask: 0b0000_0111,
        learn_threshold: 100,
    });
    world.set_seru_registry(reg);
    let mut cat = MonsterCatalog::new();
    cat.insert(MonsterDef::new(7, "Killer Bee", 25, 9).with_seru(1));
    cat.insert(MonsterDef::new(8, "Slime", 40, 8).with_seru(2));
    cat.insert(MonsterDef::new(9, "Wolf", 35, 12)); // no Seru
    world.set_monster_catalog(cat);
    world
}
