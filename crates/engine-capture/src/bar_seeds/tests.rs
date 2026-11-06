//! Unit tests for the bar seeds.

use super::*;

#[test]
fn bar_seeds_round_trip_through_the_child_env() {
    let seeds = vec![
        BarSeed {
            slot: 0,
            hp: 412,
            mp: 37,
            hp_max: None,
            mp_max: None,
            ground: None,
            facing: None,
            defeat_lanes: None,
            status: 0,
            init_key: None,
            heal_latch: false,
            gauge: None,
        },
        BarSeed {
            slot: 4,
            hp: 1,
            mp: 0,
            hp_max: None,
            mp_max: None,
            ground: Some([-4, -707]),
            facing: Some(0x9F0),
            defeat_lanes: Some(0),
            status: 0,
            init_key: None,
            heal_latch: false,
            gauge: None,
        },
        BarSeed {
            slot: 2,
            hp: 330,
            mp: 0,
            hp_max: Some(9999),
            mp_max: Some(0),
            ground: Some([10, 20]),
            facing: None,
            defeat_lanes: None,
            status: 0x1,
            init_key: Some(0),
            heal_latch: false,
            gauge: None,
        },
        BarSeed {
            slot: 3,
            hp: 9,
            mp: 1,
            hp_max: None,
            mp_max: None,
            ground: None,
            facing: None,
            defeat_lanes: Some(0xd0d),
            status: 0x1C10,
            init_key: Some(154),
            heal_latch: false,
            gauge: None,
        },
    ];
    assert_eq!(bar_seeds_from_env(&bar_seeds_to_env(&seeds)), seeds);
    assert!(bar_seeds_from_env("").is_empty());
}

#[test]
fn only_the_tracked_ailments_seed() {
    // Rage's delegation group stays behind; Venom and a Rot limb pass.
    assert_eq!(seedable_status_bits(0x0380 | 0x0001 | 0x0010), 0x0011);
    assert_eq!(seedable_status_bits(0x0380), 0);
}

/// An empty-seat seed rides the child env and stands the seat's struct, not
/// an actor.
#[test]
fn an_empty_seat_seed_stands_the_seat() {
    let seed = BarSeed {
        slot: EMPTY_SEAT_SLOT_BASE + 2,
        hp: 0,
        mp: 0,
        hp_max: None,
        mp_max: None,
        ground: Some([-82, 543]),
        facing: None,
        defeat_lanes: None,
        status: 0,
        init_key: None,
        heal_latch: false,
        gauge: None,
    };
    let back = bar_seeds_from_env(&bar_seeds_to_env(&[seed]));
    assert_eq!(back, vec![seed]);
    let mut world = legaia_engine_core::world::World::new();
    world.enter_battle(1, 1);
    let before = world.actors[2].move_state.world_x;
    apply_bar_seeds(&mut world, &back);
    assert_eq!(world.battle_pool_position(2), Some((-82, 543)));
    assert_eq!(world.actors[2].move_state.world_x, before);
}

/// Ground in a seed means the captured formation already holds its seed
/// recentre; bars alone do not.
#[test]
fn a_ground_seed_holds_the_seed_recentre() {
    let mut world = legaia_engine_core::world::World::new();
    world.enter_battle(1, 1);
    let bars = BarSeed {
        slot: 0,
        hp: 10,
        mp: 0,
        hp_max: None,
        mp_max: None,
        ground: None,
        facing: None,
        defeat_lanes: None,
        status: 0,
        init_key: None,
        heal_latch: false,
        gauge: None,
    };
    apply_bar_seeds(&mut world, &[bars]);
    assert!(!world.battle.seed_recentre_held);
    apply_bar_seeds(
        &mut world,
        &[BarSeed {
            ground: Some([0, 400]),
            ..bars
        }],
    );
    assert!(world.battle.seed_recentre_held);
}

#[test]
fn the_heal_latch_rides_the_bars_entry() {
    let seed = BarSeed {
        slot: 2,
        hp: 832,
        mp: 50,
        hp_max: None,
        mp_max: None,
        ground: Some([58, 316]),
        facing: None,
        defeat_lanes: None,
        status: 0,
        init_key: Some(300),
        heal_latch: true,
        gauge: None,
    };
    let env = bar_seeds_to_env(&[seed]);
    assert!(env.ends_with(":k300:v"), "{env}");
    assert_eq!(bar_seeds_from_env(&env), vec![seed]);
}

#[test]
fn a_raised_pool_seats_and_round_trips() {
    let seed = BarSeed {
        slot: 0,
        hp: 9973,
        mp: 79,
        hp_max: Some(9999),
        mp_max: None,
        ground: None,
        facing: None,
        defeat_lanes: None,
        status: 0,
        init_key: Some(0),
        heal_latch: false,
        gauge: None,
    };
    let env = bar_seeds_to_env(&[seed]);
    assert_eq!(env, "0:9973:79:k:m9999");
    assert_eq!(bar_seeds_from_env(&env), vec![seed]);
}
