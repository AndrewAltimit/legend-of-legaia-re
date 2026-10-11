//! Unit tests for the field seeds' environment forms.

use super::*;

/// The walker seed survives its env form.
#[test]
fn walker_seeds_round_trip_through_their_env_form() {
    let w = vec![
        WalkerSeed {
            flat: 41,
            x: 3456,
            z: -12,
            heading: 0xC00,
        },
        WalkerSeed {
            flat: 7,
            x: 0,
            z: 16320,
            heading: 0,
        },
    ];
    assert_eq!(walkers_from_env(&walkers_env(&w)), w);
}

/// The morph-envelope seed survives its env form.
#[test]
fn morph_seeds_round_trip_through_their_env_form() {
    let m = vec![
        MorphSeed {
            flat: 7,
            weights: vec![0x1000, 0x0C5A, 0],
            done_mask: 0x8000_0003,
            env: 0x5015,
        },
        MorphSeed {
            flat: 0x45,
            weights: vec![0x10],
            done_mask: 0,
            env: 0x1000,
        },
    ];
    assert_eq!(morphs_from_env(&morphs_env(&m)), m);
}

#[test]
fn sprite_arm_seeds_round_trip_through_their_env_form() {
    let s = vec![legaia_engine_core::world::ambient::SpriteArmSeed {
        record_off: 0x728,
        pos: [4224, -300, 9294],
        rot: [0, 0x7C0, -3],
        scale: 0x1000,
        colour: 0xC900_0000,
        level: 0x9B3,
        pose: vec![
            [1, 2, 3, 4, 5, 6, 7, 8],
            [0xF0, 0, 0x1F, 0, 0, 0x80, 0x40, 0x80],
        ],
    }];
    assert_eq!(sprite_arms_from_env(&sprite_arms_env(&s)), s);
}

#[test]
fn object_clip_seeds_round_trip_the_env() {
    let clips = vec![
        ObjectClipSeed {
            record: 0,
            clip: 2,
            cursor: 95,
            flags: 0x0108,
            rate: 8,
        },
        ObjectClipSeed {
            record: 17,
            clip: 1,
            cursor: -3,
            flags: 0x0015,
            rate: 16,
        },
    ];
    assert_eq!(object_clips_from_env(&object_clips_env(&clips)), clips);
    assert_eq!(object_clips_from_env("1:2:3;junk;4:5:6:7:8").len(), 1);
}
