//! The player's Auto attack: the attack-mode pick sets the per-fighter flag,
//! the round's start runs `FUN_801F0450`'s pool arm for every flagged Attack,
//! and the member's dispatch plays the rebuilt queue.

use super::*;
use crate::battle_input::{BattleCommand, BattleCommandSession, CommandPhase};
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
    world.battle.round_flow.phase = crate::battle_round::RoundPhase::Command;
    world
}

fn press(world: &mut World, button: PadButton) {
    world.set_pad(0);
    world.set_pad(button.mask());
    world.tick_battle_command();
}

fn on_ring(world: &mut World, actor: u8) {
    let attack = BattleCommand::MENU
        .iter()
        .position(|c| *c == BattleCommand::Attack)
        .unwrap() as u8;
    world.battle.command = Some(BattleCommandSession {
        actor,
        party_slot: actor,
        no_escape: false,
        phase: CommandPhase::Menu { cursor: attack },
    });
}

/// Attack -> the prompt -> `Auto` -> a target: the commit carries the flag.
#[test]
fn the_auto_chip_sets_the_fighters_flag_and_command_clears_it() {
    let mut world = party_world(2);
    on_ring(&mut world, 0);
    press(&mut world, PadButton::Cross); // Attack -> the Auto | Command prompt
    press(&mut world, PadButton::Cross); // the cursor rests on Auto
    // Confirm through whatever target cursor the pick opened.
    for _ in 0..4 {
        if world.battle.round_flow.pending[0].is_some() {
            break;
        }
        press(&mut world, PadButton::Cross);
    }
    assert!(matches!(
        world.battle.round_flow.pending[0],
        Some(PendingPartyAction::Attack { .. })
    ));
    assert!(world.battle.auto_combo.flags[0], "Auto flags the fighter");

    // A non-Attack commit leaves the flag clear.
    world.commit_party_command(1, PendingPartyAction::Spirit);
    assert!(!world.battle.auto_combo.flags[1]);
}

/// The round's start rebuilds a flagged Attack's queue from the four
/// direction commands, spent against the action gauge, and the dispatch
/// plays exactly that queue.
#[test]
fn the_round_start_pool_arm_builds_the_queue_the_dispatch_plays() {
    let mut world = party_world(1);
    world.battle.swing_costs[0] = [0x1E; 4];
    world.battle.auto_combo.flags[0] = true;
    world.battle.round_flow.pending[0] = Some(PendingPartyAction::Attack { target: 1 });
    world.run_auto_attack_pool_arms();
    let queue = world.battle.auto_combo.queues[0]
        .clone()
        .expect("a flagged Attack gets a pool-arm queue");
    assert!(!queue.is_empty(), "a 100-AP gauge affords 0x1E-cost swings");
    assert!(
        queue.iter().all(|b| (0x0C..=0x0F).contains(b)),
        "the pool and the refill only ever write direction commands: {queue:?}"
    );
    assert!(queue.len() <= 3, "100 AP buys at most three 0x1E swings");

    world.dispatch_pending_party_action(0, PendingPartyAction::Attack { target: 1 });
    let params = world.actors[0].battle.params;
    assert_eq!(&params[..queue.len()], &queue[..]);
    assert_eq!(params[queue.len()], 0, "terminated");
    assert!(world.battle.auto_combo.queues[0].is_none(), "played once");
}

/// An unflagged Attack takes the ordinary seed, not the pool arm.
#[test]
fn an_unflagged_attack_builds_no_pool_queue() {
    let mut world = party_world(1);
    world.battle.round_flow.pending[0] = Some(PendingPartyAction::Attack { target: 1 });
    world.run_auto_attack_pool_arms();
    assert!(world.battle.auto_combo.queues[0].is_none());
}
