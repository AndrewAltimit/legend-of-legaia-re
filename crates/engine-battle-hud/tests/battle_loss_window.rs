//! The results frame's loss window (screen element `0x42`, opened by
//! `FUN_8004E568` at `0x8004F900` on a party wipe) and the spoils window it
//! replaces.
//!
//! These read the battle HUD's result-window projections; the window gate the
//! simulation keeps (`World::battle_spoils_window_up`) is pinned in
//! engine-core's `world::tests::battle_special_word`.

use legaia_engine_battle_hud::{battle_defeat_banner, battle_spoils_banner};
use legaia_engine_battle_vm::battle_action::BattleEndCause;
use legaia_engine_battle_vm::battle_party_panel::DefeatText;
use legaia_engine_core::world::World;
use legaia_engine_core::world::test_drivers::BattleTestDrivers;
use legaia_engine_mode::mode::SceneMode;

/// A battle with a named lead, `party` members and one monster.
fn loss_world(party: u8, word: u32) -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: party,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    for i in 0..=usize::from(party) {
        let b = &mut world.actors[i].battle;
        b.max_hp = 100;
        b.hp = 100;
        b.liveness = 1;
    }
    world.actors[usize::from(party)].battle_monster_id = Some(0x10);
    world.battle.special_word = word;
    let mut lead = legaia_save::CharacterRecord::zeroed();
    lead.set_name("Lead");
    world.party.roster = legaia_save::Party {
        members: vec![lead; usize::from(party)],
    };
    // Placeholder pieces, not the disc's text.
    world.tables.defeat_text = Some(DefeatText {
        solo_suffix: " SOLO".into(),
        team_tail: " TEAM".into(),
    });
    world
}

fn wipe(world: &mut World) {
    world.battle.end = Some(BattleEndCause::PartyWipe);
    world.begin_battle_end_sequence();
}

#[test]
fn a_wipe_opens_the_loss_window_naming_the_lead() {
    let mut solo = loss_world(1, 0);
    wipe(&mut solo);
    let banner = battle_defeat_banner(&solo).expect("loss window up");
    assert_eq!(banner.line.as_deref(), Some("Lead SOLO"));

    let mut team = loss_world(2, 0);
    wipe(&mut team);
    assert_eq!(
        battle_defeat_banner(&team).and_then(|b| b.line).as_deref(),
        Some("Lead TEAM")
    );
}

#[test]
fn a_special_battle_wipe_opens_no_loss_window() {
    let mut world = loss_world(1, 0x100);
    wipe(&mut world);
    assert!(battle_defeat_banner(&world).is_none());
}

#[test]
fn a_win_opens_no_loss_window() {
    let mut world = loss_world(1, 0);
    world.battle.end = Some(BattleEndCause::MonsterWipe);
    world.begin_battle_end_sequence();
    for _ in 0..World::VICTORY_LOAD_FRAMES {
        world.tick_battle_end_sequence();
    }
    assert!(world.battle.victory.expect("armed").window_opened);
    assert!(battle_defeat_banner(&world).is_none());
}

/// `last_rewards` outlives its battle; a wipe after a win must not re-show
/// that win's spoils in place of the loss window.
#[test]
fn a_wipe_after_a_win_does_not_reshow_the_old_spoils() {
    let mut world = loss_world(1, 0);
    world.battle.last_rewards = Some(Default::default());
    wipe(&mut world);
    assert!(battle_spoils_banner(&world).is_none());
    assert!(battle_defeat_banner(&world).is_some());
}

/// The window spawns at its seat A (`y = 236`, record `0x42`) and glides to
/// its rest seat (`160`) over `0x10` frames; the stepper is the battle
/// frame's, which keeps running under the results sequence.
#[test]
fn the_loss_window_glides_up_from_below_the_screen() {
    let mut world = loss_world(1, 0);
    wipe(&mut world);
    assert_eq!(battle_defeat_banner(&world).expect("up").slide_y, 76);
    world.live_battle_tick();
    let first = battle_defeat_banner(&world).expect("up").slide_y;
    assert!(0 < first && first < 76, "moving: {first}");
    for _ in 0..16 {
        world.live_battle_tick();
    }
    assert_eq!(battle_defeat_banner(&world).expect("up").slide_y, 0);
}

#[test]
fn without_the_disc_pool_the_window_opens_empty() {
    let mut world = loss_world(1, 0);
    world.tables.defeat_text = None;
    wipe(&mut world);
    assert_eq!(battle_defeat_banner(&world).expect("up").line, None);
}
