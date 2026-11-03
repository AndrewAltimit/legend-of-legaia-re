//! PROT 0964's **Element Change** body (`0x801F88EC`, action `0xAF`): the
//! camera arms and the party's hide.
//!
//! See `ghidra/scripts/funcs/overlay_cast_element_change_0964_801f88ec.txt`.

use super::{
    CaptureCamArm, CaptureDrift, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleShot,
    SPEED_SCALAR, focus_on, yaw_from,
};

/// PROT 0964's Element Change tick body.
pub const ELEMENT_CHANGE_BODY: u32 = 0x801F_88EC;

/// The module's countdown word (`lui 0x8020` / `-0x648C`).
pub const ELEMENT_CHANGE_COUNTDOWN: u32 = 0x801F_9B74;

/// **Element Change** (`beq v1,1` / `slti v1,2` / `beq v1,2`; caster
/// `actor_table[ctx + 0x13]`):
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 0 (`0x801F8990`) | a cut on the caster: pitch `0`, yaw `0x800 - facing`, TR `(0, 0x600, 0x600)`, one frame; the record and the element roll are the body's | seed `scalar << 8` |
/// | 1 (`0x801F8AC0`) | every pass TR z `+= k` and every party seat hidden (`+0x04 = 0`, `+0x21C = 0xFF`) | `+= scalar << 6` |
/// | 2 (`0x801F8B94`) | the wait; on its pass the body's exit | - |
///
/// PORT: overlay_cast_element_change_0964_801f88ec (PROT 0964 `0xAF`; the camera arms and the party's hide)
pub fn element_change_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let mut arm = CaptureCamArm::default();
    match phase {
        0 => {
            arm.shot = Some(ModuleShot {
                angles: [0, yaw_from(0x800, c.facing), 0],
                tr: [0, 0x600, 0x600],
                focus: focus_on(c),
                frames: 1,
            });
            st.countdown.0 = SPEED_SCALAR << 8;
        }
        1 => {
            arm.drift = Some(CaptureDrift {
                tr_z: SPEED_SCALAR as i16,
                ..Default::default()
            });
            arm.party_write = CaptureSeatWrite {
                colour: Some(0),
                render_flag: Some(0xFF),
                ..Default::default()
            };
            if st.countdown.drain_above(0) {
                arm.hold = true;
            } else {
                st.countdown.add(1 << 6);
            }
        }
        2 => {
            if st.countdown.drain_above(0) {
                arm.hold = true;
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
    fn the_hide_runs_on_held_passes() {
        let mut st = ModuleCamState::default();
        let open = element_change_camera(&mut st, 0, ModuleCamSeats::default());
        assert_eq!(open.shot.unwrap().tr, [0, 0x600, 0x600]);
        let held = element_change_camera(&mut st, 1, ModuleCamSeats::default());
        assert!(held.hold);
        assert_eq!(held.party_write.render_flag, Some(0xFF));
    }
}
