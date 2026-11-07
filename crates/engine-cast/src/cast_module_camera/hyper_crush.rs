//! PROT 0936's **Hyper Crush** body (`0x801F69D8`, action `0x4B`): the camera
//! arms, the records, and arm 4's six-pass hold. The phase chain is
//! [`crate::cast_module_ticks::HYPER_CRUSH_CHAIN`], its countdowns
//! [`super::capture_countdown::HYPER_CRUSH`].
//!
//! See `ghidra/scripts/funcs/overlay_cast_hyper_crush_0936_801f69d8.txt`.

use super::v_windhash::seat;
use super::{
    CaptureCamArm, CaptureDrift, CapturePoseSet, ModuleCamSeats, ModuleCamState, ModuleSeat,
    ModuleShot, SPEED_SCALAR, focus_on, yaw_from,
};
use legaia_engine_system::trig::trig12;

/// Arm 1's three records (`0x801F6DF0..0x801F6E48`).
const ARM1_RECORDS: [u32; 3] = [0x801F_7CCC, 0x801F_7D1C, 0x801F_7D6C];

/// Arm 3's eight records (`0x801F7088..0x801F7150`).
const ARM3_RECORDS: [u32; 8] = [
    0x801F_7E38,
    0x801F_7E8C,
    0x801F_7EEC,
    0x801F_7F38,
    0x801F_7F84,
    0x801F_7FF4,
    0x801F_8064,
    0x801F_80F4,
];

/// Arm 4 leaves once its pass counter `0x801F817C` reaches this (`0x801F72D4`).
const ARM4_PASSES: i32 = 6;

/// `(t * k) / 4096` toward zero, the `bgez; addiu 0xFFF; sra 12` idiom.
fn scaled(t: i16, k: i32) -> i32 {
    i32::from(t) * k / 4096
}

/// The point `fore` units ahead of the caster along its facing and `side`
/// units across it, off the sine view `0x8007B81C` on `x` and the cosine view
/// `0x8007B7F8` on `z`, each read at the facing and at the facing plus
/// `0x400`: `x + fore sin - side cos`, `z + fore cos + side sin`, each term
/// rounded toward zero.
pub(super) fn ahead(c: ModuleSeat, fore: i32, side: i32) -> (i16, i16) {
    let (sin, cos) = trig12(c.facing & 0xFFF);
    let (sin_q, cos_q) = trig12(c.facing.wrapping_add(0x400) & 0xFFF);
    (
        (i32::from(c.x) + scaled(sin, fore) - scaled(sin_q, side)) as i16,
        (i32::from(c.z) + scaled(cos, fore) - scaled(cos_q, side)) as i16,
    )
}

/// **Hyper Crush** (a `beq` / `slti` tree over `0..=8` and `0xFF`; caster
/// `actor_table[ctx + 0x13]`, module word `0x801F8174`):
///
/// | arm | camera | records | countdown |
/// |---|---|---|---|
/// | 0 (`0x801F6B14`) | shot: pitch `-0x40`, yaw `0x800 - facing`, TR `(-0x100, 0x500, 0x400)`, `0x40` frames | - | seed `scalar * 0x60` |
/// | 1 (`0x801F6C20`) | every pass a `0x40`-frame shot off the word `w`: yaw `0x800 - facing + scalar * 32 - w / 4`, TR `(-0x100, scalar << 7 - (w - 0x500), w / 4 + 0x300)` | on the gate, three on the point `(104, 88)` off the caster, height `-0x258` | `+= scalar * 0x60` |
/// | 2 (`0x801F6E70`) | on the gate: pitch `0xC0`, yaw `0xF00 - facing`, TR `(0x800, 0x500, 0x800)`, `0x18` frames | - | `+= scalar * 0x18` |
/// | 3 (`0x801F6F30`) | - | on the gate, eight on the point `(449, 134)` off the caster, on its facing | `+= scalar << 6` |
/// | 4 (`0x801F7228`) | TR y `0x580` on odd passes, `0x500` on even | - | drained untested; the arm leaves on its sixth pass |
/// | 5 (`0x801F72F8`) | on the gate: pitch `0x80`, yaw `-facing`, TR `(0, 0x400, 0x800)` on the arm-3 point, `a3 = 1` | - | `+= scalar * 0xC0` |
/// | 6 (`0x801F7614`) | TR z `+4 s` a pass | - | `+= scalar << 6` |
/// | 7 (`0x801F7A54`) | TR z `+2 s`, pitch `-s / 4` | - | - |
/// | 8 (`0x801F7AE4`) | TR z `+s` | - | - |
///
/// PORT: FUN_801F69D8 (PROT 0936 `0x4B`; the camera arms, the records and arm 4's hold)
pub fn hyper_crush_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let s = SPEED_SCALAR as i16;
    let c = seats.caster;
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    let tr_z = |k: i16, pitch: i16| CaptureDrift {
        tr_z: k,
        pitch,
        ..Default::default()
    };
    match phase {
        0 => {
            st.countdown.0 = SPEED_SCALAR * 0x60;
            arm.shot = Some(ModuleShot {
                angles: [-0x40, yaw_from(0x800, c.facing), 0],
                tr: [-0x100, 0x500, 0x400],
                focus: focus_on(c),
                frames: 0x40,
            });
        }
        1 => {
            arm.hold = st.countdown.drain_above(0);
            let w = st.countdown.0;
            let quarter = w / 4;
            arm.shot = Some(ModuleShot {
                angles: [
                    -0x40,
                    (yaw_from(0x800, c.facing) as i32 + SPEED_SCALAR * 32 - quarter) as i16,
                    0,
                ],
                tr: [
                    -0x100,
                    ((SPEED_SCALAR << 7) - (w - 0x500)) as i16,
                    (quarter + 0x300) as i16,
                ],
                focus: focus_on(c),
                frames: 0x40,
            });
            if !arm.hold {
                let (x, z) = ahead(c, 104, 88);
                for (slot, &r) in arm.pass_spawns.iter_mut().zip(&ARM1_RECORDS) {
                    *slot = Some(seat(r, [x, -0x258, z], [0; 3]));
                }
                st.countdown.add(0x60);
            }
        }
        2 => {
            arm.hold = st.countdown.drain_above(0);
            if !arm.hold {
                arm.shot = Some(ModuleShot {
                    angles: [0xC0, yaw_from(0xF00, c.facing), 0],
                    tr: [0x800, 0x500, 0x800],
                    focus: focus_on(c),
                    frames: 0x18,
                });
                st.countdown.add(0x18);
            }
        }
        3 => {
            arm.hold = st.countdown.drain_above(0);
            if !arm.hold {
                st.module_words[0] = 0;
                let (x, z) = ahead(c, 449, 134);
                for (slot, &r) in arm.charge_spawns.iter_mut().zip(&ARM3_RECORDS) {
                    *slot = Some(seat(r, [x, 0, z], [0, c.facing as i16, 0]));
                }
                st.countdown.add(1 << 6);
            }
        }
        4 => {
            st.countdown.drain();
            st.module_words[0] += 1;
            let n = st.module_words[0];
            arm.pose_set = Some(CapturePoseSet {
                tr_y: Some(if n & 1 != 0 { 0x580 } else { 0x500 }),
                ..Default::default()
            });
            arm.hold = n != ARM4_PASSES;
        }
        5 => {
            arm.hold = st.countdown.drain_above(0);
            if !arm.hold {
                let (x, z) = ahead(c, 449, 134);
                arm.shot = Some(ModuleShot {
                    angles: [0x80, yaw_from(0, c.facing), 0],
                    tr: [0, 0x400, 0x800],
                    focus: [x.wrapping_neg(), 0, z.wrapping_neg()],
                    frames: 1,
                });
                st.countdown.add(0xC0);
            }
        }
        6..=8 => {
            arm.drift = Some(match phase {
                6 => tr_z(4 * s, 0),
                7 => tr_z(2 * s, -(s >> 2)),
                _ => tr_z(s, 0),
            });
            if phase < 8 {
                arm.hold = st.countdown.drain_above(0);
                if !arm.hold && phase == 6 {
                    st.countdown.add(1 << 6);
                }
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
    fn arm_four_holds_six_passes_and_shakes_the_camera() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        let mut passes = 0;
        loop {
            passes += 1;
            let a = hyper_crush_camera(&mut st, 4, seats);
            let y = a.pose_set.expect("a shake").tr_y.unwrap();
            assert_eq!(y, if passes % 2 == 1 { 0x580 } else { 0x500 });
            if !a.hold {
                break;
            }
        }
        assert_eq!(passes, 6);
    }

    #[test]
    fn the_arm_three_point_is_ahead_of_the_caster() {
        let c = ModuleSeat {
            x: 100,
            y: 0,
            z: 200,
            facing: 0,
        };
        // facing 0: sin 0, cos 0x1000; at 0x400 sin 0x1000, cos 0.
        assert_eq!(ahead(c, 449, 134), (100 - 134, 200 + 449));
    }
}
