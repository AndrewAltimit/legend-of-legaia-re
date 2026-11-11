//! PROT 0935's **Earthquake** (`0x801F69D8`, action `0x4A`): the camera arms,
//! the charge records and the pass-by-pass rock burst.
//!
//! See `ghidra/scripts/funcs/overlay_cast_earthquake_0935_801f69d8.txt`.

use super::v_windhash::seat;
use super::{
    CaptureCamArm, CaptureDrift, CapturePoseSet, CaptureSeatWrite, ModuleCamSeats, ModuleCamState,
    ModuleShot, SPEED_SCALAR, bearing_12bit_approx, focus_on, yaw_from,
};
use legaia_engine_system::trig::trig12;

/// The module's countdown word (`lui 0x8020` / `-0x7D44`).
pub const EARTHQUAKE_COUNTDOWN: u32 = 0x801F_82BC;

/// Arm 1's records on each side of the caster, then the one behind it.
const SIDE_A: [u32; 3] = [0x801F_8028, 0x801F_8074, 0x801F_811C];
const SIDE_B: [u32; 3] = [0x801F_8028, 0x801F_8074, 0x801F_80C0];
const BEHIND: u32 = 0x801F_8178;
/// Arm 3's pair on the caster's flanks.
const FLANK: u32 = 0x801F_81CC;
/// The pair arms 4 and 5 burst from the ground each pass.
pub const ROCK_RECORDS: [u32; 2] = [0x801F_8238, 0x801F_826C];
/// The first formation monster whose records stand higher (Van Saryu).
const TALL_MONSTER: u8 = 0x89;
/// The caster's node frame arm 2 pins and arm 3 waits past.
const WIND_UP_FRAME: u16 = 0x160;
/// The node frame arm 2 pins each pass (`li v0,0x130` / `sh v0,0x68`).
const WIND_UP_PIN: u16 = 0x130;

/// `(s * n) / 4096`, a `sra 12` rounded toward zero as the arm's `bgez` does.
fn scaled(s: i16, n: i32) -> i32 {
    (i32::from(s) * n) / 0x1000
}

/// The point `along` units down the heading and `across` units over it, each a
/// table product rounded toward zero (`0x801F6C94..0x801F7368`).
fn off(x: i16, z: i16, facing: u16, along: i32, across: i32) -> (i16, i16) {
    let (s, co) = trig12(facing);
    let (sq, cq) = trig12(facing.wrapping_add(0x400) & 0xFFF);
    (
        (i32::from(x) + scaled(s, along) + scaled(sq, across)) as i16,
        (i32::from(z) + scaled(co, along) + scaled(cq, across)) as i16,
    )
}

/// **Earthquake** (a `beq` / `slti` chain over `0..=6, 0xFF`; caster
/// `actor_table[ctx + 0x13]`):
///
/// | arm | camera | gate |
/// |---|---|---|
/// | 0 (`0x801F6AE8`) | the caster turns on the target; pitch `0`, yaw `0x800 - facing`, TR `(0, 0x600, 0xA00)`, `0x10` frames | - |
/// | 1 (`0x801F6C20`) | the same shot every pass; on its pass the rate falls to `0`, `+0x21C = 9`, and seven records seat around the caster | the clip's loop counter `0xFF` |
/// | 2 (`0x801F6F78`) | the same shot; rate back to `8` on its pass | seed `scalar << 6` |
/// | 3 (`0x801F70BC`) | TR y `-= 24k`, TR z `+= 4k`, pitch `+= k >> 3`; on its pass two records on the flanks | the node frame `0x160`; `+= step << 8` |
/// | 4 (`0x801F737C`) | TR z `+= 8k`, pitch `+= k >> 3`, TR y alternates `0x380` / `0x400`; rocks each pass; on its pass pitch `0x80`, yaw `-facing`, TR `(0, 0x200, 0x1000)`, a cut | the word; `+= step << 8` |
/// | 5 (`0x801F7738`) | TR z `+= 4k`, TR y alternates `0x280` / `0x300`; rocks each pass | the word |
/// | 6 (`0x801F7D88`) | TR z `+= k`, TR y alternates; the target row's settle is the body's | - |
///
/// `k` is `scalar * step`, `scalar` per engine tick. The phase chain - clips,
/// the hit of arm 5 - is `EARTHQUAKE_CHAIN`. Not ported: the record-table words
/// `+0x85` / `+0x86` the arms write through the caster's monster record, the
/// pad rumble words. The 2D puff `FUN_801DFDF0(2, ..)` beside each rock pair is
/// the host's (`World::capture_stager_tick`, beside the rocks).
///
/// PORT: overlay_cast_earthquake_0935_801f69d8 (PROT 0935; the camera arms and the records)
pub fn earthquake_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let mut arm = CaptureCamArm::default();
    let step = i32::from(st.frame_step.max(1));
    let held_shot = |facing: u16| ModuleShot {
        angles: [0, yaw_from(0x800, facing), 0],
        tr: [0, 0x600, 0xA00],
        focus: focus_on(c),
        frames: 0x10,
    };
    // `0x801F7340`'s pass word: one step a battle frame, its low bit the shake.
    if st.opens_pass && (4..=6).contains(&phase) {
        st.ramp = (st.ramp + 1) & 3;
    }
    let w = st.ramp;
    match phase {
        0 => {
            let (tx, tz) = match (st.target_code, st.target_aim) {
                (Some(t), Some(aim)) if t >= 8 => (-aim.centroid_x, -aim.centroid_z),
                _ => (seats.victim.x, seats.victim.z),
            };
            let facing = bearing_12bit_approx(tz, tx, c.z, c.x).wrapping_add(0x800) & 0xFFF;
            arm.caster_write = CaptureSeatWrite {
                facing: Some(facing),
                ..Default::default()
            };
            arm.shot = Some(held_shot(facing));
        }
        1 => {
            arm.shot = Some(held_shot(c.facing));
            // `beq v1,0xFF` to the arm's busy exit (`0x801F6C78`): held until
            // the clip's window has run once.
            if st.caster_loops == Some(0xFF) {
                arm.hold = true;
                return arm;
            }
            arm.caster_write.render_flag = Some(9);
            st.countdown.0 = SPEED_SCALAR << 6;
            let y = if seats.first_monster == TALL_MONSTER {
                -0x280
            } else {
                -0x190
            };
            let rot = [0, c.facing as i16, 0];
            let (ax, az) = off(c.x, c.z, c.facing, 0x100, 288);
            let (bx, bz) = off(c.x, c.z, c.facing, 0x100, -288);
            let (cx, cz) = off(c.x, c.z, c.facing, -0x100, 0);
            for (slot, (r, p)) in arm.charge_spawns.iter_mut().zip([
                (SIDE_A[0], [ax, y, az]),
                (SIDE_A[1], [ax, y, az]),
                (SIDE_A[2], [ax, y, az]),
                (SIDE_B[0], [bx, y, bz]),
                (SIDE_B[1], [bx, y, bz]),
                (SIDE_B[2], [bx, y, bz]),
                (BEHIND, [cx, -0x1E0, cz]),
            ]) {
                *slot = Some(seat(r, p, rot));
            }
        }
        2 => {
            arm.shot = Some(held_shot(c.facing));
            arm.caster_write.seek = Some(WIND_UP_PIN);
            if st.countdown.drain_above(0) {
                arm.hold = true;
            }
        }
        3 => {
            arm.drift = Some(CaptureDrift {
                pitch: (SPEED_SCALAR >> 3) as i16,
                tr_y: (-24 * SPEED_SCALAR) as i16,
                tr_z: (4 * SPEED_SCALAR) as i16,
                ..Default::default()
            });
            if st.caster_frame.is_some_and(|f| f < WIND_UP_FRAME) {
                arm.hold = true;
                return arm;
            }
            st.ramp = 0;
            st.countdown.0 += step << 8;
            let rot = [0, c.facing as i16, 0];
            let (ax, az) = off(c.x, c.z, c.facing, 192, 384);
            let (bx, bz) = off(c.x, c.z, c.facing, 192, -384);
            arm.pass_spawns[0] = Some(seat(FLANK, [ax, 0, az], rot));
            arm.pass_spawns[1] = Some(seat(FLANK, [bx, 0, bz], rot));
        }
        4 => {
            arm.drift = Some(CaptureDrift {
                pitch: (SPEED_SCALAR >> 3) as i16,
                tr_z: (8 * SPEED_SCALAR) as i16,
                ..Default::default()
            });
            arm.pose_set = Some(CapturePoseSet {
                tr_y: Some(if w & 1 != 0 { 0x380 } else { 0x400 }),
                ..Default::default()
            });
            arm.rock_burst = Some(w as u8);
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            arm.shot = Some(ModuleShot {
                angles: [0x80, (c.facing as i16).wrapping_neg(), 0],
                tr: [0, 0x200, 0x1000],
                focus: focus_on(c),
                frames: 1,
            });
            st.countdown.0 += step << 8;
        }
        5 => {
            arm.drift = Some(CaptureDrift {
                tr_z: (4 * SPEED_SCALAR) as i16,
                ..Default::default()
            });
            arm.pose_set = Some(CapturePoseSet {
                tr_y: Some(if w & 1 != 0 { 0x280 } else { 0x300 }),
                ..Default::default()
            });
            arm.rock_burst = Some(w as u8);
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            arm.caster_write.render_flag = Some(0);
            arm.folds_hit = true;
        }
        6 => {
            arm.drift = Some(CaptureDrift {
                tr_z: SPEED_SCALAR as i16,
                ..Default::default()
            });
            arm.pose_set = Some(CapturePoseSet {
                tr_y: Some(if w & 1 != 0 { 0x280 } else { 0x300 }),
                ..Default::default()
            });
        }
        _ => {}
    }
    arm
}

/// One rock pass's ground point off two `rand() % 1023 + 0x100` draws, the
/// quadrant walking with the pass word (`0x801F7480..0x801F7624`): `0` both
/// up, `1` x down, `2` z down, `3` both down.
pub fn rock_point(c: (i16, i16), w: u8, dx: i32, dz: i32) -> (i16, i16) {
    let sx = if w & 1 != 0 { -1 } else { 1 };
    let sz = if w & 2 != 0 { -1 } else { 1 };
    (
        (i32::from(c.0) + sx * (dx + 0x100)) as i16,
        (i32::from(c.1) + sz * (dz + 0x100)) as i16,
    )
}

#[cfg(test)]
mod tests {
    use super::super::ModuleSeat;
    use super::*;

    fn seats() -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 100,
                y: 0,
                z: 800,
                facing: 0x800,
            },
            victim: ModuleSeat {
                x: 100,
                y: 0,
                z: -800,
                facing: 0,
            },
            ..Default::default()
        }
    }

    #[test]
    fn arm_one_holds_on_the_loop_counter_then_seats_seven() {
        let mut st = ModuleCamState {
            caster_loops: Some(0xFF),
            ..Default::default()
        };
        assert!(earthquake_camera(&mut st, 1, seats()).hold);
        st.caster_loops = Some(2);
        let a = earthquake_camera(&mut st, 1, seats());
        assert!(!a.hold);
        assert_eq!(a.charge_spawns.iter().flatten().count(), 7);
        assert_eq!(st.countdown.0, SPEED_SCALAR << 6);
    }

    #[test]
    fn arm_four_rearms_by_the_step_and_cuts_to_the_wide_shot() {
        let mut st = ModuleCamState {
            frame_step: 4,
            ..Default::default()
        };
        st.countdown.0 = SPEED_SCALAR;
        let a = earthquake_camera(&mut st, 4, seats());
        assert_eq!(a.shot.unwrap().tr, [0, 0x200, 0x1000]);
        assert_eq!(st.countdown.0, 4 << 8);
    }

    #[test]
    fn the_rock_quadrant_walks_with_the_pass_word() {
        assert_eq!(rock_point((0, 0), 0, 10, 20), (266, 276));
        assert_eq!(rock_point((0, 0), 1, 10, 20), (-266, 276));
        assert_eq!(rock_point((0, 0), 2, 10, 20), (266, -276));
        assert_eq!(rock_point((0, 0), 3, 10, 20), (-266, -276));
    }
}
