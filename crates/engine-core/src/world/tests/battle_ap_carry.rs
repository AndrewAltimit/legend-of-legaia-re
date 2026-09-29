//! The Spirit (AP) gauge carries between fights through the character
//! record, as retail's does: the party loader `FUN_80053CB8` seeds the
//! actor's `+0x170` from record `+0x10E` (`0x800542BC..0x800542C4`), and both
//! results arms of `FUN_8004E568` store it back (`0x8004F218..0x8004F220`,
//! `0x8004FC18..0x8004FC20`). The port opened every fight at 0, so no art
//! costing AP could fire until the gauge had been refilled in that battle.

use super::*;
use crate::monster_catalog::{FormationDef, FormationSlot};

fn world_with_saved_ap(ap: u16) -> World {
    let mut party = legaia_save::Party::zeroed(3);
    for member in &mut party.members {
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = 100;
        hms.hp_max = 100;
        hms.sp_cur = ap;
        hms.sp_max = 100;
        member.set_hp_mp_sp(hms);
    }
    let mut world = World::new();
    world.load_party(party);
    world.party.party_count = 1;
    world.field_return = Some(FieldReturnState {
        actors: world.actors.clone(),
        player_actor_slot: world.player_actor_slot,
        party_count: world.party.party_count,
    });
    world.battle.return_mode = SceneMode::Field;
    world
}

#[test]
fn a_fight_opens_with_the_records_saved_ap() {
    let mut world = world_with_saved_ap(64);
    world.enter_battle_from_formation(&FormationDef::new(7, vec![FormationSlot::new(1)]));
    assert_eq!(world.actors[0].battle.spirit_gauge, 64);
}

#[test]
fn the_gauge_goes_back_to_the_record_at_the_battle_end() {
    let mut world = world_with_saved_ap(10);
    world.enter_battle_from_formation(&FormationDef::new(7, vec![FormationSlot::new(1)]));
    world.actors[0].battle.spirit_gauge = 73;
    world.finish_battle();
    assert_eq!(world.party.roster.members[0].hp_mp_sp().sp_cur, 73);
}
