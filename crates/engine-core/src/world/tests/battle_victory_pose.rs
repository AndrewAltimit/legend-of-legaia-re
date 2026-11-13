//! Who poses at a win: retail's results sequencer reads the acting slot
//! `ctx[+0x13]` (`FUN_8004E568`, `0x8004E5A4`), which the action SM's victory
//! arm leaves on the member who finished the fight, re-picks when that member
//! is down, and forces to seat 2 / seat 1 for the Songi formations
//! (`FUN_801E295C`, `0x801E6688..0x801E6760`).

use super::*;
use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot};
use legaia_engine_battle_vm::battle_action::{ActionState, BattleEndCause};
use legaia_engine_mode::mode::SceneMode;

/// Three party members (slots 0..=2) against one monster (slot 3), the
/// monster already down and `acting` holding the action at its end.
fn world_won_by(acting: u8) -> World {
    let mut world = World::new();
    world.party.party_count = 3;
    world.battle.player_driven = true;
    world.toggles.live_gameplay_loop = true;
    world.mode = SceneMode::Battle;
    for i in 0..4 {
        let a = &mut world.actors[i];
        a.active = true;
        a.battle.liveness = 1;
        a.battle.hp = 100;
        a.battle.max_hp = 100;
    }
    world.actors.truncate(4);
    world.actors[3].battle.hp = 0;
    world.actors[3].battle.liveness = 0;
    world.battle_ctx.active_actor = acting;
    world.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
    world
}

fn pose_seat(world: &mut World) -> usize {
    for _ in 0..64 {
        world.live_battle_tick();
        if let Some(seq) = world.battle.victory {
            assert_eq!(seq.cause, BattleEndCause::MonsterWipe);
            return seq.pose_actor;
        }
    }
    panic!("the wiped monster side never raised the victory");
}

#[test]
fn the_member_who_finished_the_fight_poses() {
    let mut world = world_won_by(1);
    assert_eq!(pose_seat(&mut world), 1, "Noa's finishing blow, Noa's pose");
    let mut world = world_won_by(0);
    assert_eq!(pose_seat(&mut world), 0);
}

#[test]
fn a_downed_finisher_hands_the_pose_to_a_standing_member() {
    let mut world = world_won_by(2);
    world.actors[2].battle.hp = 0;
    world.actors[2].battle.liveness = 0;
    let seat = pose_seat(&mut world);
    assert!(
        seat < 2,
        "the re-pick lands on a standing member, got {seat}"
    );
}

#[test]
fn the_songi_formations_force_their_seat() {
    for (songi, seat) in [(0xB3u16, 2usize), (0xB4, 1)] {
        let mut world = world_won_by(0);
        world.battle.active_formation = Some(FormationDef::new(1, vec![FormationSlot::new(songi)]));
        assert_eq!(pose_seat(&mut world), seat, "formation {songi:#04x}");
    }
}
