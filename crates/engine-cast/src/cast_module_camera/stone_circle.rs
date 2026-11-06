//! PROT 0941's **Stone Circle** body (`0x801F6A04`, action `0xB9`): the camera
//! arms, the party's hide and the records.
//!
//! See `ghidra/scripts/funcs/overlay_cast_steal_0941_801f6a04.txt`.

use super::v_windhash::seat;
use super::{
    CaptureCamArm, CaptureDrift, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleShot,
    SPEED_SCALAR, focus_on, yaw_from,
};

/// PROT 0941's `0xB9` tick body.
pub const STONE_CIRCLE_BODY: u32 = crate::cast_arm_ticks::STEAL_SWEEP_TICK;

/// The module's countdown word (`lui 0x8020` / `-0x7C14`).
pub const STONE_CIRCLE_COUNTDOWN: u32 = 0x801F_83EC;

/// Arm 0's record on the caster (`0x801F6B28`).
const CHARGE: u32 = 0x801F_8218;
/// Arm 1's three records, at each of the eight ring points (`0x801F6DD4..`).
pub const RING_RECORDS: [u32; 3] = [0x801F_7E70, 0x801F_7ED8, 0x801F_7F38];
/// Arm 2's six records at the origin (`0x801F6ECC..0x801F6F44`).
const BLAST: [u32; 6] = [
    0x801F_7FDC,
    0x801F_8020,
    0x801F_8070,
    0x801F_80C0,
    0x801F_8108,
    0x801F_8154,
];

/// **Stone Circle** (`sltiu 5` through the table at `0x801F69D8`; caster
/// `actor_table[ctx + 0x13]`):
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 0 (`0x801F6AB0`) | on the caster: pitch `0`, yaw `0x800 - facing`, TR `(0, 0x500, 0x600)`, `0xC` frames; the charge record | seed `scalar << 8` |
/// | 1 (`0x801F6B38`) | every pass TR z `+= k`, every party seat hidden; on its pass the pool halted, the party back, a cut: pitch `0x200`, TR `(0, 0, 0x2000)`; the ring | `+= scalar << 7` |
/// | 2 (`0x801F6E38`) | every pass TR z `+= 12k`, TR y `-= 2k`; on its pass six records and the hit | `+= scalar << 8` |
/// | 3 (`0x801F7154`) | on its pass the pool halted, TR `(0, 0x500, 0xC00)`, `0x40` frames | `+= scalar << 6` |
/// | 4 (`0x801F72A0`) | the wait; on its pass the body's exit | - |
///
/// The damage is `steal_sweep_tick`'s. Not ported: the shuffled `+0x54` each ring
/// record takes off its `rand() % 8` slot (the draws are made), and the row's
/// rate `2` on arm 3.
///
/// PORT: overlay_cast_steal_0941_801f6a04 (PROT 0941 `0xB9`; the camera arms, the hide and the records)
pub fn stone_circle_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let yaw = yaw_from(0x800, c.facing);
    let shot = |pitch: i16, y: i16, z: i16, frames: u16| ModuleShot {
        angles: [pitch, yaw, 0],
        tr: [0, y, z],
        focus: focus_on(c),
        frames,
    };
    let rot = [0, c.facing as i16, 0];
    let mut arm = CaptureCamArm::default();
    match phase {
        0 => {
            arm.shot = Some(shot(0, 0x500, 0x600, 0xC));
            arm.pass_spawns[0] = Some(seat(CHARGE, [c.x, c.y, c.z], rot));
            st.countdown.0 = SPEED_SCALAR << 8;
        }
        1 => {
            arm.drift = Some(CaptureDrift {
                tr_z: SPEED_SCALAR as i16,
                ..Default::default()
            });
            if st.countdown.drain_above(0) {
                arm.party_write = CaptureSeatWrite {
                    colour: Some(0),
                    render_flag: Some(0xFF),
                    ..Default::default()
                };
                arm.hold = true;
                return arm;
            }
            arm.party_write = CaptureSeatWrite {
                colour: Some(0x2008_0200),
                render_flag: Some(0),
                ..Default::default()
            };
            // The pass halts the part pool ahead of the cut and the ring
            // (`jal 0x80050E74` at `0x801F6BF0`): the charge's column goes.
            arm.flushes_pool = true;
            arm.shot = Some(shot(0x200, 0, 0x2000, 1));
            st.countdown.add(1 << 7);
            arm.ring = Some((RING_RECORDS, rot));
        }
        2 => {
            arm.drift = Some(CaptureDrift {
                tr_y: (-2 * SPEED_SCALAR) as i16,
                tr_z: (12 * SPEED_SCALAR) as i16,
                ..Default::default()
            });
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            for (slot, r) in arm.charge_spawns.iter_mut().zip(BLAST) {
                *slot = Some(seat(r, [0; 3], rot));
            }
            st.countdown.add(1 << 8);
        }
        3 => {
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            // ... and so does arm 3's (`jal 0x80050E74` at `0x801F71D0`).
            arm.flushes_pool = true;
            arm.shot = Some(shot(0, 0x500, 0xC00, 0x40));
            st.countdown.add(1 << 6);
        }
        4 if st.countdown.drain_above(0) => arm.hold = true,
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_party_hides_until_the_cut() {
        let mut st = ModuleCamState::default();
        stone_circle_camera(&mut st, 0, ModuleCamSeats::default());
        let held = stone_circle_camera(&mut st, 1, ModuleCamSeats::default());
        assert_eq!(held.party_write.render_flag, Some(0xFF));
        assert_eq!(held.party_write.colour, Some(0));
        st.countdown.0 = SPEED_SCALAR;
        let pass = stone_circle_camera(&mut st, 1, ModuleCamSeats::default());
        assert_eq!(pass.party_write.render_flag, Some(0));
        assert_eq!(pass.party_write.colour, Some(0x2008_0200));
        assert_eq!(pass.shot.unwrap().tr, [0, 0, 0x2000]);
        assert!(pass.ring.is_some());
    }

    /// Arms 1 and 3 halt the part pool on their pass, not while held
    /// (`0x801F6BF0`, `0x801F71D0`): the charge's column is gone once the
    /// ring rises.
    #[test]
    fn arms_one_and_three_halt_the_pool_on_their_pass() {
        for phase in [1u8, 3] {
            let mut st = ModuleCamState::default();
            st.countdown.0 = SPEED_SCALAR << 4;
            let held = stone_circle_camera(&mut st, phase, ModuleCamSeats::default());
            assert!(held.hold && !held.flushes_pool, "arm {phase} held");
            st.countdown.0 = 0;
            let pass = stone_circle_camera(&mut st, phase, ModuleCamSeats::default());
            assert!(!pass.hold && pass.flushes_pool, "arm {phase} pass");
        }
        let mut st = ModuleCamState::default();
        assert!(!stone_circle_camera(&mut st, 2, ModuleCamSeats::default()).flushes_pool);
    }
}
