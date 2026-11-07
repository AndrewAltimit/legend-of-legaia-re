//! PROT 0942's **Dark Typhoon** body (`0x801F69F4`, action `0xAA`): the camera
//! arms and the party's hide and show. The phase chain is
//! [`crate::cast_module_ticks::POWER_UP_AA_CHAIN`], its countdowns
//! [`super::capture_countdown::POWER_UP_AA`].
//!
//! See `ghidra/scripts/funcs/overlay_cast_power_up_0942_801f69f4.txt`.

use super::{
    CaptureCamArm, CaptureDrift, CapturePartWrite, CaptureSeatWrite, ModuleCamSeats,
    ModuleCamState, ModuleShot, ModuleSpawn, SPEED_SCALAR, SpawnAnchor, SpawnRecord, focus_on,
    yaw_from,
};
use legaia_engine_system::trig::trig12;

const fn on_shot(record: u32, scale: u16) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor: SpawnAnchor::ShotFocusScaled { scale },
    }
}

const fn on_caster(record: u32) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor: SpawnAnchor::Caster { y: 0 },
    }
}

/// Arm 0's seven calls on the shot's focus and angle trios: three through
/// `FUN_80021B04` (the vortex, `0xA00` / `0x400` / `0x1000`, handles at
/// `0x801F88E4..0x801F88EC`), then four through the pool wrapper
/// (`0x801F6AF4..0x801F6C04`).
pub(super) const ARM0_SPAWNS: [ModuleSpawn; 7] = [
    on_shot(0x801F_8324, 0xA00),
    on_shot(0x801F_821C, 0x400),
    on_shot(0x801F_82A0, 0x1000),
    on_shot(0x801F_83A8, 0x1000),
    on_shot(0x801F_8414, 0x1000),
    on_shot(0x801F_84C8, 0x1000),
    on_shot(0x801F_87C4, 0x1000),
];

/// Arm 3's five pool calls on the caster's `+0x34` / `+0x44`; the last two
/// handles go to `0x801F88F0` / `0x801F88F4` (`0x801F7364..0x801F73F4`).
pub(super) const ARM3_SPAWNS: [ModuleSpawn; 5] = [
    on_caster(0x801F_8568),
    on_caster(0x801F_85E4),
    on_caster(0x801F_8608),
    on_caster(0x801F_8734),
    on_caster(0x801F_877C),
];

/// Arm 4's two pool calls on the caster (`0x801F78F0..0x801F7958`).
pub(super) const ARM4_SPAWNS: [ModuleSpawn; 2] = [on_caster(0x801F_867C), on_caster(0x801F_86EC)];

/// Arm 4 halts the two records arm 3 parked (`ori 0x8` into `+0x10` of the
/// handles `0x801F88F0` / `0x801F88F4`, `0x801F7910..0x801F7940`).
pub(super) const ARM4_HALTS: [u32; 2] = [0x801F_8734, 0x801F_877C];

/// Arm 6 halts the three vortex records arm 0 seated straight through
/// `FUN_80021B04`, which the pool flush does not reach (`0x801F7C30..0x801F7C58`).
pub(super) const ARM6_HALTS: [u32; 3] = VORTEX;

/// The three vortex records arm 0 seats through `FUN_80021B04` (handles
/// `0x801F88E4..0x801F88EC`).
const VORTEX: [u32; 3] = [0x801F_8324, 0x801F_821C, 0x801F_82A0];

/// Arm 4's ring after its two calls: eight `0x801F867C` records a third of a
/// trig sample off the caster's plane pair at bearings `facing + 0x100 +
/// 0x200 i`, on the caster's angle trio (`0x801F795C..0x801F7A1C`; the
/// `0x55555556` multiply is the `/ 3`).
pub fn dark_typhoon_ring(ground: (i16, i16), facing: i16, rot: [i16; 3]) -> Vec<ModuleSpawn> {
    let (x0, z0) = ground;
    (0..8i32)
        .map(|i| {
            let a = ((i << 9) + i32::from(facing) + 0x100) & 0xFFF;
            let (sin, cos) = trig12(a as u16);
            ModuleSpawn {
                record: SpawnRecord::Module(0x801F_867C),
                anchor: SpawnAnchor::LiteralScaled {
                    pos: [x0.wrapping_add(sin / 3), 0, z0.wrapping_add(cos / 3)],
                    rot,
                    scale: 0x1000,
                },
            }
        })
        .collect()
}

/// PROT 0942's `0xAA` tick body.
pub const DARK_TYPHOON_BODY: u32 = 0x801F_69F4;

/// **Dark Typhoon** (`sltiu 7` through the table at `0x801F69D8`; caster
/// `actor_table[ctx + 0x13]`). Every arm past 0 adds its drift into the camera
/// globals (`0x8007B790` / `0x8007B792`, TR `0x800840B8..0x800840C0`) on every
/// pass, then drains the module word `0x801F88B4` by `step * rate` and returns
/// busy while it stays positive:
///
/// | arm | drift a tick | on the gate | countdown |
/// |---|---|---|---|
/// | 0 (`0x801F6A9C`) | - | shot: yaw `0x800 - facing`, TR `(0, 0x500, 0x600)`, `0xC` frames on the caster | seed `scalar * 0x180` |
/// | 1 (`0x801F6C9C`) | TR z `+s`; every party seat's colour `0` and `+0x21C = 0xFF` | a cut: pitch `0x40`, yaw `0x900 - facing`, TR `(-0xC0, 0x900, 0x200)` | `+= scalar * 0x60` |
/// | 2 (`0x801F6E38`) | TR z `+s`, TR x `+s / 4`, TR y `-s`, yaw `-s / 2`, pitch `-s / 8` | a cut: pitch `0xC0`, yaw `0x800 - facing`, TR `(0, 0x600, 0x400)`; every party seat shown (`0x20080200`, `+0x21C = 0`) | `+= scalar << 7` |
/// | 3 (`0x801F715C`) | TR z `+12 s`, TR y `-2 s` | - | `+= scalar * 0x180` |
/// | 4 (`0x801F73F8`) | TR z `-s / 2`, TR y `+s / 8` | - | `+= scalar << 6` |
/// | 5 (`0x801F7A2C`) | TR z `-s / 2`, TR y `-s / 4` | - | `+= scalar << 7` |
/// | 6 (`0x801F7BAC`) | TR z `-6 s`, TR y `+s` | the pool flush (the presentation decode's) and the vortex halted, then done | - |
///
/// `s` is `step * rate` over the step: the engine ticks once a vsync.
///
/// PORT: FUN_801F69F4 (PROT 0942 `0xAA`; the camera arms and the party's hide and show)
pub fn dark_typhoon_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let s = SPEED_SCALAR as i16;
    let c = seats.caster;
    let shot = |pitch: i16, k: i32, tr: [i16; 3], frames: u16| ModuleShot {
        angles: [pitch, yaw_from(k, c.facing), 0],
        tr,
        focus: focus_on(c),
        frames,
    };
    let drift = |pitch: i16, yaw: i16, tr_x: i16, tr_y: i16, tr_z: i16| CaptureDrift {
        pitch,
        yaw,
        tr_y,
        tr_z,
        tr_x,
    };
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    match phase {
        0 => {
            st.countdown.0 = SPEED_SCALAR * 0x180;
            arm.shot = Some(shot(0, 0x800, [0, 0x500, 0x600], 0xC));
        }
        1..=6 => {
            arm.drift = Some(match phase {
                1 => drift(0, 0, 0, 0, s),
                2 => drift(-(s >> 3), -(s >> 1), s >> 2, -s, s),
                3 => drift(0, 0, 0, -2 * s, 12 * s),
                4 => drift(0, 0, 0, s >> 3, -(s >> 1)),
                5 => drift(0, 0, 0, -(s >> 2), -(s >> 1)),
                _ => drift(0, 0, 0, s, -6 * s),
            });
            if phase == 1 {
                // `0x801F6CDC..0x801F6D20`, ahead of the gate: every party
                // seat's colour word cleared and `+0x21C = 0xFF`.
                arm.party_write = CaptureSeatWrite {
                    colour: Some(0),
                    render_flag: Some(0xFF),
                    ..Default::default()
                };
            }
            // Arms 2 and 3 walk the vortex's position halfwords on every pass
            // (`0x801F6EE0..0x801F6FEC`: `-2 s` on both; `0x801F71C0..
            // 0x801F721C`: `-12 * step` on the height).
            arm.part_write = match phase {
                2 => Some(CapturePartWrite {
                    records: &VORTEX,
                    dx: -2 * s,
                    dy: -2 * s,
                    ..Default::default()
                }),
                3 => Some(CapturePartWrite {
                    records: &VORTEX,
                    dy: -12,
                    ..Default::default()
                }),
                _ => None,
            };
            arm.hold = st.countdown.drain_above(0);
            if arm.hold {
                return arm;
            }
            match phase {
                1 => {
                    // The pool flush at `0x801F6D5C` is the presentation
                    // decode's (`PresentationCall::PoolFlush`): it halts the
                    // pool's records and leaves the three vortex parts.
                    arm.shot = Some(shot(0x40, 0x900, [-0xC0, 0x900, 0x200], 1));
                    // The vortex stood at `(0x200, 0x140)` (`0x801F6DF0..0x801F6E34`).
                    arm.part_write = Some(CapturePartWrite {
                        records: &VORTEX,
                        x: Some(0x200),
                        y: Some(0x140),
                        ..Default::default()
                    });
                    st.countdown.add(0x60);
                }
                2 => {
                    arm.shot = Some(shot(0xC0, 0x800, [0, 0x600, 0x400], 1));
                    // ... and back to `(0, 0x300)` (`0x801F70FC..0x801F7158`).
                    arm.part_write = Some(CapturePartWrite {
                        records: &VORTEX,
                        dx: -2 * s,
                        dy: -2 * s,
                        x: Some(0),
                        y: Some(0x300),
                    });
                    arm.party_write = CaptureSeatWrite {
                        colour: Some(0x2008_0200),
                        render_flag: Some(0),
                        ..Default::default()
                    };
                    st.countdown.add(1 << 7);
                }
                3 => st.countdown.add(0x180),
                4 => st.countdown.add(1 << 6),
                5 => st.countdown.add(1 << 7),
                _ => {}
            }
        }
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_one_holds_its_count_then_cuts_and_flushes() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        let a = dark_typhoon_camera(&mut st, 0, seats);
        assert_eq!(a.shot.expect("arm 0's shot").tr, [0, 0x500, 0x600]);
        let mut held = 0;
        loop {
            let a = dark_typhoon_camera(&mut st, 1, seats);
            assert_eq!(a.drift.expect("a drift").tr_z, SPEED_SCALAR as i16);
            if !a.hold {
                assert_eq!(a.shot.expect("the cut").tr, [-0xC0, 0x900, 0x200]);
                break;
            }
            held += 1;
        }
        assert_eq!(held + 1, 0x180, "arm 1 runs 0x180 ticks");
    }
}
