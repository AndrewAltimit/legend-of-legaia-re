//! The round-start formation squash + recentre (`FUN_801DB318`, flow case
//! `0`) over the engine's compacted actor table, and the intro labels that
//! read the same positions.

use super::*;

fn seat_monsters(world: &mut World, party: u8, ids: &[u16]) {
    use crate::monster_catalog::MonsterDef;
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

fn zs(world: &World, n: usize) -> Vec<i16> {
    (0..n).map(|i| world.actors[i].move_state.world_z).collect()
}

/// Three on one spans `z = -825 ..= 800`; the centroid `(-25 as u32) >> 1`
/// truncates to `-13`, so every combatant moves `+13` - the offset every
/// catalogued three-on-one capture reads (party `-812` / `-762`, monster
/// `813`).
#[test]
fn three_on_one_recentres_by_thirteen() {
    let mut world = World::new();
    seat_monsters(&mut world, 3, &[4]);
    assert_eq!(zs(&world, 4), vec![-825, -775, -775, 800]);
    world.normalize_battle_formation();
    assert_eq!(zs(&world, 4), vec![-812, -762, -762, 813]);
    // X is already centred.
    let xs: Vec<i16> = (0..4).map(|i| world.actors[i].move_state.world_x).collect();
    assert_eq!(xs, vec![0, 600, -600, 0]);
}

/// A balanced formation does not move, and a solo party's two absent retail
/// slots - modelled at the origin - leave the extents alone.
#[test]
fn balanced_and_solo_formations_hold_still() {
    let mut world = World::new();
    seat_monsters(&mut world, 3, &[12, 12, 12]);
    let before = zs(&world, 6);
    world.normalize_battle_formation();
    assert_eq!(zs(&world, 6), before);

    let mut world = World::new();
    seat_monsters(&mut world, 1, &[79]);
    world.normalize_battle_formation();
    assert_eq!(zs(&world, 2), vec![-800, 800]);
}

/// Monsters sit in retail pool slots `3..7` whatever the party size: a solo
/// party's first monster is engine slot 1 and retail slot 3.
#[test]
fn engine_monster_slots_map_onto_retail_pool_slots() {
    let mut world = World::new();
    seat_monsters(&mut world, 1, &[4, 7]);
    assert_eq!(world.retail_battle_pool_slot(0), Some(0));
    assert_eq!(world.retail_battle_pool_slot(1), Some(3));
    assert_eq!(world.retail_battle_pool_slot(2), Some(4));
    assert_eq!(world.retail_battle_pool_slot(3), None);
}

/// A dead monster is outside the walk (`+0x14C == 0`): it neither sets the
/// extents nor moves.
#[test]
fn a_dead_monster_is_left_where_it_fell() {
    let mut world = World::new();
    seat_monsters(&mut world, 1, &[4, 7]);
    world.actors[2].move_state.world_z = 3000;
    world.actors[2].battle.liveness = 0;
    world.normalize_battle_formation();
    assert_eq!(world.actors[2].move_state.world_z, 3000);
    assert_eq!(zs(&world, 2), vec![-800, 800]);
}

/// A formation wider than `0x800` is squashed back to that span, then
/// recentred.
#[test]
fn a_wandered_formation_is_squashed_to_the_frame() {
    let mut world = World::new();
    seat_monsters(&mut world, 1, &[4]);
    world.actors[0].move_state.world_z = -2000;
    world.actors[1].move_state.world_z = 2000;
    world.normalize_battle_formation();
    let z = zs(&world, 2);
    assert!(z[1] - z[0] <= 0x800, "{z:?}");
    assert_eq!(z[0] + z[1], 0, "recentred: {z:?}");
}

/// The intro label for a monster group sits over the group's seat:
/// `(x >> 3) - width / 2 + 0xA0`, off the monster actor's world X.
#[test]
fn intro_labels_sit_over_their_monsters() {
    let mut world = World::new();
    seat_monsters(&mut world, 3, &[4, 7, 9]);
    let rows = crate::battle_hud::battle_enemy_target_rows(&world);
    let xs: Vec<i16> = rows.iter().map(|r| r.x).collect();
    // The three normal-family seats: -600, 0, 600.
    assert_eq!(xs, vec![-600, 0, 600]);
    let mut rows = rows;
    crate::target_picker::layout_enemy_menu_rows(&mut rows, |_| 0);
    let placed: Vec<i16> = rows.iter().map(|r| r.x).collect();
    assert_eq!(placed, vec![0xA0 - 75, 0xA0, 0xA0 + 75]);
}

/// A scripted fight seats its monsters on the alternate family: three
/// monsters read `(0, 900) (-600, 700) (600, 700)` instead of the normal
/// row.
#[test]
fn a_scripted_fight_takes_the_alternate_seat_family() {
    let mut world = World::new();
    seat_monsters(&mut world, 3, &[4, 7, 9]);
    world.seat_scripted_monster_family(3);
    let seats: Vec<(i16, i16)> = (3..6)
        .map(|i| {
            (
                world.actors[i].move_state.world_x,
                world.actors[i].move_state.world_z,
            )
        })
        .collect();
    assert_eq!(seats, vec![(0, 900), (-600, 700), (600, 700)]);
}

/// The seat pair moves with the live pair, so the range reference stays on
/// the body it names.
#[test]
fn the_seat_pair_moves_with_the_recentre() {
    let mut world = World::new();
    seat_monsters(&mut world, 3, &[4]);
    for a in world.actors.iter_mut().take(4) {
        a.battle.seat = Some((a.move_state.world_x, a.move_state.world_z - 62));
    }
    world.normalize_battle_formation();
    assert_eq!(world.actors[0].battle.seat, Some((0, -874)));
    assert_eq!(world.actors[3].battle.seat, Some((0, 813 - 62)));
}
