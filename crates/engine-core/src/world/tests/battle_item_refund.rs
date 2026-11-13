//! The win arm's unspent-item refund (`FUN_8004E568`,
//! `0x8004F354..0x8004F38C`): the item window's commit takes the copy out of
//! the bag and the effect lands only at the member's turn, so a battle that
//! ends before that turn - the last monster fled or killed by an earlier
//! actor - hands the copy back.

use super::*;
use legaia_engine_battle::victory::VictoryPhase;
use legaia_engine_battle_loop::round_flow::PendingPartyAction;
use legaia_engine_battle_vm::battle_action::ActionCategory;
use legaia_engine_mode::mode::SceneMode;

const LEAF: u8 = 0x60;

/// A won battle with two party seats, each having committed one `LEAF`
/// (already taken out of a bag of four), and `setup` applied to the seats.
fn bag_after_win(setup: impl FnOnce(&mut World)) -> u8 {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 2,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.party.inventory.add(LEAF, 2);
    for i in 0..2 {
        let b = &mut world.actors[i].battle;
        b.max_hp = 100;
        b.hp = 40;
        b.liveness = 1;
        b.init_key = 0;
        b.action_category = ActionCategory::Item.as_byte();
        world.battle.round_flow.pending[i] = Some(PendingPartyAction::Item {
            item_id: LEAF,
            used_slots: vec![i as u8],
        });
    }
    setup(&mut world);
    world.battle.end = Some(BattleEndCause::MonsterWipe);
    world.begin_battle_end_sequence();
    for _ in 0..World::VICTORY_LOAD_FRAMES {
        world.tick_battle_end_sequence();
    }
    let seq = world.battle.victory.expect("sequence armed");
    assert!(matches!(seq.phase, VictoryPhase::Results { .. }));
    world.party.inventory.get(&LEAF).copied().unwrap_or(0)
}

#[test]
fn an_item_whose_turn_never_came_goes_back_in_the_bag() {
    // Both turns taken (keys spent): nothing back.
    assert_eq!(bag_after_win(|_| {}), 2);
    // Seat 1's turn never came: its copy comes back.
    assert_eq!(bag_after_win(|w| w.actors[1].battle.init_key = 7), 3);
    // Both unspent.
    assert_eq!(
        bag_after_win(|w| {
            w.actors[0].battle.init_key = 9;
            w.actors[1].battle.init_key = 7;
        }),
        4
    );
}

#[test]
fn a_downed_or_non_item_seat_gets_nothing_back() {
    // Downed: the loop takes the HP floor (`0x8004F234`) instead.
    assert_eq!(
        bag_after_win(|w| {
            w.actors[1].battle.init_key = 7;
            w.actors[1].battle.hp = 0;
        }),
        2
    );
    // A non-item commit with an unspent key.
    assert_eq!(
        bag_after_win(|w| {
            w.actors[1].battle.init_key = 7;
            w.actors[1].battle.action_category = 0;
        }),
        2
    );
}
