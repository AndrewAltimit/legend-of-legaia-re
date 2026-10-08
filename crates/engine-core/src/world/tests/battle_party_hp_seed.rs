//! A fight seats each member's HP / MP from its character record, as
//! retail's party loader `FUN_80053CB8` does (`0x80053D8C..0x80053E10`:
//! record `+0x6CE` / `+0x6CC` / `+0x6D2` onto actor `+0x14C` / `+0x14E` /
//! `+0x150`). The port read the field actor slots, which a scene's script
//! can reset to blank actors (`opurud` blanks slots 1 / 2), so a fight
//! entered there seated Noa and Gala at 0 HP.

use super::*;
use crate::monster_catalog::{FormationDef, FormationSlot};

fn party_world() -> World {
    let mut party = legaia_save::Party::zeroed(3);
    for (i, member) in party.members.iter_mut().enumerate() {
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = 80 + i as u16;
        hms.hp_max = 120;
        hms.mp_cur = 30 + i as u16;
        hms.mp_max = 40;
        member.set_hp_mp_sp(hms);
    }
    let mut world = World::new();
    world.load_party(party);
    world.party.party_count = 3;
    world.field_return = Some(FieldReturnState {
        actors: world.actors.clone(),
        player_actor_slot: world.player_actor_slot,
        party_count: world.party.party_count,
    });
    world.battle.return_mode = SceneMode::Field;
    world
}

#[test]
fn blanked_field_actors_do_not_seat_the_party_at_zero_hp() {
    let mut world = party_world();
    // The scene's script resets actor slots 1 and 2 to blank actors.
    for slot in 1..3 {
        world.actors[slot].battle.hp = 0;
        world.actors[slot].battle.max_hp = 0;
        world.actors[slot].battle.mp = 0;
    }
    world.enter_battle_from_formation(&FormationDef::new(7, vec![FormationSlot::new(1)]));
    for slot in 0..3 {
        let b = &world.actors[slot].battle;
        let want = (80 + slot as u16, 120, 30 + slot as u16);
        assert_eq!((b.hp, b.max_hp, b.mp), want, "member {slot}");
    }
}

#[test]
fn a_member_without_a_filled_record_keeps_its_actor_values() {
    let mut world = World::new();
    world.load_party(legaia_save::Party::zeroed(1));
    world.party.party_count = 1;
    world.actors[0].battle.hp = 55;
    world.actors[0].battle.max_hp = 60;
    world.enter_battle_from_formation(&FormationDef::new(7, vec![FormationSlot::new(1)]));
    assert_eq!(world.actors[0].battle.hp, 55);
}
