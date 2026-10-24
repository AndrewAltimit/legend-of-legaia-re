//! The battle Magic and Item windows' models (`battle_spell_menu_model`,
//! `battle_item_menu_model`): what the windowed surfaces read off the open
//! submenu sessions. The sessions' own input and commit stay in engine-core's
//! `world::tests::battle_items_magic`.

use legaia_engine_battle_hud::{battle_item_menu_model, battle_spell_menu_model};
use legaia_engine_battle_menus::battle_input::CommandPhase;
use legaia_engine_core::world::World;
use legaia_engine_core::world::test_drivers::BattleTestDrivers;
use legaia_engine_mode::mode::SceneMode;

/// One party member and one monster at `monster_hp`, with a catalog holding
/// the two items these tests use: Heal (ally side) and Bomb (enemy side).
fn offensive_item_world(monster_hp: u16, monster_id: u16) -> World {
    use legaia_engine_menus::items::{ItemCatalog, ItemEffect, ItemEntry};
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.battle.player_driven = true;
    world.mode = SceneMode::Battle;
    let mut catalog = ItemCatalog::new();
    for (id, name, effect, usable_in_field) in [
        (0x01, "Heal", ItemEffect::Heal { amount: 100 }, true),
        (0x13, "Bomb", ItemEffect::Damage { amount: 200 }, false),
    ] {
        catalog.insert(ItemEntry {
            id,
            name,
            effect,
            usable_in_battle: true,
            usable_in_field,
        });
    }
    world.set_item_catalog(catalog);
    world.actors[0].battle.max_hp = 200;
    world.actors[0].battle.hp = 200;
    world.actors[0].battle.liveness = 1;
    world.actors[1].battle.max_hp = monster_hp;
    world.actors[1].battle.hp = monster_hp;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(monster_id);
    world.battle_ctx.active_actor = 0;
    world
}

/// Take `Begin` on the party's commit-confirm screen (`0x6E`) when the last
/// commit raised it, so the round plays out.
fn take_commit_begin(w: &mut World) {
    let Some(cmd) = w.battle.command.as_mut() else {
        return;
    };
    if !matches!(cmd.phase, CommandPhase::CommitConfirm { .. }) {
        return;
    }
    cmd.phase = CommandPhase::BeginRound;
    w.tick_battle_command();
}

/// The target panel the hosts draw lists ONLY the selected item's side -
/// retail's rule is structural, not a confirm-time buzz: state `0x64`'s
/// cursor walk wraps inside the seated party band `[0, ctx[+0x00])`
/// (`FUN_801D0748` `0x801D2BE8`/`0x801D2C78`), so a heal item's panel is
/// the party roster and an enemy row is unrepresentable; the (test-only)
/// offensive items take the monster-ring side (`0x5B`).
#[test]
fn heal_item_target_panel_lists_party_rows_only() {
    use legaia_engine_system::input::PadButton;

    let mut world = offensive_item_world(500, 7);
    world.actors[0].battle.hp = 120; // hurt, so the heal is admissible
    world.party.inventory.insert(0x01, 1); // Heal (ally-side).
    world.battle.item_menu = Some(world.build_battle_item_session());
    // The session roster still carries both sides (the offensive items
    // need the enemy rows) - the model is where the side rule lands.
    assert_eq!(world.battle.item_menu.as_ref().unwrap().targets.len(), 2);
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu(); // confirm the heal -> target select
    take_commit_begin(&mut world);
    let m = battle_item_menu_model(&world).expect("menu model");
    let (rows, cursor) = m.targets.expect("target select armed");
    assert_eq!(rows.len(), 1, "party rows only - no enemy in a heal panel");
    assert_eq!(cursor, 0);
    // The mirror side: the Bomb's panel lists only the enemy.
    let mut world = offensive_item_world(500, 7);
    world.party.inventory.insert(0x13, 1);
    world.battle.item_menu = Some(world.build_battle_item_session());
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    world.tick_battle_item_menu();
    take_commit_begin(&mut world);
    let m = battle_item_menu_model(&world).expect("menu model");
    let (rows, cursor) = m.targets.expect("target select armed");
    assert_eq!(rows.len(), 1, "enemy rows only in an offensive panel");
    assert_eq!(cursor, 0, "session cursor remapped onto the filtered list");
}

/// The battle Magic window's model: one row per learned spell, the hand on
/// the session cursor, the highlighted spell's effective MP cost in the
/// `MP` tab and a level of at least 1 - and no model outside battle.
#[test]
fn the_battle_spell_menu_model_projects_the_open_window() {
    use legaia_engine_battle::spells::SpellCatalog;
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.tables.spell_catalog = SpellCatalog::vanilla();
    assert!(battle_spell_menu_model(&world).is_none(), "no window open");
    world.battle.spell_menu = Some(legaia_engine_battle::battle_magic::BattleSpellSession::new(
        0,
        0,
        &[0x41],
        &world.tables.spell_catalog,
        200,
        0,
    ));
    let cost = world.tables.spell_catalog.get(0x41).unwrap().mp_cost;
    let m = battle_spell_menu_model(&world).expect("browsing model");
    assert_eq!(m.rows.len(), 1);
    assert!(m.rows[0].1, "200 MP covers the spell");
    assert_eq!(m.cursor, Some(0));
    let (lv, mp) = m.level_mp.expect("the Lv / MP tabs");
    assert!(lv >= 1);
    assert_eq!(mp, u16::from(cost));

    world.mode = SceneMode::Field;
    assert!(battle_spell_menu_model(&world).is_none(), "battle only");
}
