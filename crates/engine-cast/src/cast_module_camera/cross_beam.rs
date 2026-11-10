//! PROT 0948's body (`0x801F69F0`, Cross Beam `0x58`): the camera arms, the
//! eight charge records and the seat hides.
//!
//! See `ghidra/scripts/funcs/overlay_cast_cross_beam_0948_801f69f0.txt`.

use super::v_windhash::{HIDDEN, SHOWN, seat};
use super::{
    CaptureCamArm, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleShot, SPEED_SCALAR,
    bearing_12bit_approx, focus_on,
};

/// The module's countdown word (`lui 0x8020` / `-0x77AC`).
pub const CROSS_BEAM_COUNTDOWN: u32 = 0x801F_8854;

/// The eight records arm 1 seats on its zeroed trios
/// (`0x801F6D08..0x801F6DA0`).
const CHARGE_RECORDS: [u32; 8] = [
    0x801F_8564,
    0x801F_85B8,
    0x801F_860C,
    0x801F_867C,
    0x801F_86F0,
    0x801F_8748,
    0x801F_87A4,
    0x801F_87FC,
];

/// **Cross Beam** (`sltiu 6` through the table at `0x801F69D8`; caster
/// `actor_table[ctx + 0x13]`, victim `actor_table[caster + 0x1DD]`):
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 0 (`0x801F6AA4`) | the caster turns on the victim; every party seat and live monster hidden, the caster shown; shot on the caster: pitch `-0x20`, yaw `0x800 - facing`, TR `(0, 0x600, ctx[+0x6D0] / 3)`, `0x20` frames | seed `scalar << 6` |
/// | 1 (`0x801F6CC0`) | once the gate passes, eight records, then pitch `0`, the same yaw, TR `(0, 0x600, ctx[+0x6D0] / 2)`, `0x20` frames | `+= scalar * 0x60` |
/// | 2 (`0x801F6E40`) | once the gate passes, the victim shown and the caster hidden; a cut on the victim: pitch `0x100`, yaw `-facing`, TR `(0, 0, 0x1800)` | `+= scalar << 7` |
/// | 3 (`0x801F6EF4`) | once the gate passes, on the victim: pitch `0x40`, yaw `-facing`, TR `(0, -4 * victim[+0x3E], 2 * radius)`, `0x40` frames; the hit, and the victim turns on the caster | `+= scalar << 6` |
/// | 4 (`0x801F7048`) | - | `+= scalar * 12` |
/// | 5 (`0x801F70A8`) | once the victim settles, every present party seat and live monster shown | - |
///
/// The phase chain and the beam itself are ported
/// ([`crate::cast_module_ticks::CROSS_BEAM_CHAIN`]); this director owns the
/// gates.
///
/// PORT: overlay_cast_cross_beam_0948_801f69f0 (PROT 0948; the camera arms, the charge records and the seat hides)
pub fn cross_beam_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (c, v) = (seats.caster, seats.victim);
    let mut arm = CaptureCamArm::default();
    let depth = i32::from(seats.depth_raw as i16);
    match phase {
        0 => {
            let facing = bearing_12bit_approx(v.z, v.x, c.z, c.x).wrapping_add(0x800) & 0xFFF;
            arm.caster_write = CaptureSeatWrite {
                facing: Some(facing),
                ..SHOWN
            };
            arm.others_write = HIDDEN;
            // `0x801F6C64..0x801F6CA8`.
            arm.vram_move = Some(super::GIMARD_BREATH_CLUT_MOVE);
            arm.shot = Some(ModuleShot {
                angles: [-0x20, 0x800_i16.wrapping_sub(facing as i16), 0],
                tr: [0, 0x600, (depth / 3) as i16],
                focus: focus_on(c),
                frames: 0x20,
            });
            st.countdown.0 = SPEED_SCALAR << 6;
        }
        1 => {
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            for (slot, record) in arm.charge_spawns.iter_mut().zip(CHARGE_RECORDS) {
                *slot = Some(seat(record, [0; 3], [0; 3]));
            }
            arm.shot = Some(ModuleShot {
                angles: [0, 0x800_i16.wrapping_sub(c.facing as i16), 0],
                tr: [0, 0x600, (depth / 2) as i16],
                focus: focus_on(c),
                frames: 0x20,
            });
            st.countdown.0 += SPEED_SCALAR * 0x60;
        }
        2 => {
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            arm.victim_write = SHOWN;
            arm.caster_write = HIDDEN;
            arm.shot = Some(ModuleShot {
                angles: [0x100, (c.facing as i16).wrapping_neg(), 0],
                tr: [0, 0, 0x1800],
                focus: focus_on(v),
                frames: 1,
            });
            st.countdown.add(1 << 7);
        }
        3 => {
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            let body_y = st.victim_body.map_or(v.y, |b| b.y);
            arm.shot = Some(ModuleShot {
                angles: [0x40, (c.facing as i16).wrapping_neg(), 0],
                tr: [
                    0,
                    (-i32::from(body_y) * 4) as i16,
                    (i32::from(seats.victim_radius) * 2) as i16,
                ],
                focus: focus_on(v),
                frames: 0x40,
            });
            st.countdown.add(1 << 6);
            let facing = bearing_12bit_approx(c.z, c.x, v.z, v.x).wrapping_add(0x800) & 0xFFF;
            arm.victim_write.facing = Some(facing);
        }
        4 => {
            if st.countdown.drain_above(0) {
                arm.hold = true;
            } else {
                st.countdown.0 += SPEED_SCALAR * 12;
            }
        }
        5 => {
            if st.countdown.0 > 0 && st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            // `0x801F7238`'s stage `0` with a bump, which cuts the long charge
            // clip still playing behind the hide, is the phase chain's
            // (`CROSS_BEAM_CHAIN` arm 5).
            arm.others_write = SHOWN;
        }
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::super::ModuleSeat;
    use super::*;

    fn seats() -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 928,
                y: 0,
                z: 81,
                facing: 0x400,
            },
            victim: ModuleSeat {
                x: 38,
                y: 0,
                z: 711,
                facing: 0,
            },
            depth_raw: 3000,
            victim_radius: 300,
            caster_scale: 0x1000,
            action: 0x58,
            ..Default::default()
        }
    }

    #[test]
    fn arm_one_seats_all_eight_records_on_its_pass_only() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = SPEED_SCALAR * 2;
        let held = cross_beam_camera(&mut st, 1, seats());
        assert!(held.hold);
        assert!(held.charge_spawns.iter().all(Option::is_none));
        let pass = cross_beam_camera(&mut st, 1, seats());
        assert_eq!(pass.charge_spawns.iter().flatten().count(), 8);
        assert_eq!(pass.shot.unwrap().tr, [0, 0x600, 1500]);
    }

    #[test]
    fn arm_two_cuts_to_the_victim_and_hides_the_caster() {
        let mut st = ModuleCamState::default();
        let a = cross_beam_camera(&mut st, 2, seats());
        assert_eq!(a.caster_write.render_flag, Some(0xFF));
        let shot = a.shot.unwrap();
        assert_eq!((shot.tr, shot.frames), ([0, 0, 0x1800], 1));
        assert_eq!(shot.focus, [-38, 0, -711]);
    }

    #[test]
    fn arm_three_sizes_its_shot_by_the_victim_body() {
        let mut st = ModuleCamState {
            victim_body: Some(ModuleSeat {
                x: 0,
                y: -100,
                z: 0,
                facing: 0,
            }),
            ..Default::default()
        };
        let a = cross_beam_camera(&mut st, 3, seats());
        assert_eq!(a.shot.unwrap().tr, [0, 400, 600]);
        assert!(a.victim_write.facing.is_some());
    }
}
