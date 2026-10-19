//! The battle-intro labels read the formation's seats: the enemy target rows
//! sit over the monster actors the round-start formation placed
//! (`FUN_801DB318`, flow case `0`). The squash + recentre itself is
//! engine-core's `world::tests::battle_formation_span`.

use legaia_engine_core::world::*;

fn seat_monsters(world: &mut World, party: u8, ids: &[u16]) {
    use legaia_engine_battle::monster_catalog::MonsterDef;
    world.enter_battle(party, ids.len() as u8);
    for (k, &id) in ids.iter().enumerate() {
        if world.tables.monster_catalog.get(id).is_none() {
            world
                .tables
                .monster_catalog
                .insert(MonsterDef::new(id, format!("M{id}"), 40, 5));
        }
        let a = &mut world.actors[usize::from(party) + k];
        a.battle_monster_id = Some(id);
        a.battle.hp = 40;
        a.battle.max_hp = 40;
    }
}

/// The intro label for a monster group sits over the group's seat:
/// `(x >> 3) - width / 2 + 0xA0`, off the monster actor's world X.
#[test]
fn intro_labels_sit_over_their_monsters() {
    let mut world = World::new();
    seat_monsters(&mut world, 3, &[4, 7, 9]);
    let rows = legaia_engine_battle_hud::battle_enemy_target_rows(&world);
    let xs: Vec<i16> = rows.iter().map(|r| r.x).collect();
    // The three normal-family seats: -600, 0, 600.
    assert_eq!(xs, vec![-600, 0, 600]);
    let mut rows = rows;
    legaia_engine_battle::target_picker::layout_enemy_menu_rows(&mut rows, |_| 0);
    let placed: Vec<i16> = rows.iter().map(|r| r.x).collect();
    assert_eq!(placed, vec![0xA0 - 75, 0xA0, 0xA0 + 75]);
}
