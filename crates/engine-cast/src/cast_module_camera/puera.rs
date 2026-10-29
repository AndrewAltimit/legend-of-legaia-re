//! PROT 0922 (Puera, `0x94`)'s director, beside its ported tick body
//! (`crate::cast_module_ticks::puera_tick`): the countdown holds of
//! [`super::summon_holds::PUERA`] plus the camera, drift, spawn and caption
//! writes of the body `FUN_801F6A3C`; its fades are the presentation hook's
//! (`cast_band::presentation`).
//!
//! Every gate drains only a positive word (`blez` ahead of the `subu`), and
//! the drifts sit inside that branch: an arm drifts while its countdown is
//! still positive. A drift is `scalar * step * k` a battle frame, so
//! `scalar * k` a tick here ([`SPEED_SCALAR`]).
//!
//! Provenance: `ghidra/scripts/funcs/overlay_summon_puera_0922_801f6a3c.txt`
//! (the image at slot-B base `0x801F69D8`, arm table at file `+0`).

use super::summon_holds::{PUERA, hold_step};
use super::*;
use std::sync::OnceLock;

const fn rec(va: u32, pos: [i16; 3], rot: [i16; 3]) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(va),
        anchor: SpawnAnchor::Literal { pos, rot },
    }
}

const fn at0(va: u32) -> ModuleSpawn {
    rec(va, [0; 3], [0; 3])
}

/// PROT 0922 arm 2's two calls on its pass (`0x801F6D98`, `0x801F6DB0`), on
/// the zeroed stack trios; the second's handle is kept at `0x801F9F90`.
pub const PUERA_ARM2_SPAWNS: [ModuleSpawn; 2] = [at0(0x801F_9104), at0(0x801F_917C)];

/// PROT 0922 arm 5's call (`0x801F6F2C`), after it halts arm 2's second
/// part.
pub const PUERA_ARM5_SPAWNS: [ModuleSpawn; 1] = [at0(0x801F_9218)];

/// PROT 0922 arm 11's three calls (`0x801F7394..0x801F73D4`).
pub const PUERA_ARM11_SPAWNS: [ModuleSpawn; 3] =
    [at0(0x801F_9270), at0(0x801F_92D4), at0(0x801F_9338)];

/// PROT 0922 arm 12's call (`0x801F7650`), on the angle trio its pass shot
/// left (pitch `0x20`).
pub const PUERA_ARM12_SPAWNS: [ModuleSpawn; 1] = [rec(0x801F_939C, [0; 3], [0x20, 0, 0])];

/// PROT 0922 arm 17's five calls (`0x801F7F08..0x801F7F80`).
pub const PUERA_ARM17_SPAWNS: [ModuleSpawn; 5] = [
    at0(0x801F_9828),
    at0(0x801F_979C),
    at0(0x801F_9634),
    at0(0x801F_96AC),
    at0(0x801F_9724),
];

/// PROT 0922 arm 20's five calls (`0x801F874C..0x801F87C4`).
pub const PUERA_ARM20_SPAWNS: [ModuleSpawn; 5] = [
    at0(0x801F_98C4),
    at0(0x801F_9928),
    at0(0x801F_998C),
    at0(0x801F_99D8),
    at0(0x801F_9A84),
];

/// PROT 0922 arm 22's twelve calls (`0x801F88B0..0x801F89D4`), on the
/// angle trio its shot left (yaw `0x400`).
pub const PUERA_ARM22_SPAWNS: [ModuleSpawn; 12] = [
    rec(0x801F_9AE4, [0; 3], [0, 0x400, 0]),
    rec(0x801F_9B34, [0; 3], [0, 0x400, 0]),
    rec(0x801F_9B84, [0; 3], [0, 0x400, 0]),
    rec(0x801F_9BD4, [0; 3], [0, 0x400, 0]),
    rec(0x801F_9C24, [0; 3], [0, 0x400, 0]),
    rec(0x801F_9C74, [0; 3], [0, 0x400, 0]),
    rec(0x801F_9CE0, [0; 3], [0, 0x400, 0]),
    rec(0x801F_9D58, [0; 3], [0, 0x400, 0]),
    rec(0x801F_9DD0, [0; 3], [0, 0x400, 0]),
    rec(0x801F_9E44, [0; 3], [0, 0x400, 0]),
    rec(0x801F_9EB8, [0; 3], [0, 0x400, 0]),
    rec(0x801F_9F24, [0; 3], [0, 0x400, 0]),
];

/// The record arm 18 seats twenty-three times round its ring.
pub const PUERA_RING_RECORD: u32 = 0x801F_989C;

/// Truncating division, the `bgez; addiu (d - 1); sra` idiom.
fn tdiv(v: i32, d: i32) -> i16 {
    (v / d) as i16
}

/// PROT 0922 arm 13's sixteen calls on its pass: four on the zeroed stack
/// (`0x801F7744..0x801F77A0`), then eight of `0x801F9484` on a ring of
/// radius `7/20` (`0x801F77BC..0x801F7894`: for `i` in `0..8`, `h = i *
/// 0x200 + 0x100`, point `(7 sin h / 20, 0, 7 cos h / 20)`, angles
/// `(-cos h / 8, 0, sin h / 8)`) and four of `0x801F9594` on a ring of
/// radius `1/4` (`0x801F78B4..0x801F797C`: `h = i * 0x400 + 0x200`, point
/// `(sin h / 4, 0, cos h / 4)`, angles `(cos h / 24, 0, -sin h / 24)`).
pub fn puera_arm13_spawns() -> &'static [ModuleSpawn] {
    static S: OnceLock<Vec<ModuleSpawn>> = OnceLock::new();
    S.get_or_init(|| {
        let mut v = vec![
            at0(0x801F_93E8),
            at0(0x801F_94EC),
            at0(0x801F_9540),
            at0(0x801F_95E0),
        ];
        for i in 0..8u16 {
            let (s, c) = trig12(i * 0x200 + 0x100);
            let (s, c) = (i32::from(s), i32::from(c));
            v.push(rec(
                0x801F_9484,
                [tdiv(7 * s, 20), 0, tdiv(7 * c, 20)],
                [tdiv(-c, 8), 0, tdiv(s, 8)],
            ));
        }
        for i in 0..4u16 {
            let (s, c) = trig12(i * 0x400 + 0x200);
            let (s, c) = (i32::from(s), i32::from(c));
            v.push(rec(
                0x801F_9594,
                [tdiv(s, 4), 0, tdiv(c, 4)],
                [tdiv(c, 24), 0, tdiv(-s, 24)],
            ));
        }
        v
    })
}

/// PROT 0922 arm 18's twenty-three calls of [`PUERA_RING_RECORD`]
/// (`0x801F8024..0x801F80B4`): for `i` in `0..23`, `h = i * 0xB2`, point
/// `(sin h / 8, cos h / 8 - 0x1200, -0x1100)` on the angles `(0x280, 0,
/// 0)` - a ring standing up in front of the creature.
pub fn puera_arm18_spawns() -> &'static [ModuleSpawn] {
    static S: OnceLock<Vec<ModuleSpawn>> = OnceLock::new();
    S.get_or_init(|| {
        (0..23u16)
            .map(|i| {
                let (s, c) = trig12(i * 0xB2);
                rec(
                    PUERA_RING_RECORD,
                    [
                        tdiv(i32::from(s), 8),
                        tdiv(i32::from(c), 8) - 0x1200,
                        -0x1100,
                    ],
                    [0x280, 0, 0],
                )
            })
            .collect()
    })
}

/// One of arm 18's ring parts as arm 19's pass re-seats it: its position
/// `+0x14..+0x18` and angle trio `+0x24..+0x28`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PueraCrystalSeat {
    pub pos: [i16; 3],
    pub rot: [i16; 3],
}

/// The height a falling crystal stops at (`li t6,-0x100` at `0x801F85D8`).
pub const PUERA_CRYSTAL_FLOOR: i16 = -0x100;

/// PROT 0922 arm 19's pass (`0x801F8318..0x801F85B8`) re-seats arm 18's
/// twenty-three ring parts as the crystals arm 20 drops round the monster.
///
/// It first deals the heights: a 24-byte table at `0x801FA0D0` is zeroed and
/// each value `1..=23` is written into the slot `rand() % 23` names, drawing
/// again while that slot is taken (`0x801F8340..0x801F83BC`). Then, for the
/// part `i` whose handle sits at `0x801F9F90 + (0x16 + i) * 4`
/// (`0x801F83E8..0x801F85B8`), five draws: two `rand() % 0x801 - 0x400` for
/// `x` / `z` that the ring point overwrites at once (`trunc(2 sin h / 7)`,
/// `trunc(2 cos h / 7)` with `h = i * 0xB2`, the table index `s1` stepping
/// `0x164` bytes), the pitch and roll `rand() % 0x101 - 0x80`, and the yaw
/// `rand() & 0xFFF`. The height is `-0x2000 - (table[i] << 10)`. A point
/// with `|x| + |z| < 0x400` would draw all five again; on a ring of radius
/// `0x2000 / 7` none does.
pub fn puera_crystal_seats(mut rand: impl FnMut() -> u32) -> [PueraCrystalSeat; 23] {
    let mut table = [0u8; 24];
    for v in 1..=23u8 {
        loop {
            let slot = (rand() % 23) as usize;
            if table[slot] == 0 {
                table[slot] = v;
                break;
            }
        }
    }
    std::array::from_fn(|i| {
        let draw = |rand: &mut dyn FnMut() -> u32, m: u32, off: i32| (rand() % m) as i32 - off;
        let (s, c) = trig12((i as u16).wrapping_mul(0xB2));
        loop {
            let _ = draw(&mut rand, 0x801, 0x400);
            let _ = draw(&mut rand, 0x801, 0x400);
            let x = tdiv(2 * i32::from(s), 7);
            let z = tdiv(2 * i32::from(c), 7);
            let pitch = draw(&mut rand, 0x101, 0x80) as i16;
            let roll = draw(&mut rand, 0x101, 0x80) as i16;
            let yaw = (rand() & 0xFFF) as i16;
            if i32::from(x).abs() + i32::from(z).abs() >= 0x400 {
                break PueraCrystalSeat {
                    pos: [x, -0x2000 - (i16::from(table[i]) << 10), z],
                    rot: [pitch, yaw, roll],
                };
            }
        }
    })
}

/// One pass of arm 20's fall (`0x801F85F8..0x801F8690`, every pass, held or
/// not): a crystal above [`PUERA_CRYSTAL_FLOOR`] drops `(step * scalar) <<
/// 7`, `drop` here; one that is then at `-0xFF` or lower down is put back on
/// the floor, and has landed this pass.
/// Returns the new height and whether it landed this pass (the arm then
/// raises the pad's rumble words, unless the option at `0x800845C8` is set).
pub fn puera_crystal_fall(y: i16, drop: i32) -> (i16, bool) {
    let mut y = y;
    if y < PUERA_CRYSTAL_FLOOR {
        y = y.wrapping_add(drop as i16);
    }
    if y > PUERA_CRYSTAL_FLOOR {
        return (PUERA_CRYSTAL_FLOOR, true);
    }
    (y, false)
}

/// The parts each arm halts on its pass (`ori 0x8` into the handle's flag
/// word), by record: arm 5 arm 2's second (`0x801F6F28`), arm 12 arm 11's
/// three (`0x801F75D8..0x801F7654`, with its two fades), arm 19 arm 17's
/// five (`0x801F8214..0x801F8290`), arm 22 arm 18's ring and arm 20's five
/// (`0x801F884C..0x801F886C`).
pub fn puera_arm_kills(phase: u8) -> &'static [u32] {
    match phase {
        5 => &[0x801F_917C],
        12 => &[0x801F_9270, 0x801F_92D4, 0x801F_9338],
        19 => &[
            0x801F_9828,
            0x801F_979C,
            0x801F_9634,
            0x801F_96AC,
            0x801F_9724,
        ],
        22 => &[
            PUERA_RING_RECORD,
            0x801F_98C4,
            0x801F_9928,
            0x801F_998C,
            0x801F_99D8,
            0x801F_9A84,
        ],
        _ => &[],
    }
}

/// A cut on the stage origin (every shot of the body has a zero focus).
fn cut(angles: [i16; 2], tr: [i16; 3], frames: u16) -> ModuleShot {
    ModuleShot {
        angles: [angles[0], angles[1], 0],
        tr,
        focus: [0; 3],
        frames,
    }
}

/// The drift arm `phase` writes on a pass whose countdown is still positive.
pub fn puera_drift(phase: u8) -> Option<CaptureDrift> {
    let p = SPEED_SCALAR;
    let d = |pitch: i32, yaw: i32, tr_y: i32, tr_z: i32| {
        Some(CaptureDrift {
            pitch: pitch as i16,
            yaw: yaw as i16,
            tr_y: tr_y as i16,
            tr_z: tr_z as i16,
            tr_x: 0,
        })
    };
    match phase {
        // `0x801F6CA0..0x801F6CB8`, `0x801F6D2C..0x801F6D48`.
        1 | 2 => d(0, 0, 0, p),
        // `0x801F7714..0x801F772C` and arms 14 / 15's twins.
        13..=15 => d(0, 0, 0, 6 * p),
        // `0x801F7D48..0x801F7D94`.
        16 => d(-(p / 8), 0, -5 * p, 6 * p),
        // `0x801F7E58..0x801F7ECC`.
        17 => d(p / 8, p / 4, 7 * p, p / 8),
        // `0x801F86E4..0x801F871C`: the half-step lands on the pitch
        // (`lhu v0,-0x4870(a0)` = `0x8007B790`), not the yaw.
        20 => d(p / 2, 0, -2 * p, 16 * p),
        // `0x801F8F4C..0x801F8F64`.
        24 => d(0, 0, 0, -p),
        _ => None,
    }
}

/// PROT 0922 (Puera).
///
/// | arm | on pass |
/// |---|---|
/// | 0 (`0x801F6C44`) | cut pitch `0`, yaw `0x800`, TR `(0, 0x300, 0xC00)` |
/// | 2 | [`PUERA_ARM2_SPAWNS`] |
/// | 5 | [`PUERA_ARM5_SPAWNS`] |
/// | 6 (`0x801F702C`, `0x801F7060`) | the spell name; a `0x80`-frame shot pitch `-0x80`, yaw `0x800`, TR `(0, 0x300, 0x600)` |
/// | 9 (`0x801F7200`, `0x801F7258`) | a `0x20`-frame shot pitch `0x80`, yaw `0x800`, TR `(0, 0x300, 0x800)`; the attack-record name |
/// | 10 (`0x801F72E8`) | a `0x40`-frame shot yaw `0x800`, TR `(0, 0x1200, 0x800)` |
/// | 11 | [`PUERA_ARM11_SPAWNS`] |
/// | 12 (`0x801F74E8` every pass, `0x801F75D0`) | every pass a 3-frame shot yaw `0x800`, TR `(0, 8 w + 0x1200, 2 w + 0x800)` with `w = (scalar << 6) - countdown`; on the pass a cut pitch `0x20`, TR `(0, 0x400, 0xC00)`, [`PUERA_ARM12_SPAWNS`] |
/// | 13 | [`puera_arm13_spawns`] |
/// | 16 (`0x801F7DE0`) | cut pitch `-0x140`, yaw `0x700`, TR `(0, -0xE00, 0x1700)` |
/// | 17 | [`PUERA_ARM17_SPAWNS`] |
/// | 18 | [`puera_arm18_spawns`] |
/// | 19 (`0x801F82AC`) | cut pitch `0x40`, TR `(0, 0x400, 0x1000)` |
/// | 20 | [`PUERA_ARM20_SPAWNS`] |
/// | 22 (`0x801F8898`) | cut yaw `0x400`, TR `(0, -0x800, 0x4000)`; [`PUERA_ARM22_SPAWNS`] |
/// | 23 (`0x801F8D68`) | cut pitch `0x20`, TR `(0, 0x500, 0x1000)` |
/// | 24 (`0x801F90A4`) | the line cleared |
///
/// The drifts are [`puera_drift`]'s; the halts are [`puera_arm_kills`]'.
/// Not carried: the TR y shake arms 14 and 15 store absolutely on alternate
/// passes (`0x801F7A0C..0x801F7A60`, `0x801F7BDC..0x801F7C6C`) and arm 22's
/// colour-wash writes. Arm 19's crystal re-seat and arm 20's fall are
/// [`puera_crystal_seats`] / [`puera_crystal_fall`], applied to the parts by
/// the engine.
///
/// PORT: overlay_summon_puera_0922_801f6a3c (PROT 0922; camera arms 0, 6, 9, 10, 12, 16, 19, 22 and 23, the drifts of arms 1, 2, 13..17, 20 and 24, the spawn calls of arms 2, 5, 11, 12, 13, 17, 18, 20 and 22 and the captions of arms 6, 9 and 24)
pub fn puera_direct(st: &mut ModuleCamState, phase: u8, _seats: ModuleCamSeats) -> ArmDirection {
    let before = st.countdown.0;
    let held = hold_step(PUERA, st, phase);
    let mut base = if held {
        ArmDirection::HOLD
    } else {
        puera_pass(phase)
    };
    if phase == 12 && held {
        let w = (SPEED_SCALAR << 6) - before;
        base.shot = Some(cut(
            [0, 0x800],
            [0, (8 * w + 0x1200) as i16, (2 * w + 0x800) as i16],
            3,
        ));
    }
    match puera_drift(phase).filter(|_| before > 0) {
        Some(d) => base.drifted(d),
        None => base,
    }
}

fn puera_pass(phase: u8) -> ArmDirection {
    match phase {
        0 => ArmDirection::shot(cut([0, 0x800], [0, 0x300, 0xC00], 1)),
        2 => ArmDirection {
            spawns: &PUERA_ARM2_SPAWNS,
            ..ArmDirection::PASS
        },
        5 => ArmDirection {
            spawns: &PUERA_ARM5_SPAWNS,
            ..ArmDirection::PASS
        },
        6 => ArmDirection {
            caption: Some(ModuleCaption::SpellName),
            ..ArmDirection::shot(cut([-0x80, 0x800], [0, 0x300, 0x600], 0x80))
        },
        9 => ArmDirection {
            caption: Some(ModuleCaption::AttackName),
            ..ArmDirection::shot(cut([0x80, 0x800], [0, 0x300, 0x800], 0x20))
        },
        10 => ArmDirection::shot(cut([0, 0x800], [0, 0x1200, 0x800], 0x40)),
        11 => ArmDirection {
            spawns: &PUERA_ARM11_SPAWNS,
            ..ArmDirection::PASS
        },
        12 => ArmDirection {
            spawns: &PUERA_ARM12_SPAWNS,
            ..ArmDirection::shot(cut([0x20, 0], [0, 0x400, 0xC00], 1))
        },
        13 => ArmDirection {
            spawns: puera_arm13_spawns(),
            ..ArmDirection::PASS
        },
        16 => ArmDirection::shot(cut([-0x140, 0x700], [0, -0xE00, 0x1700], 1)),
        17 => ArmDirection {
            spawns: &PUERA_ARM17_SPAWNS,
            ..ArmDirection::PASS
        },
        18 => ArmDirection {
            spawns: puera_arm18_spawns(),
            ..ArmDirection::PASS
        },
        19 => ArmDirection::shot(cut([0x40, 0], [0, 0x400, 0x1000], 1)),
        20 => ArmDirection {
            spawns: &PUERA_ARM20_SPAWNS,
            ..ArmDirection::PASS
        },
        22 => ArmDirection {
            spawns: &PUERA_ARM22_SPAWNS,
            ..ArmDirection::shot(cut([0, 0x400], [0, -0x800, 0x4000], 1))
        },
        23 => ArmDirection::shot(cut([0x20, 0], [0, 0x500, 0x1000], 1)),
        24 => ArmDirection {
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
    fn puera_rings_are_on_their_radii() {
        let s = puera_arm13_spawns();
        assert_eq!(s.len(), 16);
        let s18 = puera_arm18_spawns();
        assert_eq!(s18.len(), 23);
        let SpawnAnchor::Literal { pos, .. } = s18[0].anchor else {
            panic!("literal");
        };
        // h = 0: sin 0, cos 0x1000.
        assert_eq!(pos, [0, 0x200 - 0x1200, -0x1100]);
    }

    #[test]
    fn puera_crystals_ring_the_floor_at_staggered_heights() {
        let mut seed = 0x1234_5678u32;
        let seats = puera_crystal_seats(|| {
            seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
            (seed >> 16) & 0x7FFF
        });
        // h = 0: (0, 2 * 0x1000 / 7).
        assert_eq!([seats[0].pos[0], seats[0].pos[2]], [0, 0x2000 / 7]);
        let mut heights: Vec<i16> = seats.iter().map(|s| s.pos[1]).collect();
        heights.sort_unstable();
        let want: Vec<i16> = (1..=23).rev().map(|k: i16| -0x2000 - (k << 10)).collect();
        assert_eq!(heights, want);
        assert!(seats.iter().all(|s| (-0x80..=0x80).contains(&s.rot[0])));
    }

    #[test]
    fn a_crystal_stops_on_the_floor() {
        assert_eq!(puera_crystal_fall(-0x2400, 0x100), (-0x2300, false));
        assert_eq!(
            puera_crystal_fall(-0x180, 0x100),
            (PUERA_CRYSTAL_FLOOR, true)
        );
        // Exactly on the floor is not past it: no landing, no rumble.
        assert_eq!(
            puera_crystal_fall(-0x200, 0x100),
            (PUERA_CRYSTAL_FLOOR, false)
        );
        assert_eq!(
            puera_crystal_fall(PUERA_CRYSTAL_FLOOR, 0x100),
            (PUERA_CRYSTAL_FLOOR, false)
        );
    }

    #[test]
    fn puera_arm_12_follows_every_held_pass() {
        let mut st = ModuleCamState {
            countdown: ModuleCountdown(SPEED_SCALAR << 6),
            ..Default::default()
        };
        let a = puera_direct(&mut st, 12, ModuleCamSeats::default());
        assert!(a.hold);
        let s = a.shot.unwrap();
        assert_eq!(s.tr, [0, 0x1200, 0x800]);
        assert_eq!(s.frames, 3);
    }
}
