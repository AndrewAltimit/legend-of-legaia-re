//! PROT 0924 (Ultimate Rave, the Lippian flute): the holds of
//! [`summon_holds::ULTIMATE_RAVE`] with the body's camera, captions, staged
//! clips and spawns.
//!
//! See `ghidra/scripts/funcs/overlay_stager_ultimate_rave_0924_801f6a18.txt`.

use super::body_kit::{caption, drift, with_drift};
use super::body_paced::shot;
use super::summon_holds::{ULTIMATE_RAVE, hold_step};
use super::*;

const fn rave_spawn(va: u32, anchor: SpawnAnchor) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(va),
        anchor,
    }
}

const ORIGIN: SpawnAnchor = SpawnAnchor::LiteralScaled {
    pos: [0; 3],
    rot: [0; 3],
    scale: 0x1000,
};

/// Arm 5's pass: the drop on the creature (`0x801F7744`).
const RAVE_DROP: [ModuleSpawn; 1] = [rave_spawn(0x801F_787C, SpawnAnchor::Creature)];
/// Arm 6's pass: the record its handle word `0x801F7C58` keeps for arm 8's kill.
const RAVE_BURST: [ModuleSpawn; 1] = [rave_spawn(0x801F_78E4, ORIGIN)];
/// Arm 9's pass, when the cast is not the one-in-128 kill.
const RAVE_FINISH: [ModuleSpawn; 1] = [rave_spawn(0x801F_7BE8, ORIGIN)];

/// The module's image string "Ultimate Rave" arm 9 prints (`0x801F69D8`).
const RAVE_LINE: u32 = 0x801F_69D8;

/// The shot arms 3 and 4 hold over the word `w`: pitch `2w - 0x40`, yaw
/// `0x200 - 8w`, TR y `0x500 - 5w`, TR z `0x800 + 0x20w`, re-armed each pass.
fn sweep_shot(word: i32, focus_z: i16) -> ArmDirection {
    let w = word as i16;
    shot(
        [
            w.wrapping_mul(2).wrapping_sub(0x40),
            0x200i16.wrapping_sub(w.wrapping_mul(8)),
        ],
        [
            0,
            0x500i16.wrapping_sub(w.wrapping_mul(5)),
            0x800i16.wrapping_add(w.wrapping_mul(0x20)),
        ],
        [0, 0, focus_z],
        1,
    )
}

/// PROT 0924's director. `c` is the creature seat, `f` its heading, `v` the
/// victim, `w` the word, `D` the frame step.
///
/// | arm | every pass | on its pass |
/// |---:|---|---|
/// | 0 | - | cut: TR `(0, 0x5FC, 0)` |
/// | 1, 2 | while the word runs, TR z `+= 8D` | arm 2: the creature stands at `z = 0x2000`, facing `0x800`, rate `8` |
/// | 3 | TR z `+= 8D` | the creature at `z = 0x4000`; the sweep's first shot |
/// | 4 | the sweep's shot over `w` | the spell name |
/// | 5 | cut: pitch `-0x40`, yaw `0x200`, TR `(0, 0x500, 0x800)`, on `-c.z` | the attack name; clip `2` at rate `4`, the creature behind the victim by `sin(f) * r >> 13`; cut: yaw `0x800 - f`, TR `(0, 0x400, 0x800)` on `c`; the drop |
/// | 6 | pitch `+= D`, TR y `-= D`, TR z `+= D` | the burst; cut: pitch `-0x40`, yaw `0x900 - f`, TR `(0, 0x400, 0x400)` on `c`; clip down one at rate `2` |
/// | 7 | clip `2` bare, TR z `+= 4D` | cut: yaw `0x200 - f`, TR `(0, 0x300, 0x600)` on `c` |
/// | 8, 9 | TR z `+= 4D`, yaw `+= 2D` | arm 8: the rate `0`; arm 9: the line, the finish |
/// | 10 | - | cut: pitch `0x100`, yaw `-0x100 - f`, TR `(0, 0x300, 0x800)` on `c`; clip `3` at rate `4` |
/// | 11 | - | the caption cleared |
///
/// PORT: overlay_stager_ultimate_rave_0924_801f6a18 (PROT 0924; the camera arms, captions, creature clips and spawns; the one-in-128 kill's parts on arm 10 are not carried)
pub fn ultimate_rave_direct(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> ArmDirection {
    const D1: i32 = DELTA_PER_TICK;
    let word_ran = st.countdown.0 > 0;
    let word_in = st.countdown.0;
    let hold = hold_step(ULTIMATE_RAVE, st, phase);
    let c = st.creature_live.or(st.creature).unwrap_or_default();
    let f = c.facing;
    let dir = match phase {
        0 => shot([0, 0], [0, 0x5FC, 0], [0; 3], 1),
        1 | 2 => {
            if !hold && phase == 2 {
                st.creature = Some(ModuleSeat {
                    x: 0,
                    y: 0,
                    z: 0x2000,
                    facing: 0x800,
                });
                st.creature_rate = Some(8);
            }
            let d = word_ran.then(|| drift(0, 0, 0, 0, 8 * D1));
            with_drift(ArmDirection::PASS, d)
        }
        3 if hold => with_drift(ArmDirection::PASS, Some(drift(0, 0, 0, 0, 8 * D1))),
        3 => {
            st.creature = Some(ModuleSeat { z: 0x4000, ..c });
            sweep_shot(st.countdown.0, -0x4000)
        }
        4 if hold => sweep_shot(word_in, c.z.wrapping_neg()),
        4 => caption(
            sweep_shot(word_in, c.z.wrapping_neg()),
            ModuleCaption::SpellName,
        ),
        5 if hold => shot(
            [-0x40, 0x200],
            [0, 0x500, 0x800],
            [0, 0, c.z.wrapping_neg()],
            1,
        ),
        5 => {
            let v = seats.victim;
            let facing = v.facing.wrapping_add(0x800) & 0xFFF;
            let r = i32::from(seats.victim_radius);
            let (sin, cos) = trig12(facing);
            let (sin, cos) = (i32::from(sin), i32::from(cos));
            let seat = ModuleSeat {
                x: (i32::from(v.x) - (sin * r) / 0x2000) as i16,
                y: 0,
                z: (i32::from(v.z) - (cos * r) / 0x2000) as i16,
                facing,
            };
            st.creature = Some(seat);
            st.creature_stage = Some(2);
            st.creature_restage = true;
            st.creature_rate = Some(4);
            ArmDirection {
                spawns: &RAVE_DROP,
                ..caption(
                    shot(
                        [0, yaw_from(0x800, facing)],
                        [0, 0x400, 0x800],
                        focus_on(seat),
                        1,
                    ),
                    ModuleCaption::AttackName,
                )
            }
        }
        6 => {
            st.creature_rate = Some(if hold { 4 } else { 2 });
            let d = Some(drift(D1, 0, 0, -D1, D1));
            if hold {
                with_drift(ArmDirection::PASS, d)
            } else {
                st.creature_stage = Some(1);
                st.creature_restage = true;
                ArmDirection {
                    spawns: &RAVE_BURST,
                    ..with_drift(
                        shot(
                            [-0x40, yaw_from(0x900, f)],
                            [0, 0x400, 0x400],
                            focus_on(c),
                            1,
                        ),
                        d,
                    )
                }
            }
        }
        7 => {
            st.creature_stage = Some(2);
            st.creature_rate = Some(2);
            let d = Some(drift(0, 0, 0, 0, 4 * D1));
            if hold {
                with_drift(ArmDirection::PASS, d)
            } else {
                with_drift(
                    shot([0, yaw_from(0x200, f)], [0, 0x300, 0x600], focus_on(c), 1),
                    d,
                )
            }
        }
        8 | 9 => {
            let d = Some(drift(0, 2 * D1, 0, 0, 4 * D1));
            if !hold && phase == 8 {
                st.creature_rate = Some(0);
            }
            if !hold && phase == 9 {
                ArmDirection {
                    spawns: &RAVE_FINISH,
                    ..with_drift(
                        caption(ArmDirection::PASS, ModuleCaption::ModuleString(RAVE_LINE)),
                        d,
                    )
                }
            } else {
                with_drift(ArmDirection::PASS, d)
            }
        }
        10 if !hold => {
            st.creature_stage = Some(3);
            st.creature_restage = true;
            st.creature_rate = Some(4);
            shot(
                [0x100, yaw_from(-0x100, f)],
                [0, 0x300, 0x800],
                focus_on(c),
                1,
            )
        }
        11 if !hold => caption(ArmDirection::PASS, ModuleCaption::Cleared),
        _ => ArmDirection::PASS,
    };
    ArmDirection { hold, ..dir }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sweep's shot opens wide of the creature and closes on `w = 0`.
    #[test]
    fn the_sweep_closes_as_its_word_runs_out() {
        let d = sweep_shot(0, -0x4000);
        assert!(d.shot.is_some());
    }

    /// Arm 5's pass stands the creature behind the victim, facing away from it.
    #[test]
    fn arm_5_stands_the_creature_behind_the_victim() {
        let mut seats = ModuleCamSeats::default();
        seats.victim.facing = 0x800;
        seats.victim.z = 100;
        seats.victim_radius = 0x100;
        let mut st = ModuleCamState::default();
        let d = ultimate_rave_direct(&mut st, 5, seats);
        assert!(!d.hold);
        assert_eq!(st.creature.map(|c| c.facing), Some(0));
        assert_eq!(st.creature_stage, Some(2));
        assert_eq!(d.spawns.len(), 1);
    }
}
