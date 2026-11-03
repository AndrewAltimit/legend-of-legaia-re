//! PROT 0942's **Power Up** body (`0x801F7D34`, action `0x52`): the camera
//! arms and the charge record. The message bar is the presentation decode's.
//!
//! See `ghidra/scripts/funcs/overlay_cast_power_up_0942_801f7d34.txt`.

use super::v_windhash::seat;
use super::{
    CaptureCamArm, ModuleCamSeats, ModuleCamState, ModuleShot, SPEED_SCALAR, focus_on, yaw_from,
};

/// PROT 0942's `0x52` tick body.
pub const POWER_UP_BODY: u32 = crate::cast_module_ticks::POWER_UP_TICK;

/// The module's countdown word (`lui 0x8020` / `-0x774C`).
pub const POWER_UP_COUNTDOWN: u32 = 0x801F_88B4;

/// The charge record arm 0 seats on the caster (`0x801F7DE4`).
const CHARGE_RECORD: u32 = 0x801F_81B8;

/// **Power Up** (a `beq` / `slti` chain over `0..=3`; caster
/// `actor_table[ctx + 0x13]`):
///
/// | arm | camera | countdown |
/// |---|---|---|
/// | 0 (`0x801F7DD4`) | the charge record on the caster; shot: pitch `-0x20`, yaw `0x800 - facing`, TR `(0, 0x400, ctx[+0x6D0] / 2)`, `0xC` frames | seed `scalar * 12` |
/// | 1 (`0x801F7EB8`) | once the gate passes: pitch `0x80`, TR z `ctx[+0x6D0]`, `0x100` frames; the caster's `+0x21C = 7` is the body's | `+= scalar << 8` |
/// | 2 (`0x801F7F5C`) | once the gate passes: pitch `0`, the same TR, `0x40` frames | `+= scalar * 0x60` |
/// | 3 (`0x801F8010`) | once the gate passes, the buff (the body's) | - |
///
/// PORT: overlay_cast_power_up_0942_801f7d34 (PROT 0942 `0x52`; the camera arms and the charge record)
pub fn power_up_camera(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> CaptureCamArm {
    let c = seats.caster;
    let depth = i32::from(seats.depth_raw as i16);
    let on_caster = |pitch: i16, tr_z: i32, frames: u16| ModuleShot {
        angles: [pitch, yaw_from(0x800, c.facing), 0],
        tr: [0, 0x400, tr_z as i16],
        focus: focus_on(c),
        frames,
    };
    let mut arm = CaptureCamArm::default();
    if phase == 0 {
        arm.pass_spawns[0] = Some(seat(
            CHARGE_RECORD,
            [c.x, c.y, c.z],
            [0, c.facing as i16, 0],
        ));
        arm.shot = Some(on_caster(-0x20, depth / 2, 0xC));
        st.countdown.0 = SPEED_SCALAR * 12;
        return arm;
    }
    if !(1..=3).contains(&phase) {
        return arm;
    }
    if st.countdown.drain_above(0) {
        arm.hold = true;
        return arm;
    }
    match phase {
        1 => {
            arm.shot = Some(on_caster(0x80, depth, 0x100));
            st.countdown.add(1 << 8);
        }
        2 => {
            arm.shot = Some(on_caster(0, depth, 0x40));
            st.countdown.0 += SPEED_SCALAR * 0x60;
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
                x: 900,
                y: 0,
                z: 100,
                facing: 0x400,
            },
            depth_raw: 3000,
            action: 0x52,
            ..Default::default()
        }
    }

    #[test]
    fn arm_zero_seats_the_charge() {
        let mut st = ModuleCamState::default();
        let a = power_up_camera(&mut st, 0, seats());
        assert_eq!(a.pass_spawns.iter().flatten().count(), 1);
        assert_eq!(a.shot.unwrap().tr, [0, 0x400, 1500]);
        assert_eq!(st.countdown.0, SPEED_SCALAR * 12);
    }

    #[test]
    fn arm_one_holds_twelve_ticks_then_pulls_back_slowly() {
        let mut st = ModuleCamState::default();
        power_up_camera(&mut st, 0, seats());
        for _ in 0..11 {
            assert!(power_up_camera(&mut st, 1, seats()).hold);
        }
        let a = power_up_camera(&mut st, 1, seats());
        assert!(!a.hold);
        let shot = a.shot.unwrap();
        assert_eq!(
            (shot.angles[0], shot.tr[2], shot.frames),
            (0x80, 3000, 0x100)
        );
    }
}
