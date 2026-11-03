//! PROT 0955's **Void Accessories** body (`0x801F6A28`, action `0x73`): the
//! camera arms, the seat hides and the records, over
//! [`crate::cast_module_ticks::void_accessories_tick`].
//!
//! See `ghidra/scripts/funcs/overlay_cast_white_shield_0955_801f6a28.txt`.

use super::capture::{CaptureCamArm, CaptureDrift, CaptureSeatWrite};
use super::v_windhash::{HIDDEN, SHOWN, seat};
use super::{
    ModuleCamSeats, ModuleCamState, ModuleShot, SPEED_SCALAR, bearing_12bit_approx, focus_on,
    yaw_from,
};

/// PROT 0955's Void Accessories tick body.
pub const VOID_ACCESSORIES_BODY: u32 = crate::cast_module_ticks::VOID_ACCESSORIES_TICK;

/// The six records arm 0 seats on the caster (`0x801F6C24..0x801F6C9C`).
pub const VOID_RECORDS: [u32; 6] = [
    0x801F_9790,
    0x801F_980C,
    0x801F_9888,
    0x801F_9904,
    0x801F_9990,
    0x801F_9A0C,
];

/// **Void Accessories** (a `beq` / `slti` chain over `0..=4` and `0xFF`;
/// caster `actor_table[ctx + 0x13]`, victim `actor_table[caster + 0x1DD]`,
/// word `0x801F9D28`):
///
/// | arm | camera and seats | word |
/// |---|---|---|
/// | 0 (`0x801F6B0C`) | every live seat hidden, the caster shown; pitch `-0x20`, yaw `0x800 - facing`, TR `(0, 0x600, depth / 2)`, `0xC` frames; six records | `= scalar * 12` |
/// | 1 (`0x801F6CAC`) | gate; pitch `-0xA0`, the same yaw, TR `(0, 0x740, depth / 3)`, `0xA0` frames; the caster's flag `7` | `+= scalar * 0xA0` |
/// | 2 (`0x801F6D7C`) | gate; the victim turned on the caster and shown, the caster hidden; a cut on the victim: pitch `0`, yaw `0x800 - facing`, TR `(0, 0x300, 0x600)` | `+= scalar << 7` |
/// | 3 (`0x801F6E60`) | TR z `+scalar`; gate; the strip ([`crate::cast_module_ticks::void_accessories_tick`]) | `+= scalar << 7` |
/// | 4 (`0x801F7080`) | TR z `+scalar`; gate; every live seat shown as the body finishes | - |
///
/// PORT: overlay_cast_white_shield_0955_801f6a28 (PROT 0955 `0x73`; the camera arms, the seat hides and the records)
pub fn void_accessories_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (c, v) = (seats.caster, seats.victim);
    let depth = i32::from(seats.depth_raw as i16);
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    let pull = Some(CaptureDrift {
        tr_z: SPEED_SCALAR as i16,
        ..Default::default()
    });
    match phase {
        0 => {
            arm.others_write = HIDDEN;
            arm.others_live_only = true;
            arm.caster_write = SHOWN;
            arm.shot = Some(ModuleShot {
                angles: [-0x20, yaw_from(0x800, c.facing), 0],
                tr: [0, 0x600, (depth / 2) as i16],
                focus: focus_on(c),
                frames: 0xC,
            });
            let on = [c.x, c.y, c.z];
            let rot = [0, c.facing as i16, 0];
            for (i, r) in VOID_RECORDS.iter().enumerate() {
                arm.charge_spawns[i] = Some(seat(*r, on, rot));
            }
            st.countdown.0 = SPEED_SCALAR * 12;
        }
        1 => {
            if st.countdown.drain_above(0) {
                arm.hold = true;
            } else {
                arm.shot = Some(ModuleShot {
                    angles: [-0xA0, yaw_from(0x800, c.facing), 0],
                    tr: [0, 0x740, (depth / 3) as i16],
                    focus: focus_on(c),
                    frames: 0xA0,
                });
                arm.caster_write = CaptureSeatWrite {
                    render_flag: Some(7),
                    ..Default::default()
                };
                st.countdown.add(0xA0);
            }
        }
        2 => {
            if st.countdown.drain_above(0) {
                arm.hold = true;
            } else {
                let turned = bearing_12bit_approx(c.z, c.x, v.z, v.x).wrapping_add(0x800) & 0xFFF;
                arm.victim_write = CaptureSeatWrite {
                    facing: Some(turned),
                    ..SHOWN
                };
                arm.caster_write = HIDDEN;
                arm.shot = Some(ModuleShot {
                    angles: [0, yaw_from(0x800, turned), 0],
                    tr: [0, 0x300, 0x600],
                    focus: focus_on(v),
                    frames: 1,
                });
                st.countdown.add(1 << 7);
            }
        }
        3 => {
            arm.drift = pull;
            if st.countdown.drain_above(0) {
                arm.hold = true;
            } else {
                st.countdown.add(1 << 7);
            }
        }
        4 => {
            arm.drift = pull;
            if st.countdown.drain_above(0) {
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
    use super::super::ModuleSeat;
    use super::*;

    /// The arm dwells the retail recording's game clock reads: `12`, `160`,
    /// `128`, `128` vsyncs.
    #[test]
    fn the_arms_dwell_retails_lengths() {
        let seats = ModuleCamSeats {
            caster: ModuleSeat {
                x: 0,
                y: 0,
                z: 0,
                facing: 0,
            },
            action: 0x73,
            depth_raw: 0xC00,
            ..Default::default()
        };
        let mut st = ModuleCamState::default();
        assert!(void_accessories_camera(&mut st, 0, seats).charge_spawns[5].is_some());
        for (phase, want) in [(1, 12), (2, 160), (3, 128), (4, 128)] {
            let mut n = 1;
            while void_accessories_camera(&mut st, phase, seats).hold {
                n += 1;
            }
            assert_eq!(n, want, "arm {phase}");
        }
    }
}
