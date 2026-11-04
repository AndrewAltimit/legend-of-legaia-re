//! The frame step retail runs each arm of a summon module at.
//!
//! The frame driver `FUN_80016B6C` sets `DAT_1F800393` from the slowest of
//! the last sixteen frames, capped at `4`
//! (`docs/subsystems/battle-stage-camera.md`, "The frame step"). Nothing in
//! the game state says what a frame costs, but a module's arm draws the same
//! screen each time it runs, so the reading is a property of the arm: every
//! arm of a whole-screen effect reads `4`, the band's opening arms `1`.
//!
//! This table is that reading, per entry and arm, off a retail recording of
//! the cast (`scripts/ci/cast-sequence.py`, `meta.csv`'s `step` per game
//! frame, the arm's most common step weighted by the time it spends there).
//! A `0` is an arm the recording passed through in no whole frame.
//!
//! The step matters beyond the camera's rate: a scroller that fires on a
//! countdown moves `step` rows a firing, so its speed in rows a vsync depends
//! on the step, and a record seated every pass seats half as many at `4` as at
//! `2`.

/// `(PROT entry, step by module phase)`.
const ARM_STEPS: &[(u32, &[u8])] = &[
    // gimard
    (903, &[0, 1, 1, 1, 4, 4, 4, 4, 4, 4, 4, 4, 3]),
    // theeder
    (904, &[0, 1, 1, 1, 3, 4, 3, 3, 3, 4, 4, 3, 4, 4, 3]),
    // vera
    (905, &[0, 1, 2, 2, 4, 4, 0, 0, 4, 4, 4]),
    // gizam
    (906, &[0, 1, 1, 1, 1, 4, 4, 3, 4, 4, 4, 3, 4, 4, 2]),
    // nighto
    (907, &[0, 1, 1, 1, 1, 4, 3, 4, 3, 3, 3, 4, 4, 4, 0, 4]),
    // zenoir
    (908, &[0, 1, 1, 1, 1, 4, 4, 4, 4, 4, 4, 4, 4]),
    // viguro
    (909, &[0, 1, 1, 1, 2, 2, 2, 2, 2, 2, 4, 4, 4, 4, 4]),
    // swordie
    (910, &[0, 1, 1, 4, 4, 4, 4, 4, 4, 4, 4]),
    // orb
    (911, &[0, 1, 1, 4, 4, 3, 0, 0, 0, 3, 4]),
    // freed
    (
        912,
        &[0, 1, 1, 4, 4, 4, 4, 4, 4, 4, 3, 4, 4, 4, 4, 3, 4, 4, 2, 3],
    ),
    // nova
    (
        913,
        &[
            0, 1, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 2, 2, 2, 3,
        ],
    ),
    // gola
    (914, &[0, 1, 1, 1, 4, 4, 4, 4, 4, 3, 3, 3, 4, 4, 4]),
    // mushura
    (915, &[0, 1, 1, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 2]),
    // aluru
    (
        916,
        &[0, 1, 1, 1, 3, 4, 4, 4, 4, 3, 4, 4, 4, 4, 4, 3, 3, 3, 1, 4],
    ),
    // barra
    (917, &[0, 1, 1, 3, 2, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 3]),
    // kemaro
    (
        918,
        &[0, 1, 1, 1, 4, 4, 3, 2, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4],
    ),
    // spoon
    (919, &[0, 1, 1, 4, 3, 4, 4, 4, 4]),
    // slippery
    (920, &[0, 1, 1, 3, 3, 3, 4, 4, 4, 4, 4]),
    // iota
    (921, &[0, 1, 1, 2, 4, 4, 4, 4, 4, 4, 4, 3]),
    // puera
    (
        922,
        &[
            0, 1, 1, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 0, 4, 4, 2,
        ],
    ),
    // gilium
    (
        923,
        &[
            0, 1, 1, 4, 4, 4, 4, 2, 3, 4, 3, 4, 3, 2, 2, 2, 3, 3, 3, 3, 4, 4, 2,
        ],
    ),
    // juggernaut
    (
        927,
        &[
            0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 0, 4, 4, 4, 3, 3, 2,
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
        &[0, 1, 3, 2, 3, 4, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 3, 3],
    ),
    // horn
    (930, &[0, 1, 2, 3, 4, 4, 4, 4, 4, 4, 4, 2]),
    // jedo
    (
        931,
        &[
            0, 1, 1, 3, 3, 4, 4, 4, 4, 4, 3, 3, 2, 2, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4,
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
            0, 1, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 3, 2, 2, 2, 2, 2, 2, 2, 2,
        ],
    ),
    // ozma
    (
        934,
        &[
            0, 1, 1, 2, 2, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 3, 4, 4, 4, 3,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_screen_arm_reads_four_and_the_band_opening_one() {
        assert_eq!(cast_arm_frame_step(921, 1), Some(1));
        assert_eq!(cast_arm_frame_step(921, 5), Some(4));
        assert_eq!(cast_arm_frame_step(921, 200), None);
        assert_eq!(cast_arm_frame_step(960, 1), None);
    }
}
