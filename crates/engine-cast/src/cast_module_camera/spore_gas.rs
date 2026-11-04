//! PROT 0939's **Spore Gas** body (`0x801F69D8`, action `0x4F`): the camera
//! arms and the seat swap. The four hits are [`crate::cast_module_ticks::SporeGasBeat`].
//!
//! See `ghidra/scripts/funcs/overlay_cast_spore_gas_0939_801f69d8.txt`.

use super::v_windhash::{HIDDEN, SHOWN};
use super::{
    CaptureCamArm, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleSeat, ModuleShot,
    SPEED_SCALAR, bearing_12bit_approx, focus_on, yaw_from,
};

/// The shot on the caster the first three arms build.
fn on_caster(c: ModuleSeat, pitch: i16, tr: [i16; 2], frames: u16) -> ModuleShot {
    ModuleShot {
        angles: [pitch, yaw_from(0x800, c.facing), 0],
        tr: [0, tr[0], tr[1]],
        focus: focus_on(c),
        frames,
    }
}

/// **Spore Gas** (`beq` / `slti` tree over `0..=5` and `0xFF`; caster
/// `actor_table[ctx + 0x13]`, victim `actor_table[caster + 0x1DD]`):
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 0 (`0x801F6AC4`) | pitch `-0x20`, yaw `0x800 - facing`, TR `(0, 0x600, 0x800)`, `0x80` frames | seed `scalar << 7` |
/// | 1 (`0x801F6BC8`) | once the gate passes: pitch `0x200`, TR `(0, 0x200, 0xC00)`, `0x50` frames | `+= scalar * 0x50` |
/// | 2 (`0x801F6E34`) | once the gate passes: pitch `-0x40`, TR `(0, 0x400, 0x1000)`, `0x40` frames | `+= scalar << 6` |
/// | 3 (`0x801F6ED4`) | once the gate passes: caster hidden, victim shown and turned on the caster; a cut on it: pitch `0`, yaw `0x800 - facing`, TR `(0, 0x500, 0x400)` | `+= scalar << 7` |
/// | 4 (`0x801F6FD0`) | the hits; leaves on the gate | - |
/// | `0xFF` (`0x801F746C`) | the caster shown again | - |
///
/// The phase chain is [`crate::cast_module_ticks::SPORE_GAS_CHAIN`]; this
/// director owns the gates.
///
/// PORT: overlay_cast_spore_gas_0939_801f69d8 (PROT 0939 `0x4F`; the camera arms and the seat swap)
pub fn spore_gas_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (c, v) = (seats.caster, seats.victim);
    let mut arm = CaptureCamArm::default();
    if (1..=4).contains(&phase) && st.countdown.drain_above(0) {
        arm.hold = true;
        return arm;
    }
    match phase {
        0 => {
            arm.shot = Some(on_caster(c, -0x20, [0x600, 0x800], 0x80));
            st.countdown.0 = SPEED_SCALAR << 7;
        }
        1 => {
            arm.shot = Some(on_caster(c, 0x200, [0x200, 0xC00], 0x50));
            st.countdown.0 += SPEED_SCALAR * 0x50;
        }
        2 => {
            arm.shot = Some(on_caster(c, -0x40, [0x400, 0x1000], 0x40));
            st.countdown.add(1 << 6);
        }
        3 => {
            let facing = bearing_12bit_approx(c.z, c.x, v.z, v.x).wrapping_add(0x800) & 0xFFF;
            arm.caster_write = HIDDEN;
            arm.victim_write = CaptureSeatWrite {
                facing: Some(facing),
                ..SHOWN
            };
            arm.shot = Some(ModuleShot {
                angles: [0, yaw_from(0x800, facing), 0],
                tr: [0, 0x500, 0x400],
                focus: focus_on(v),
                frames: 1,
            });
            st.countdown.add(1 << 7);
        }
        0xFF => arm.caster_write = SHOWN,
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seats() -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: -6,
                y: 0,
                z: 188,
                facing: 2184,
            },
            victim: ModuleSeat {
                x: 800,
                y: 0,
                z: 57,
                facing: 0,
            },
            ..Default::default()
        }
    }

    #[test]
    fn the_opening_shot_is_retails_table() {
        // `battle_teien_berserker_spore_gas_mid_cast`: pitch `-0x20`, TR y `0x600`, yaw
        // endpoint `920` on a heading of `1128`.
        let mut st = ModuleCamState::default();
        let mut s = seats();
        s.caster.facing = 1128;
        let shot = spore_gas_camera(&mut st, 0, s).shot.expect("a shot");
        assert_eq!(shot.angles[0], -0x20);
        assert_eq!(shot.angles[1], 920);
        assert_eq!(shot.tr, [0, 0x600, 0x800]);
        assert_eq!(shot.frames, 0x80);
    }

    #[test]
    fn arm_three_swaps_the_seats_on_the_gate() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = 1 << 20;
        let held = spore_gas_camera(&mut st, 3, seats());
        assert!(held.hold && held.shot.is_none());
        st.countdown.0 = 0;
        let arm = spore_gas_camera(&mut st, 3, seats());
        assert!(!arm.hold);
        assert_eq!(arm.caster_write.render_flag, Some(0xFF));
        assert_eq!(arm.victim_write.render_flag, Some(0));
        assert!(arm.victim_write.facing.is_some());
        assert_eq!(arm.shot.expect("the cut").frames, 1);
    }
}
