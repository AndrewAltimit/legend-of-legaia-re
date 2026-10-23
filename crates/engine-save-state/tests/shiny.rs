use crate::support::capture_world;
use legaia_engine_core::world::test_drivers::BattleTestDrivers;
use legaia_engine_save_state::WorldSaveState;

#[test]
fn shiny_capture_marks_spell_shiny_and_persists_through_save() {
    let mut world = capture_world(1);
    world.set_spell_catalog(legaia_engine_battle::spells::SpellCatalog::vanilla());
    // Killer Bee captured as shiny (Seru 1 -> spell 0x20).
    world.seru.battle_captures = vec![7];
    world.seru.shiny_captures = vec![7];
    world.finish_battle();

    assert!(world.seru.log.has_learned(0, 1), "spell learned");
    assert!(
        world.seru.log.is_shiny(0, 0x20),
        "shiny capture flags the learned spell shiny"
    );
    // shiny_captures drained.
    assert!(world.seru.shiny_captures.is_empty());

    // Round-trips through the LGSF v4 save.
    let sf = world.save_full();
    assert!(
        sf.ext_v2
            .per_char
            .iter()
            .any(|(slot, ce)| *slot == 0 && ce.shiny_spells.contains(&0x20)),
        "shiny spell serialised into the save"
    );
    let mut reloaded = capture_world(1);
    reloaded.load_full(sf);
    assert!(
        reloaded.seru.log.is_shiny(0, 0x20),
        "shiny survives save/load"
    );
}
