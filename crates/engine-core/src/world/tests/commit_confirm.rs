//! The party-wide commit confirm (`0x6E`): raised once after the last
//! member's commit, `Begin` plays the round, `Reselect` steps back onto the
//! last member's ring and refunds a committed item. A pad-driven ladder
//! through `World::tick_battle_command`, the seam both hosts tick.

use super::*;
use crate::battle_flow::BattleFlowState;
use crate::battle_hud::{CommandChipPhase, battle_command_chips};
use crate::battle_input::{BattleCommandSession, CommandPhase};
use crate::battle_round::PendingPartyAction;
use crate::input::PadButton;

fn party_world(party: u8) -> World {
    let mut world = World::new();
    world.party.party_count = party;
    world.battle.player_driven = true;
    world.toggles.live_gameplay_loop = true;
    world.mode = SceneMode::Battle;
    for i in 0..usize::from(party) + 2 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    world
}

fn commit_spirit(world: &mut World, actor: u8) {
    world.battle.command = Some(BattleCommandSession {
        actor,
        party_slot: actor,
        no_escape: false,
        phase: CommandPhase::SpiritGuard,
    });
    world.tick_battle_command();
}

fn press(world: &mut World, button: PadButton) {
    world.set_pad(0);
    world.set_pad(button.mask());
    world.tick_battle_command();
}

fn on_confirm(world: &World) -> bool {
    matches!(
        world.battle.command.as_ref().map(|c| &c.phase),
        Some(CommandPhase::CommitConfirm { .. })
    )
}

#[test]
fn the_screen_waits_for_the_last_member_then_begin_plays_the_round() {
    let mut world = party_world(3);
    commit_spirit(&mut world, 0);
    assert!(!on_confirm(&world), "member 1 still owes a command");
    commit_spirit(&mut world, 1);
    assert!(!on_confirm(&world), "member 2 still owes a command");
    commit_spirit(&mut world, 2);
    assert!(on_confirm(&world), "the last commit raises 0x6E");
    assert_eq!(world.battle.flow, BattleFlowState::CommitBegin);
    let chips = battle_command_chips(&world).expect("the confirm draws");
    assert_eq!(chips.phase, CommandChipPhase::CommitConfirm);
    assert_eq!(chips.chips.len(), 2);
    assert_eq!(chips.cursor, 0, "the highlight opens on Begin");

    // Cross takes the highlighted Begin: the round plays out.
    press(&mut world, PadButton::Cross);
    assert!(world.battle.command.is_none(), "Begin leaves the screen");
    assert_eq!(world.battle.flow, BattleFlowState::Idle);
    for slot in 0..3 {
        assert!(
            world.battle.ap_gauges[slot].spirit_charged,
            "slot {slot} acted"
        );
    }
}

#[test]
fn reselect_reopens_the_last_members_ring_and_refunds_its_item() {
    let mut world = party_world(2);
    world.party.inventory.insert(0x01, 1);
    commit_spirit(&mut world, 0);
    // Member 1 commits an item; the copy is consumed at the commit.
    world.consume_item(0x01);
    world.commit_party_command(
        1,
        PendingPartyAction::Item {
            item_id: 0x01,
            used_slots: vec![0],
        },
    );
    assert!(on_confirm(&world));
    assert_eq!(world.party.inventory.get(&0x01).copied().unwrap_or(0), 0);

    // Circle (the cancel mask) is Reselect.
    press(&mut world, PadButton::Circle);
    let cmd = world.battle.command.as_ref().expect("a ring reopens");
    assert_eq!(cmd.actor, 1, "back onto the LAST member, not the first");
    assert!(matches!(cmd.phase, CommandPhase::Menu { .. }));
    assert_eq!(world.battle.flow, BattleFlowState::CategoryMenu);
    assert_eq!(
        world.party.inventory.get(&0x01).copied(),
        Some(1),
        "the committed item came back"
    );
    assert!(
        world.battle.round_flow.pending[1].is_none(),
        "member 1 owes a command again"
    );
    assert!(
        world.battle.round_flow.pending[0].is_some(),
        "member 0's commit stands"
    );

    // Re-commit and begin.
    commit_spirit(&mut world, 1);
    assert!(
        on_confirm(&world),
        "the screen comes back after the re-commit"
    );
    press(&mut world, PadButton::Left);
    assert!(world.battle.command.is_none(), "Left is Begin too");
}

#[test]
fn a_solo_party_reaches_the_screen_off_its_only_command() {
    let mut world = party_world(1);
    commit_spirit(&mut world, 0);
    assert!(on_confirm(&world), "0x6E for a party of one");
    // Right is Reselect; the step lands on the one member's ring.
    press(&mut world, PadButton::Right);
    let cmd = world.battle.command.as_ref().expect("the ring reopens");
    assert_eq!(cmd.actor, 0);
    assert!(matches!(cmd.phase, CommandPhase::Menu { .. }));
}
