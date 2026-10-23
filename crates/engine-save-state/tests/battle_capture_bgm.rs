use crate::support::capture_world;
use legaia_engine_core::world::test_drivers::BattleTestDrivers;
use legaia_engine_core::world::*;
use legaia_engine_save_state::WorldSaveState;

#[test]
fn capture_progress_round_trips_through_save_load() {
    // Bank a sub-threshold capture, save, reload into a fresh world that
    // has the registry installed, and confirm the points + learned state
    // survive.
    let mut world = capture_world(1);
    world.seru.battle_captures = vec![7, 8]; // Seru 1 learns; Seru 2 banks 40
    world.finish_battle();
    assert!(world.seru.log.has_learned(0, 1));
    assert_eq!(world.seru.log.row(0, 2).points, 40);

    let save = world.save_full();

    let mut reloaded = capture_world(1);
    reloaded.load_full(save);
    assert!(
        reloaded.seru.log.has_learned(0, 1),
        "learned Spark restored"
    );
    assert_eq!(
        reloaded.seru.log.learned_spells(0),
        &[0x20],
        "spell list restored"
    );
    assert_eq!(
        reloaded.seru.log.row(0, 2).points,
        40,
        "sub-threshold progress restored"
    );
    assert!(
        !reloaded.seru.log.has_learned(0, 2),
        "still below threshold after reload"
    );
}

#[test]
fn arts_editor_chain_round_trips_through_save_into_the_battle_menu() {
    use legaia_engine_battle::tactical_arts_editor::{ChainEditor, EditInput, EditOutcome};

    // A field-side session: the player opens the Tactical Arts editor and
    // composes a brand-new chain for character slot 0 (Down, Up, Up).
    let mut field = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    let mut lib = field.chain_library();
    let mut ed = ChainEditor::new(0, &lib);
    // Cross: open the "+ New" editor.
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    for dir in [
        EditInput {
            down: true,
            ..Default::default()
        },
        EditInput {
            up: true,
            ..Default::default()
        },
        EditInput {
            up: true,
            ..Default::default()
        },
    ] {
        ed.tick(dir);
    }
    // Cross: commit to naming, then Cross again: confirm the default name.
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    ed.tick(EditInput {
        cross: true,
        ..Default::default()
    });
    assert!(
        matches!(ed.outcome(), Some(EditOutcome::Saved { slot: 0, .. })),
        "editor saved a new chain"
    );
    // Apply the edit to the library and store it back into the world -
    // the bridge under test (no direct `saved_chains` seeding).
    ed.apply_outcome(&mut lib).unwrap();
    field.store_chain_library(&lib);

    // The chain now serializes with the save block...
    let save = field.save_full();
    assert_eq!(save.ext_v2.saved_chains.len(), 1);

    // ...and a fresh boot that loads the save can offer it in battle.
    let mut battle = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    battle.load_full(save);
    let rows = battle.build_battle_arts_rows(0);
    assert_eq!(
        rows.len(),
        1,
        "the edited chain reaches the battle arts menu"
    );
    // Default new-chain name preset; 3 directional inputs => 3 synthetic hits.
    assert_eq!(rows[0].hits(), 3);
}
