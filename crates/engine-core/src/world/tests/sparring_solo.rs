//! The Tetsu spar seats Vahn alone whatever the field party holds - the
//! engine rule `World::enter_battle_from_formation` carries, restored on the
//! return to the field.

use super::*;
use crate::battle_tutorial::TUTORIAL_ARM_FLAG;
use crate::monster_catalog::{FormationDef, FormationSlot};
use vm::battle_action::BattleEndCause;

fn full_party_world() -> World {
    let mut world = World {
        party: crate::world::PartyState {
            party_count: 3,
            active_party: vec![1, 0, 2],
            ..Default::default()
        },
        ..World::default()
    };
    for a in world.actors.iter_mut().take(3) {
        a.battle.hp = 100;
    }
    world
}

fn enter(world: &mut World) {
    world.field_return = Some(FieldReturnState {
        actors: world.actors.clone(),
        player_actor_slot: world.player_actor_slot,
        party_count: world.party.party_count,
    });
    world.battle.return_mode = SceneMode::Field;
    world.enter_battle_from_formation(&FormationDef::new(4, vec![FormationSlot::new(79)]));
}

#[test]
fn the_disc_armed_spar_seats_vahn_alone_and_hands_the_party_back() {
    let mut world = full_party_world();
    world.system_flag_set(TUTORIAL_ARM_FLAG);
    enter(&mut world);
    assert_eq!(
        world.battle.stage_id,
        crate::battle_tutorial::TUTORIAL_STAGE_ID
    );
    assert_eq!(world.party.party_count, 1, "Vahn alone");
    assert_eq!(
        world.party_roster_slot(0),
        0,
        "the one seat is Vahn's record"
    );
    assert_eq!(
        world.actors[1].battle_monster_id,
        Some(79),
        "Tetsu sits right behind"
    );

    world.battle.end = Some(BattleEndCause::MonsterWipe);
    world.begin_battle_end_sequence();
    let mut frames = 0;
    while world.battle.victory.is_some() && frames < 2000 {
        world.tick_battle_end_sequence();
        frames += 1;
    }
    assert_eq!(world.mode, SceneMode::Field);
    assert_eq!(world.party.party_count, 3, "the field party comes back");
    assert_eq!(world.party.active_party, vec![1, 0, 2]);
    assert!(world.battle.solo_spar_restore.is_none());
}

#[test]
fn an_ordinary_fight_keeps_the_full_party() {
    let mut world = full_party_world();
    world.system_flag_clear(TUTORIAL_ARM_FLAG);
    enter(&mut world);
    assert_eq!(world.party.party_count, 3);
    assert_eq!(world.party.active_party, vec![1, 0, 2]);
    assert!(world.battle.solo_spar_restore.is_none());
}
