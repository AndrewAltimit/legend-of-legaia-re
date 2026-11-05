//! The frame step a retail recording ran each arm of a summon module at.
//!
//! This table is a measurement, not game data, and it does not model the cause.
//! The frame driver `FUN_80016B6C` sets `DAT_1F800393` from the slowest of the
//! last sixteen frames, capped at `4`
//! (`docs/subsystems/battle-stage-camera.md`, "The frame step"), and nothing on
//! the disc says what a frame costs. Each entry is the step one recording
//! (`scripts/ci/cast-sequence.py`, `meta.csv`'s `step`, the Gobu Gobu fight, the
//! value the arm spends the most vsyncs on) on the emulator's cost model. An arm
//! too short to read takes the arm before it; a `0` is an arm no recording has
//! reached.
//!
//! What the reading is good for and where it stops is on that page. In short, a
//! whole-screen arm reads the same in every fight, an arm that shows the stage
//! does not, and the step moves inside an arm as the ring empties.
//!
//! The capture band has its own table, [`CAPTURE_ARM_STEPS`].
//!
//! The step matters beyond the camera's rate: a record an arm seats every pass
//! seats half as many at `4` as at `2`.

/// `(PROT entry, step by module phase)`.
const ARM_STEPS: &[(u32, &[u8])] = &[
    // gimard
    (903, &[0, 0, 1, 1, 4, 4, 1, 1, 4, 4, 4, 4, 4]),
    // theeder
    (904, &[0, 0, 1, 1, 1, 4, 3, 3, 3, 4, 4, 3, 4, 4, 4]),
    // vera
    (905, &[0, 0, 2, 2, 4, 4, 4, 4, 4, 4, 4]),
    // gizam
    (906, &[0, 0, 0, 1, 1, 4, 4, 3, 3, 4, 4, 4, 4, 4, 2]),
    // nighto
    (907, &[0, 0, 0, 1, 1, 4, 3, 4, 3, 3, 3, 4, 4, 4, 4, 4]),
    // zenoir
    (908, &[0, 0, 0, 1, 1, 4, 4, 4, 4, 4, 4, 4, 4]),
    // viguro
    (909, &[0, 0, 1, 1, 2, 2, 2, 2, 2, 2, 2, 4, 4, 4, 4]),
    // swordie
    (910, &[0, 0, 1, 4, 4, 4, 3, 4, 4, 4, 4]),
    // orb
    (911, &[0, 0, 1, 4, 4, 3, 3, 3, 3, 3, 4]),
    // freed
    (
        912,
        &[0, 0, 1, 4, 4, 4, 4, 4, 4, 4, 3, 4, 4, 4, 4, 3, 4, 4, 2, 3],
    ),
    // nova
    (
        913,
        &[
            0, 0, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 2, 3,
        ],
    ),
    // gola
    (914, &[0, 0, 0, 1, 4, 4, 4, 4, 4, 3, 3, 3, 4, 4, 4]),
    // mushura
    (915, &[0, 0, 0, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 2]),
    // aluru
    (
        916,
        &[0, 0, 1, 1, 3, 4, 4, 4, 4, 3, 4, 4, 4, 4, 3, 3, 3, 3, 1, 4],
    ),
    // barra
    (917, &[0, 1, 1, 3, 2, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 3]),
    // kemaro
    (
        918,
        &[0, 1, 1, 1, 4, 4, 3, 2, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4],
    ),
    // spoon
    (919, &[0, 0, 1, 4, 3, 4, 4, 4, 4]),
    // slippery
    (920, &[0, 0, 1, 3, 3, 3, 4, 4, 4, 4, 4]),
    // iota
    (921, &[0, 0, 1, 2, 1, 4, 4, 4, 4, 4, 4, 4]),
    // puera
    (
        922,
        &[
            0, 0, 1, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 2,
        ],
    ),
    // gilium
    (
        923,
        &[
            0, 0, 1, 4, 4, 4, 4, 2, 3, 4, 3, 4, 3, 2, 2, 2, 3, 3, 3, 3, 4, 4, 2,
        ],
    ),
    // juggernaut
    (
        927,
        &[
            0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 3, 3, 2,
        ],
    ),
    // palma
    (
        928,
        &[0, 2, 3, 3, 2, 4, 4, 3, 4, 3, 3, 4, 4, 2, 4, 4, 4, 4, 2, 2],
    ),
    // mule
    (
        929,
        &[0, 1, 3, 3, 3, 4, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 3, 3],
    ),
    // horn
    (930, &[0, 0, 2, 3, 4, 4, 4, 4, 4, 4, 4, 2]),
    // jedo
    (
        931,
        &[
            0, 0, 1, 3, 3, 4, 4, 4, 4, 4, 3, 3, 2, 2, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4,
            4, 2, 1,
        ],
    ),
    // meta
    (
        932,
        &[
            0, 1, 1, 1, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 3, 4, 3, 3, 3, 3, 3,
        ],
    ),
    // terra
    (
        933,
        &[
            0, 1, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2, 2, 2, 2,
        ],
    ),
    // ozma
    (
        934,
        &[
            0, 0, 1, 2, 2, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 3,
        ],
    ),
];

/// The step retail runs PROT `entry`'s arm `phase` at, `None` when no
/// recording says (a module the table does not hold, or an arm it passed
/// through inside a frame).
pub fn cast_arm_frame_step(entry: u32, phase: u8) -> Option<u8> {
    let (_, steps) = ARM_STEPS.iter().find(|(e, _)| *e == entry)?;
    steps.get(usize::from(phase)).copied().filter(|&s| s != 0)
}

/// `(action id, (module phase, step))`, the capture band's reading of the same
/// measurement: a harvest state's cast (`*_pre_*`, `scripts/ci/cast-sequence.py`)
/// with the step each arm spends the most vsyncs on in state `0x70`, merged over
/// the fights a spell was recorded in, an arm under eight vsyncs left out as too short to
/// read. Keyed on the action, since one module's
/// bodies differ (PROT 0941, 0955). A phase no recording reached takes the default.
const CAPTURE_ARM_STEPS: &[(u8, &[(u8, u8)])] = &[
    // earthquake
    (0x4A, &[(1, 4), (2, 4), (3, 4), (4, 4), (5, 4), (6, 3)]),
    // spore_gas
    (0x4F, &[(1, 4), (2, 4), (3, 4), (4, 4), (5, 2)]),
    // power_up
    (0x52, &[(1, 4), (2, 4), (3, 3)]),
    // big_wave
    (0x56, &[(5, 4), (6, 4), (7, 3), (8, 4)]),
    // v_windhash
    (0x57, &[(1, 4), (2, 4), (3, 4), (4, 4), (5, 3)]),
    // cross_beam
    (0x58, &[(0, 3), (1, 4), (2, 4), (3, 4), (4, 4), (5, 3)]),
    // rolling_flare
    (0x5A, &[(3, 4), (4, 4)]),
    // scythe_wind
    (0x5B, &[(0, 3), (1, 3), (2, 3), (3, 4), (4, 4), (5, 3)]),
    // terror_scream
    (0x70, &[(1, 4), (2, 4), (3, 4), (4, 4)]),
    // void_accessories
    (0x73, &[(1, 4), (2, 4), (3, 4), (4, 4)]),
    // element_change
    (0xAF, &[(1, 4), (2, 3)]),
    // rogue_wind
    (
        0xB0,
        &[
            (1, 4),
            (50, 4),
            (51, 3),
            (52, 4),
            (53, 4),
            (54, 4),
            (55, 4),
            (56, 1),
        ],
    ),
    // rogue_thunder
    (
        0xB1,
        &[
            (1, 4),
            (100, 3),
            (101, 3),
            (102, 3),
            (103, 3),
            (104, 4),
            (105, 4),
            (106, 1),
        ],
    ),
    // rogue_flame
    (
        0xB2,
        &[
            (1, 4),
            (2, 3),
            (3, 3),
            (4, 4),
            (5, 4),
            (6, 4),
            (7, 4),
            (8, 1),
        ],
    ),
    // stone_circle
    (0xB9, &[(1, 4), (2, 4), (3, 4), (4, 3)]),
];

/// The step retail ran module phase `phase` of the capture cast `spell` at.
pub fn capture_arm_frame_step(spell: u8, phase: u8) -> Option<u8> {
    let (_, steps) = CAPTURE_ARM_STEPS.iter().find(|(s, _)| *s == spell)?;
    steps.iter().find(|(p, _)| *p == phase).map(|&(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_screen_arm_reads_four_and_the_band_opening_one() {
        assert_eq!(cast_arm_frame_step(921, 2), Some(1));
        assert_eq!(cast_arm_frame_step(921, 5), Some(4));
        assert_eq!(cast_arm_frame_step(921, 200), None);
        assert_eq!(cast_arm_frame_step(960, 1), None);
    }

    #[test]
    fn the_capture_band_reads_by_action_and_arm() {
        // V-Windhash's blade arms: a trail seated per pass seats half as many
        // at `4` as at the default `2`.
        assert_eq!(capture_arm_frame_step(0x57, 3), Some(4));
        assert_eq!(capture_arm_frame_step(0x57, 200), None);
        assert_eq!(capture_arm_frame_step(0x01, 1), None);
    }
}
