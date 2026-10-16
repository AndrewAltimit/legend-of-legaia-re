//! PROT 0927 (Juggernaut, `0x99`)'s director, beside its ported tick body
//! (`crate::cast_module_ticks::juggernaut_tick`): the countdown holds of
//! [`super::summon_holds::JUGGERNAUT`] plus every camera, drift, spawn
//! and caption write of the body `FUN_801F6A84`; its fades are the presentation
//! hook's (`cast_band::presentation`).
//!
//! The camera arms read one for one against PROT 0966's Evil Seru Magic
//! ([`super::evil_seru_magic_camera`]): the same shots and the same drifts,
//! shifted by the arms the two bodies number differently (0927's arm 0 is
//! 0966's arm 10, 0927's arm 21 carries 0966's arm 22 cut, 0927's arm 27
//! frames the caster at TR `(0, 0x400, 0x500)` where 0966's does at
//! `(0, 0x800, 0x400)`).
//!
//! Provenance: `ghidra/scripts/funcs/overlay_summon_juggernaut_0927_801f6a84.txt`
//! (the image at slot-B base `0x801F69D8`, arm table at `0x801F69E8`).

use super::summon_holds::{JUGGERNAUT, hold_step};
use super::*;

const fn lit(va: u32, pos: [i16; 3]) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(va),
        anchor: SpawnAnchor::Literal { pos, rot: [0; 3] },
    }
}

/// Arm 2's spawn point: the stack trio `sp+0x30` the prologue zeroed with
/// `0x190` stored over its `z` in the first call's delay slot
/// (`0x801F6E14`), on the zeroed angle trio `sp+0x20`.
const ARM2_POINT: [i16; 3] = [0, 0, 0x190];

/// PROT 0927 arm 2's fifteen calls on its pass (`0x801F6E10..0x801F6F60`):
/// fourteen through the pool wrapper `FUN_80050ED4`, the last through
/// `FUN_80021B04` - the magic circle and its rings on the stage origin.
pub const JUGGERNAUT_ARM2_SPAWNS: [ModuleSpawn; 15] = [
    lit(0x801F_8B50, ARM2_POINT),
    lit(0x801F_8BA4, ARM2_POINT),
    lit(0x801F_8C0C, ARM2_POINT),
    lit(0x801F_8C58, ARM2_POINT),
    lit(0x801F_8CA0, ARM2_POINT),
    lit(0x801F_8CF8, ARM2_POINT),
    lit(0x801F_8D18, ARM2_POINT),
    lit(0x801F_8DA4, ARM2_POINT),
    lit(0x801F_8DF0, ARM2_POINT),
    lit(0x801F_8E38, ARM2_POINT),
    lit(0x801F_8E90, ARM2_POINT),
    lit(0x801F_8EE0, ARM2_POINT),
    lit(0x801F_8F4C, ARM2_POINT),
    lit(0x801F_907C, ARM2_POINT),
    lit(0x801F_90DC, ARM2_POINT),
];

/// PROT 0927 arm 23's eighteen pool calls on its pass
/// (`0x801F7874..0x801F7A4C`), on the zeroed stack trios. The first
/// thirteen handles are kept at `0x801F9C8C + 0x28..` and the epilogue
/// copies the first part's `+0x14..+0x1B` onto parts 2..13 every pass
/// (`0x801F832C..`).
pub const JUGGERNAUT_ARM23_SPAWNS: [ModuleSpawn; 18] = [
    lit(0x801F_913C, [0; 3]),
    lit(0x801F_92F8, [0; 3]),
    lit(0x801F_9370, [0; 3]),
    lit(0x801F_93EC, [0; 3]),
    lit(0x801F_9468, [0; 3]),
    lit(0x801F_94E0, [0; 3]),
    lit(0x801F_955C, [0; 3]),
    lit(0x801F_95D8, [0; 3]),
    lit(0x801F_977C, [0; 3]),
    lit(0x801F_97DC, [0; 3]),
    lit(0x801F_9840, [0; 3]),
    lit(0x801F_98A4, [0; 3]),
    lit(0x801F_9908, [0; 3]),
    lit(0x801F_996C, [0; 3]),
    lit(0x801F_91D0, [0; 3]),
    lit(0x801F_9264, [0; 3]),
    lit(0x801F_9654, [0; 3]),
    lit(0x801F_96E8, [0; 3]),
];

/// PROT 0927 arm 26's eight calls on its pass, after the pool flush
/// (`jal 0x80050E74` at `0x801F7BFC`): seven pool calls
/// (`0x801F7C20..0x801F7CB0`) and one `FUN_80021B04` (`0x801F7CC8`).
pub const JUGGERNAUT_ARM26_SPAWNS: [ModuleSpawn; 8] = [
    lit(0x801F_99D0, [0; 3]),
    lit(0x801F_9A30, [0; 3]),
    lit(0x801F_9AAC, [0; 3]),
    lit(0x801F_9AF8, [0; 3]),
    lit(0x801F_9BB0, [0; 3]),
    lit(0x801F_9BF8, [0; 3]),
    lit(0x801F_9C40, [0; 3]),
    lit(0x801F_9A90, [0; 3]),
];

/// The two records PROT 0927 seats through `FUN_80021B04` rather than the
/// pool wrapper: a pool flush (`FUN_80050E74`, arms 26 and 27) leaves them
/// running.
pub const JUGGERNAUT_UNPOOLED: [u32; 2] = [0x801F_90DC, 0x801F_9A90];

/// The arms whose pass flushes the effect pool (`jal 0x80050E74` at
/// `0x801F7BFC` and `0x801F8008`), ahead of their own spawns.
pub fn juggernaut_flushes_pool(phase: u8) -> bool {
    matches!(phase, 26 | 27)
}

/// The string PROT 0927's arm 21 prints, the head of its own image (the arm
/// table starts four words later, at `0x801F69E8`).
pub const JUGGERNAUT_ATTACK_STRING: u32 = 0x801F_69D8;

/// A cut whose focus trio is `(0, 0, fz)` (every immediate-only shot of the
/// body: the prologue zeroes the nine stack halfwords each pass).
fn fixed(angles: [i16; 2], tr: [i16; 3], fz: i16) -> ModuleShot {
    ModuleShot {
        angles: [angles[0], angles[1], 0],
        tr,
        focus: [0, 0, fz],
        frames: 1,
    }
}

/// The drift arm `phase` writes into the camera globals on every pass, held
/// or not, `k * step` a battle frame and so `k` a tick: pitch / yaw
/// `0x8007B790` / `0x8007B792`, TR x / y / z `0x800840B8` / `BC` / `C0`.
pub fn juggernaut_drift(phase: u8) -> Option<CaptureDrift> {
    let d = |pitch, yaw, tr_x, tr_y, tr_z| {
        Some(CaptureDrift {
            pitch,
            yaw,
            tr_y,
            tr_z,
            tr_x,
        })
    };
    match phase {
        11 => d(1, 4, 0, -4, 12),
        12 => d(-2, 4, 0, 4, -6),
        13 | 14 => d(0, 4, 0, 2, 6),
        16 => d(0, 0, 0, 4, -8),
        19 => d(0, 1, -16, 0, 32),
        20 => d(2, 0, 8, -40, 40),
        21 => d(0, 0, 0, 0, -4),
        23 => d(0, -1, 0, 0, 4),
        24 => d(0, 2, 14, 1, 4),
        26 | 27 => d(0, 0, 0, 0, 4),
        28 => d(0, 0, 0, 0, 2),
        _ => None,
    }
}

/// PROT 0927 (Juggernaut).
///
/// | arm | drift a tick | on pass |
/// |---|---|---|
/// | 0 (`0x801F6B30`) | - | cut pitch `0xE0`, yaw `0x800`, TR `(0, 0x180, 0xC00)`, focus z `-0x190` (`0x801F6D34`) |
/// | 2 (`0x801F6D84`) | - | [`JUGGERNAUT_ARM2_SPAWNS`] |
/// | 11 (`0x801F7050`) | pitch `+1`, yaw `+4`, TR y `-4`, TR z `+12` | - |
/// | 12 (`0x801F70F0`) | pitch `-2`, yaw `+4`, TR y `+4`, TR z `-6` | - |
/// | 13, 14 | yaw `+4`, TR y `+2`, TR z `+6` | - |
/// | 16 (`0x801F7318`) | TR y `+4`, TR z `-8` | - |
/// | 17 (`0x801F7374`) | - | cut yaw `0xD80`, TR `(0, 0xA00, 0x1000)`, focus z `-0x800` (`0x801F73C8`) |
/// | 19 (`0x801F74E0`) | yaw `+1`, TR x `-16`, TR z `+32` | cut pitch `-0x100`, yaw `0x100`, TR `(0x80, 0x1800, 0x1400)`, focus z `-0x800` (`0x801F75B4`) |
/// | 20 (`0x801F75C4`) | pitch `+2`, TR x `+8`, TR y `-40`, TR z `+40` | the record name (`0x801F76AC`) |
/// | 21 (`0x801F76BC`) | TR z `-4` | the attack name (`0x801F7724`); cut pitch `0x40`, yaw `0x80`, TR `(0, 0, 0x1A00)`, focus z `-0x400` (`0x801F77F8`) |
/// | 23 (`0x801F7808`) | yaw `-1`, TR z `+4` | [`JUGGERNAUT_ARM23_SPAWNS`]; cut pitch `-0x80`, yaw `-0x80`, TR `(-0x600, 0x800, 0x1400)`, focus z `-0x600` (`0x801F7A94`) |
/// | 24 (`0x801F7AC0`) | yaw `+2`, TR x `+14`, TR y `+1`, TR z `+4` | - |
/// | 25 (`0x801F7B5C`) | - | cut pitch `-0x80`, TR `(0, 0x900, 0x1600)`, focus z `-0x400` (`0x801F7BB0`) |
/// | 26 (`0x801F7BC0`) | TR z `+4` | pool flush, [`JUGGERNAUT_ARM26_SPAWNS`], three fades |
/// | 27 (`0x801F7F40`) | TR z `+4` | pool flush, a fade, text cleared; cut yaw `0x800 - caster[+0x46]`, TR `(0, 0x400, 0x500)` on the caster (`0x801F81A8`) |
/// | 28 (`0x801F81D8`) | TR z `+2` | the body finishes |
///
/// Arm 20 prints `*(0x801C9358)` after clearing the line - the streamed
/// actor record's `rec[0]` string ([`ModuleCaption::AttackName`]), which the
/// retail capture shows as the summon's own name behind its icon glyph; arm
/// 21 prints the module's own string at `0x801F69D8`
/// ([`JUGGERNAUT_ATTACK_STRING`]), the attack name. The holds are
/// [`super::summon_holds::JUGGERNAUT`]'s.
///
/// PORT: overlay_summon_juggernaut_0927_801f6a84 (PROT 0927; camera arms 0, 11..28, their drifts, the spawn calls of arms 2 / 23 / 26 and the captions of arms 20 / 21 / 27)
pub fn juggernaut_direct(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> ArmDirection {
    let held = hold_step(JUGGERNAUT, st, phase);
    let drift = juggernaut_drift(phase);
    let base = if held {
        ArmDirection::HOLD
    } else {
        juggernaut_pass(phase, seats)
    };
    match drift {
        Some(d) => base.drifted(d),
        None => base,
    }
}

fn juggernaut_pass(phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    match phase {
        0 => ArmDirection::shot(fixed([0xE0, 0x800], [0, 0x180, 0xC00], -0x190)),
        2 => ArmDirection {
            spawns: &JUGGERNAUT_ARM2_SPAWNS,
            ..ArmDirection::PASS
        },
        17 => ArmDirection::shot(fixed([0, 0xD80], [0, 0xA00, 0x1000], -0x800)),
        19 => ArmDirection::shot(fixed([-0x100, 0x100], [0x80, 0x1800, 0x1400], -0x800)),
        20 => ArmDirection {
            caption: Some(ModuleCaption::AttackName),
            ..ArmDirection::PASS
        },
        21 => ArmDirection {
            caption: Some(ModuleCaption::ModuleString(JUGGERNAUT_ATTACK_STRING)),
            ..ArmDirection::shot(fixed([0x40, 0x80], [0, 0, 0x1A00], -0x400))
        },
        23 => ArmDirection {
            spawns: &JUGGERNAUT_ARM23_SPAWNS,
            ..ArmDirection::shot(fixed([-0x80, -0x80], [-0x600, 0x800, 0x1400], -0x600))
        },
        25 => ArmDirection::shot(fixed([-0x80, 0], [0, 0x900, 0x1600], -0x400)),
        26 => ArmDirection {
            spawns: &JUGGERNAUT_ARM26_SPAWNS,
            ..ArmDirection::PASS
        },
        27 => {
            let c = seats.caster;
            ArmDirection {
                caption: Some(ModuleCaption::Cleared),
                ..ArmDirection::shot(ModuleShot {
                    angles: [0, yaw_from(0x800, c.facing), 0],
                    tr: [0, 0x400, 0x500],
                    focus: focus_on(c),
                    frames: 1,
                })
            }
        }
        _ => ArmDirection::PASS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn juggernaut_drifts_on_held_passes_and_cuts_on_the_pass() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        // Arm 0 cuts at once.
        let a = juggernaut_direct(&mut st, 0, seats);
        assert!(!a.hold);
        assert_eq!(a.shot.unwrap().tr, [0, 0x180, 0xC00]);
        // Arm 2 arms 0x80 and spawns the circle.
        let a = juggernaut_direct(&mut st, 2, seats);
        assert_eq!(a.spawns.len(), 15);
        assert_eq!(st.countdown.0, 0x80);
        // Arm 11 drifts while it holds.
        let a = juggernaut_direct(&mut st, 11, seats);
        assert!(a.hold);
        assert_eq!(a.drift.unwrap().tr_z, 12);
        assert!(a.shot.is_none());
    }

    #[test]
    fn juggernaut_arm_27_frames_the_caster() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats {
            caster: ModuleSeat {
                x: 56,
                y: 0,
                z: 768,
                facing: 0x800,
            },
            ..Default::default()
        };
        // Run the countdown out.
        st.countdown.0 = 1;
        let a = juggernaut_direct(&mut st, 27, seats);
        assert!(!a.hold);
        let s = a.shot.unwrap();
        assert_eq!(s.angles, [0, 0, 0]);
        assert_eq!(s.tr, [0, 0x400, 0x500]);
        assert_eq!(s.focus, [-56, 0, -768]);
    }
}
