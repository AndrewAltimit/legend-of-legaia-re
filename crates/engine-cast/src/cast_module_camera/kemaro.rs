//! PROT 0918 (Kemaro, `0x90`)'s director, beside its ported tick body
//! (`crate::cast_module_ticks::kemaro_tick`): the countdown holds of
//! [`super::summon_holds::KEMARO`] plus the camera, drift, spawn and caption
//! writes of the body `FUN_801F6C70`; its fades are the presentation hook's
//! (`cast_band::presentation`), decoded off the image.
//!
//! Every drift is the speed scalar `*(0x1F80037D)` times the frame step
//! `*(0x1F800393)` a battle frame, so `scalar * k` a tick here
//! ([`SPEED_SCALAR`]).
//!
//! Provenance: `ghidra/scripts/funcs/overlay_summon_kemaro_0918_801f6c70.txt`
//! (the image at slot-B base `0x801F69D8`; a `beq`/`slti` chain head from
//! `0x801F6D08`).

use super::summon_holds::{KEMARO, hold_step};
use super::*;

const fn rec(va: u32, anchor: SpawnAnchor) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(va),
        anchor,
    }
}

const fn lit(pos: [i16; 3], rot: [i16; 3]) -> SpawnAnchor {
    SpawnAnchor::Literal { pos, rot }
}

/// PROT 0918 arm 3's four calls on its pass (`0x801F6FA4..0x801F6FEC`), on
/// the stack trio with `-0xC00` stored over its `z` in the first call's
/// delay slot: the rock wall behind the creature's seat.
pub const KEMARO_ARM3_SPAWNS: [ModuleSpawn; 4] = [
    rec(0x801F_8B88, lit([0, 0, -0xC00], [0; 3])),
    rec(0x801F_8C90, lit([0, 0, -0xC00], [0; 3])),
    rec(0x801F_8CE4, lit([0, 0, -0xC00], [0; 3])),
    rec(0x801F_8D38, lit([0, 0, -0xC00], [0; 3])),
];

/// PROT 0918 arm 5's call on its pass (`0x801F7300`), on the creature.
pub const KEMARO_ARM5_SPAWNS: [ModuleSpawn; 1] = [rec(0x801F_8D84, SpawnAnchor::Creature)];

/// PROT 0918 arm 7's call on its pass (`0x801F74EC`): the zeroed stack
/// point on the creature's angle trio, which arm 6 left at `0` (its spin is
/// `+0x72 * 6`, and `+0x72` stops on `0x1000`).
pub const KEMARO_ARM7_SPAWNS: [ModuleSpawn; 1] = [rec(0x801F_8E88, lit([0; 3], [0; 3]))];

/// PROT 0918 arm 9's three calls on its pass (`0x801F787C..0x801F78BC`),
/// on the creature: the parts whose handles arms 13..17 walk.
pub const KEMARO_ARM9_SPAWNS: [ModuleSpawn; 3] = [
    rec(0x801F_8ED8, SpawnAnchor::Creature),
    rec(0x801F_8F48, SpawnAnchor::Creature),
    rec(0x801F_8FB8, SpawnAnchor::Creature),
];

/// PROT 0918 arm 13's three calls (`0x801F7D08..0x801F7D38`), made once on
/// the first held pass with the countdown at or below `scalar << 5` (the
/// latch `0x801F92F8`), on the creature.
pub const KEMARO_ARM13_SPAWNS: [ModuleSpawn; 3] = [
    rec(0x801F_9028, SpawnAnchor::Creature),
    rec(0x801F_907C, SpawnAnchor::Creature),
    rec(0x801F_90D0, SpawnAnchor::Creature),
];

/// PROT 0918 arm 17's five calls (`0x801F8608..0x801F8688`): two on the
/// victim, three on `(0, 0, 0x400)`, all on the angle trio `(0, 0xC00, 0)`
/// the arm's shot left with its pitch zeroed.
pub const KEMARO_ARM17_SPAWNS: [ModuleSpawn; 5] = [
    rec(0x801F_911C, SpawnAnchor::Victim),
    rec(0x801F_9160, SpawnAnchor::Victim),
    rec(0x801F_91A4, lit([0, 0, 0x400], [0, 0xC00, 0])),
    rec(0x801F_9204, lit([0, 0, 0x400], [0, 0xC00, 0])),
    rec(0x801F_9264, lit([0, 0, 0x400], [0, 0xC00, 0])),
];

/// A cut on the focus trio `(0, fy, fz)`.
fn cut(angles: [i16; 2], tr: [i16; 3], fy: i16, fz: i16, frames: u16) -> ModuleShot {
    ModuleShot {
        angles: [angles[0], angles[1], 0],
        tr,
        focus: [0, fy, fz],
        frames,
    }
}

fn drift(pitch: i32, yaw: i32, tr_y: i32, tr_z: i32) -> CaptureDrift {
    CaptureDrift {
        pitch: pitch as i16,
        yaw: yaw as i16,
        tr_y: tr_y as i16,
        tr_z: tr_z as i16,
        tr_x: 0,
    }
}

/// The drift arm `phase` writes on every pass, held or not; `drained` is
/// the countdown after this pass's drain (arm 4 drifts only while it is
/// above `scalar << 5`).
pub fn kemaro_drift(phase: u8, drained: i32) -> Option<CaptureDrift> {
    let p = SPEED_SCALAR;
    Some(match phase {
        // `0x801F6F04..0x801F6F10`: TR z `+ 2 * scalar * step`.
        1 => drift(0, 0, 0, 2 * p),
        // `0x801F70E8..0x801F7118`.
        4 if drained > p << 5 => drift(0, -(p / 4), 0, -2 * p),
        // `0x801F76DC..0x801F7728`, `0x801F77A0..0x801F77EC`.
        8 | 9 => drift(0, -(p / 2), 0, -(p / 8)),
        // `0x801F7930..0x801F79B4`.
        10 => drift(p / 8, p / 4, -(p / 2), 2 * p),
        // `0x801F7A14..0x801F7A80`.
        11 => drift(-(p / 8), p / 4, 0, p),
        // `0x801F7B0C..0x801F7B7C`.
        12 => drift(p / 4, -(p / 2), 0, 4 * p),
        // `0x801F7C10..0x801F7C20`.
        13 => drift(0, -(p / 8), 0, 0),
        _ => return None,
    })
}

/// PROT 0918 (Kemaro).
///
/// | arm | on pass |
/// |---|---|
/// | 0 (`0x801F6E9C`) | cut pitch `0`, yaw `0x800`, TR `(0, 0x600, 0x800)`, focus z `0x400` |
/// | 3 | [`KEMARO_ARM3_SPAWNS`] |
/// | 4 (`0x801F71D8`) | a `0x80`-frame shot pitch `-0xC0`, yaw `0x800`, TR `(0, 0x1900, -0x1200)`, focus z `0x400` |
/// | 5 | [`KEMARO_ARM5_SPAWNS`] |
/// | 6 (`0x801F7448`) | the spell name, once the creature's scale reaches `0x1000` |
/// | 7 (`0x801F754C`, `0x801F7590`) | [`KEMARO_ARM7_SPAWNS`], the attack-record name; cut pitch `-0x20`, yaw `-0x80`, TR `(0, 0x500, 0x1C00)`, focus z `0x400` |
/// | 9 (`0x801F785C`) | [`KEMARO_ARM9_SPAWNS`]; cut pitch `-0x40`, yaw `-0x200`, TR `(0, 0x400, 0x780)` on the creature's `z` |
/// | 11 (`0x801F7ACC`) | cut pitch `0x40`, yaw `-0x700`, TR `(0, 0x400, 0x800)` on the creature's `z` |
/// | 13 (`0x801F7DFC`) | cut pitch `0`, yaw `0`, TR `(0, 0x400, 0)` on the victim's `z` |
/// | 14 (`0x801F7EE8`) | cut pitch `0x80`, yaw `0x900`, TR `(0, 0x500, ctx[+0x6D0])` on the victim's `z` |
/// | 15 (`0x801F810C`) | a held white fade (the presentation hook's) |
/// | 16 (`0x801F82CC`) | cut pitch `-0x20`, yaw `0xC00`, TR `(0, 0x200, ctx[+0x6D0] / 4)` on the victim's `z` |
/// | 17 (`0x801F8400`, no gate) | a `0x60`-frame shot pitch `-0x20`, yaw `0xC00`, TR `(0, 0x200, ctx[+0x6D0] * 3 / 2)` on the victim's `z`; [`KEMARO_ARM17_SPAWNS`] |
/// | `0x13` (`0x801F8A4C`) | the line cleared |
///
/// The drifts are [`kemaro_drift`]'s. Not carried: arm 4's TR y shake
/// (an absolute store `0x600` or `countdown / 2 + 0x600` on alternate
/// passes, `0x801F7088..0x801F70C4`), arm 12's absolute pose store
/// (`0x801F7B90..0x801F7BAC`), the heights arms 13..17 walk the arm-9
/// parts through by handle, and the `y` of the shots of arms 16 / 17,
/// which read the first arm-9 part's height.
///
/// PORT: overlay_summon_kemaro_0918_801f6c70 (PROT 0918; camera arms 0, 4, 7, 9, 11, 13, 14, 16 and 17, the drifts of arms 1, 4 and 8..13, the spawn calls of arms 3, 5, 7, 9, 13 and 17 and the captions of arms 6, 7 and 0x13)
pub fn kemaro_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let drained = st.countdown.0 - SPEED_SCALAR;
    let ramp_before = st.ramp;
    let held = hold_step(KEMARO, st, phase);
    let mut base = if held {
        ArmDirection::HOLD
    } else {
        kemaro_pass(phase, seats, st)
    };
    // Arm 6 puts the name up on the pass its ramp reaches `0x1000`.
    if phase == 6 && !held && ramp_before < 0x1000 {
        base.caption = Some(ModuleCaption::SpellName);
    }
    // Arm 13's one-shot spawns, on the first pass at or under `scalar << 5`
    // (`0x801F7C30..0x801F7D38`; arm 12's pass zeroes the latch).
    if phase == 13 && drained <= SPEED_SCALAR << 5 && st.ramp == 0 {
        st.ramp = 1;
        base.spawns = &KEMARO_ARM13_SPAWNS;
    }
    match kemaro_drift(phase, drained) {
        Some(d) => base.drifted(d),
        None => base,
    }
}

fn kemaro_pass(phase: u8, seats: ModuleCamSeats, st: &ModuleCamState) -> ArmDirection {
    let creature_z = st
        .creature_live
        .or(st.creature)
        .map_or(-0xC00, |c| c.z)
        .wrapping_neg();
    let victim_z = seats.victim.z.wrapping_neg();
    let depth = seats.depth_raw as i16;
    match phase {
        0 => ArmDirection::shot(cut([0, 0x800], [0, 0x600, 0x800], 0, 0x400, 1)),
        3 => ArmDirection {
            spawns: &KEMARO_ARM3_SPAWNS,
            ..ArmDirection::PASS
        },
        4 => ArmDirection::shot(cut([-0xC0, 0x800], [0, 0x1900, -0x1200], 0, 0x400, 0x80)),
        5 => ArmDirection {
            spawns: &KEMARO_ARM5_SPAWNS,
            ..ArmDirection::PASS
        },
        7 => ArmDirection {
            spawns: &KEMARO_ARM7_SPAWNS,
            caption: Some(ModuleCaption::AttackName),
            ..ArmDirection::shot(cut([-0x20, -0x80], [0, 0x500, 0x1C00], 0, 0x400, 1))
        },
        9 => ArmDirection {
            spawns: &KEMARO_ARM9_SPAWNS,
            ..ArmDirection::shot(cut([-0x40, -0x200], [0, 0x400, 0x780], 0, creature_z, 1))
        },
        11 => ArmDirection::shot(cut([0x40, -0x700], [0, 0x400, 0x800], 0, creature_z, 1)),
        13 => ArmDirection::shot(cut([0, 0], [0, 0x400, 0], 0, victim_z, 1)),
        14 => ArmDirection::shot(cut([0x80, 0x900], [0, 0x500, depth], 0, victim_z, 1)),
        15 => ArmDirection {
            ..ArmDirection::PASS
        },
        16 => ArmDirection::shot(cut([-0x20, 0xC00], [0, 0x200, depth / 4], 0, victim_z, 1)),
        17 => {
            let z = (i32::from(depth) * 15 / 10) as i16;
            ArmDirection {
                spawns: &KEMARO_ARM17_SPAWNS,
                ..ArmDirection::shot(cut([-0x20, 0xC00], [0, 0x200, z], 0, victim_z, 0x60))
            }
        }
        0x13 => ArmDirection {
            caption: Some(ModuleCaption::Cleared),
            ..ArmDirection::PASS
        },
        _ => ArmDirection::PASS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kemaro_arm_4_drifts_only_above_its_threshold_and_shoots_on_the_pass() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        st.countdown.0 = SPEED_SCALAR << 7;
        let a = kemaro_direct(&mut st, 4, seats);
        assert!(a.hold);
        assert_eq!(a.drift.unwrap().tr_z, -2 * SPEED_SCALAR as i16);
        st.countdown.0 = SPEED_SCALAR * 2;
        let a = kemaro_direct(&mut st, 4, seats);
        assert!(a.hold);
        assert!(a.drift.is_none());
        let a = kemaro_direct(&mut st, 4, seats);
        assert!(!a.hold);
        assert_eq!(a.shot.unwrap().frames, 0x80);
    }

    #[test]
    fn kemaro_arm_13_spawns_once() {
        let mut st = ModuleCamState {
            countdown: ModuleCountdown(SPEED_SCALAR * 30),
            ..Default::default()
        };
        let seats = ModuleCamSeats::default();
        let first = kemaro_direct(&mut st, 13, seats);
        assert_eq!(first.spawns.len(), 3);
        let again = kemaro_direct(&mut st, 13, seats);
        assert!(again.spawns.is_empty());
    }
}
