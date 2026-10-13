//! Unit tests for the bar seeds.

use super::*;

#[test]
fn bar_seeds_round_trip_through_the_child_env() {
    let seeds = vec![
        BarSeed {
            slot: 0,
            hp: 412,
            mp: 37,
            ground: None,
            facing: None,
            defeat_lanes: None,
            status: 0,
            spent_key: false,
        },
        BarSeed {
            slot: 4,
            hp: 1,
            mp: 0,
            ground: Some([-4, -707]),
            facing: Some(0x9F0),
            defeat_lanes: Some(0),
            status: 0,
            spent_key: false,
        },
        BarSeed {
            slot: 2,
            hp: 330,
            mp: 0,
            ground: Some([10, 20]),
            facing: None,
            defeat_lanes: None,
            status: 0x1,
            spent_key: true,
        },
        BarSeed {
            slot: 3,
            hp: 9,
            mp: 1,
            ground: None,
            facing: None,
            defeat_lanes: Some(0xd0d),
            status: 0x1C10,
            spent_key: true,
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
