//! PROT 0944's **Curse All** body (`0x801F7470`, action `0x53`): the camera
//! arms, the seat hides and the records, over
//! [`crate::cast_arm_ticks::guilty_cross_curse_tick`].
//!
//! See `ghidra/scripts/funcs/overlay_cast_guilty_cross_0944_801f7470.txt`.

use super::capture::{CaptureCamArm, CaptureSeatWrite};
use super::v_windhash::{HIDDEN, SHOWN, seat};
use super::{
    ModuleCamSeats, ModuleCamState, ModuleShot, SPEED_SCALAR, bearing_12bit_approx, focus_on,
    yaw_from,
};

/// PROT 0944's Curse All tick body.
pub const CURSE_ALL_BODY: u32 = crate::cast_arm_ticks::GUILTY_CROSS_CURSE_TICK;

/// Arm 1's record (`0x801F7774`) and arm 3's (`0x801F7C68`), each at
/// `(0, 0, 0x100)` on the zeroed stack trios.
pub const CURSE_ALL_RECORDS: [u32; 2] = [0x801F_7FD0, 0x801F_8038];

/// The tint arm 3 stores into each cursed seat's `+0x04` (`0x801F7B38`).
const CURSED_TINT: u32 = 0x3FF0_03FF;

/// **Curse All** (`sltiu 5` through the table at `0x801F69F0`; caster
/// `actor_table[ctx + 0x13]`, target code `s3 = caster[+0x1DD]`, word
/// `0x801F8360`):
///
/// | arm | camera and seats | word |
/// |---|---|---|
/// | 0 (`0x801F7524`) | the caster turned on its target (the row's centroid off `FUN_801DCEAC`), shown with flag `6`, every present party seat and live monster hidden; pitch `-0x20`, yaw `0x800 - facing`, TR `(0, 0x400, depth / 3)`, `0x20` frames | `= scalar << 5` |
/// | 1 (`0x801F7728`) | gate; a record; pitch `0`, the same yaw, TR `(0, 0x400, depth)`, `0x80` frames | `+= scalar * 0xA0` |
/// | 2 (`0x801F7848`) | gate; the caster hidden, the target row shown; on the row: yaw `-caster[+0x46]`, TR `(0, 0x500, 2 * extent)`, a cut | `+= scalar << 5` |
/// | 3 (`0x801F7AF4`) | gate; the row tinted and marked; a record; TR z `3 * extent`, `0x60` frames | `+= scalar << 7` |
/// | 4 (`0x801F7D58`) | gate; every live monster and present party seat shown; done | - |
///
/// The camera follows the row legs: PROT 0944's body reads a seat target at
/// `s3 < 7` through the victim's `+0x3E` and model radius (`0x801F79E8`), which the
/// disc's `0x53` never takes, so that leg frames the group too.
///
/// PORT: overlay_cast_guilty_cross_0944_801f7470 (PROT 0944 `0x53`; the camera arms, the seat hides and the records)
pub fn curse_all_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (c, v) = (seats.caster, seats.victim);
    let depth = i32::from(seats.depth_raw as i16);
    let aim = st.target_aim;
    let extent = aim.map_or(depth, |a| i32::from(a.extent));
    let focus = aim.map_or(focus_on(c), |a| [a.centroid_x, 0, a.centroid_z]);
    let at_back = [0, 0, 0x100];
    let mut arm = CaptureCamArm::default();
    let gate = |st: &mut ModuleCamState| st.countdown.drain_above(0);
    match phase {
        0 => {
            let onto = match aim {
                Some(a) => bearing_12bit_approx(
                    a.centroid_z.wrapping_neg(),
                    a.centroid_x.wrapping_neg(),
                    c.z,
                    c.x,
                ),
                None => bearing_12bit_approx(v.z, v.x, c.z, c.x),
            }
            .wrapping_add(0x800)
                & 0xFFF;
            arm.others_write = HIDDEN;
            arm.others_live_only = true;
            arm.caster_write = CaptureSeatWrite {
                facing: Some(onto),
                render_flag: Some(6),
                colour: SHOWN.colour,
                ..Default::default()
            };
            arm.shot = Some(ModuleShot {
                angles: [-0x20, yaw_from(0x800, onto), 0],
                tr: [0, 0x400, (depth / 3) as i16],
                focus: focus_on(c),
                frames: 0x20,
            });
            st.countdown.0 = SPEED_SCALAR << 5;
        }
        1 => {
            if gate(st) {
                arm.hold = true;
            } else {
                arm.pass_spawns[0] = Some(seat(CURSE_ALL_RECORDS[0], at_back, [0, 0, 0]));
                arm.shot = Some(ModuleShot {
                    angles: [0, yaw_from(0x800, c.facing), 0],
                    tr: [0, 0x400, depth as i16],
                    focus: focus_on(c),
                    frames: 0x80,
                });
                st.countdown.add(0xA0);
            }
        }
        2 => {
            if gate(st) {
                arm.hold = true;
            } else {
                arm.caster_write = HIDDEN;
                arm.party_write = SHOWN;
                arm.party_live_only = true;
                arm.shot = Some(ModuleShot {
                    angles: [0, yaw_from(0, c.facing), 0],
                    tr: [0, 0x500, (2 * extent) as i16],
                    focus,
                    frames: 1,
                });
                st.countdown.add(1 << 5);
            }
        }
        3 => {
            if gate(st) {
                arm.hold = true;
            } else {
                arm.party_write = CaptureSeatWrite {
                    colour: Some(CURSED_TINT),
                    ..Default::default()
                };
                arm.party_live_only = true;
                arm.pass_spawns[0] = Some(seat(CURSE_ALL_RECORDS[1], at_back, [0, 0, 0]));
                arm.shot = Some(ModuleShot {
                    angles: [0, yaw_from(0, c.facing), 0],
                    tr: [0, 0x500, (3 * extent) as i16],
                    focus,
                    frames: 0x60,
                });
                st.countdown.add(1 << 7);
            }
        }
        4 => {
            if gate(st) {
                arm.hold = true;
            } else {
                arm.others_write = SHOWN;
                arm.others_live_only = true;
            }
        }
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arm dwells the retail recording's game clock reads: `32`, `160`,
    /// `32`, `128` vsyncs.
    #[test]
    fn the_arms_dwell_retails_lengths() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats {
            action: 0x53,
            depth_raw: 0xC00,
            ..Default::default()
        };
        assert!(!curse_all_camera(&mut st, 0, seats).hold);
        for (phase, want) in [(1, 32), (2, 160), (3, 32), (4, 128)] {
            let mut n = 1;
            while curse_all_camera(&mut st, phase, seats).hold {
                n += 1;
            }
            assert_eq!(n, want, "arm {phase}");
        }
    }
}
