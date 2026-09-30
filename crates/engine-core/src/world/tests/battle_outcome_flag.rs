//! Story flag 1 - the script-readable battle outcome - across every battle
//! end.
//!
//! Ground truth is `FUN_8003AEB0`'s back-from-battle arm
//! (`ghidra/scripts/funcs/8003aeb0.txt`): `ori 0x40` into `0x80085758` at
//! `0x8003B58C` when the party-survived bit `DAT_8007BD60 & 0x80` is up,
//! `andi 0xbf` at `0x8003B5A0` when it is not, and `andi 0x7f` (flag 0, the
//! scripted-loss latch) at `0x8003B608` on every arm. The bit is clear only
//! after a party wipe: the results sequencer and the escape roll's success
//! arm (`0x801E802C`) both raise it.
//!
//! The port used to touch flag 1 only on the wipe arms and on Cort's form
//! transition, so an ordinary win left whatever the flag held before the
//! fight - a scene that branched on the outcome after a won battle read a
//! stale value.

use super::*;
use crate::monster_catalog::{FormationDef, FormationSlot};
use vm::battle_action::BattleEndCause;

fn battle_world() -> World {
    let mut world = World {
        party: crate::world::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.actors[0].battle.hp = 100;
    let formation = FormationDef::new(7, vec![FormationSlot::new(1)]);
    world.field_return = Some(FieldReturnState {
        actors: world.actors.clone(),
        player_actor_slot: world.player_actor_slot,
        party_count: world.party.party_count,
    });
    world.battle.return_mode = SceneMode::Field;
    world.enter_battle_from_formation(&formation);
    world
}

/// Run the end-of-battle presentation for `cause` through to the field,
/// the way the live battle tick does.
fn end_battle(world: &mut World, cause: BattleEndCause) {
    world.battle.end = Some(cause);
    world.begin_battle_end_sequence();
    let mut frames = 0;
    while world.battle.victory.is_some() && frames < 2000 {
        world.tick_battle_end_sequence();
        frames += 1;
    }
    assert!(world.battle.victory.is_none(), "end sequence never exited");
}

#[test]
fn an_ordinary_win_sets_story_flag_1() {
    let mut world = battle_world();
    world.system_flag_clear(1);
    end_battle(&mut world, BattleEndCause::MonsterWipe);
    assert_eq!(world.mode, SceneMode::Field);
    assert!(
        world.system_flag_test(1),
        "a won battle raises the outcome flag (0x8003B58C)"
    );
    assert!(!world.game_over);
}

#[test]
fn an_escape_sets_story_flag_1() {
    let mut world = battle_world();
    world.system_flag_clear(1);
    end_battle(&mut world, BattleEndCause::Escaped);
    assert!(
        world.system_flag_test(1),
        "the escape roll's success arm raises the survived bit (0x801E802C)"
    );
}

#[test]
fn a_scripted_loss_clears_story_flag_1_after_a_previous_win() {
    let mut world = battle_world();
    end_battle(&mut world, BattleEndCause::MonsterWipe);
    assert!(world.system_flag_test(1));

    // The next fight is a scripted loss: the scene raised the latch.
    let mut world2 = battle_world();
    world2.flags.system_flags = world.flags.system_flags.clone();
    world2.system_flag_set(0);
    world2.actors[0].battle.liveness = 0;
    end_battle(&mut world2, BattleEndCause::PartyWipe);
    assert!(
        !world2.system_flag_test(1),
        "a wipe clears the outcome flag"
    );
    assert!(!world2.system_flag_test(0), "the latch is consumed");
    assert!(!world2.game_over, "the latch skips the game over");
}

#[test]
fn a_win_consumes_a_scripted_loss_latch_the_fight_did_not_use() {
    // A scene that raised the latch for a fight the party then won: retail's
    // shared `andi 0x7f` clears it on the survived arm too.
    let mut world = battle_world();
    world.system_flag_set(0);
    end_battle(&mut world, BattleEndCause::MonsterWipe);
    assert!(!world.system_flag_test(0), "flag 0 cleared on every return");
    assert!(world.system_flag_test(1));
}

#[test]
fn a_direct_finish_after_a_win_sets_story_flag_1() {
    // The runner / test path that calls `finish_battle` without the
    // presentation goes through the same arm.
    let mut world = battle_world();
    world.system_flag_clear(1);
    world.battle.end = Some(BattleEndCause::MonsterWipe);
    world.finish_battle();
    assert!(world.system_flag_test(1));
}
