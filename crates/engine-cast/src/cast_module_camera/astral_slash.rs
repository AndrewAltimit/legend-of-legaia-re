//! PROT 0952's **Astral Slash** body (`0x801F6A0C`, action `0xB8`): the camera
//! arms and the seat pins of the strike.
//!
//! See `ghidra/scripts/funcs/overlay_cast_bloody_horns_0952_801f6a0c.txt`. The
//! phase chain, clip staging and rate stores are
//! [`crate::cast_module_ticks::astral_slash_tick`].

use super::v_windhash::{HIDDEN, SHOWN};
use super::{
    CaptureCamArm, ModuleCamSeats, ModuleCamState, ModuleSeat, ModuleShot, SPEED_SCALAR,
    bearing_12bit_approx, focus_on,
};

/// PROT 0952's Astral Slash tick body.
pub const ASTRAL_SLASH_BODY: u32 = crate::cast_module_ticks::ASTRAL_SLASH_TICK;

/// The victim's pair as arm 0 saved it (`0x801F7BB0`), then the caster's as
/// arm 2 saved it (`0x801F7BB8`): `[cx, cz, vx, vz]` in
/// [`ModuleCamState::horns_pin`].
fn save_victim(st: &mut ModuleCamState, v: ModuleSeat) {
    if st.pin_saved & 1 == 0 {
        st.horns_pin[2] = v.x;
        st.horns_pin[3] = v.z;
        st.pin_saved |= 1;
    }
}

/// **Astral Slash** (table `0x801F69D8`; caster `actor_table[ctx + 0x13]`,
/// victim `actor_table[caster + 0x1DD]`):
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 0, 1 (`0x801F6AC4`, `0x801F6C28`) | the walk into reach, framed by case 6; the victim held on arm 0's saved pair | - |
/// | 2 (`0x801F6D08`) | the caster's pair saved, the caster turned onto the victim's body; on the caster: pitch `0x40`, yaw `0xA00 - facing`, TR `(0, 0x500, 0x600)`, `0xC` frames (`0x801F6DE8`) | `= scalar << 6` (`0x801F6E08`) |
/// | 3 (`0x801F6E98`) | every pass, both pairs written back and the staged clip cleared (`0x801F6EA4`); on the caster: yaw `cd / 8 + 0xBC0 - facing`, TR z `0x700 - cd / 2` (`0x801F6F8C`) | drained, `bgtz` holds; on the gate `+= scalar << 8` |
/// | 4 (`0x801F6FF4`) | every pass, the caster's pair written back; on the victim's body pair: yaw `cd / 32 + 0xB80 - facing`, TR z `0x900 - cd / 8` (`0x801F70BC`) | drained, ends the module on the gate |
///
/// Every shot is pitch `0x40`, TR y `0x500`, `0xC` frames.
///
/// PORT: overlay_cast_bloody_horns_0952_801f6a0c (PROT 0952 `0xB8`; the camera arms and the pair write-backs)
pub fn astral_slash_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (c, v) = (seats.caster, seats.victim);
    let mut arm = CaptureCamArm::default();
    let shot =
        |facing: u16, base: i32, yaw_cd: i32, far: i32, z_cd: i32, on: ModuleSeat| ModuleShot {
            angles: [0x40, (yaw_cd + base - i32::from(facing as i16)) as i16, 0],
            tr: [0, 0x500, (far - z_cd) as i16],
            focus: focus_on(on),
            frames: 0xC,
        };
    if phase == 0 {
        // The CLUT row copy every pass of the walk (`0x801F6BE8..0x801F6C18`).
        arm.vram_move = Some(super::GIMARD_BREATH_CLUT_MOVE);
    }
    if phase <= 1 {
        // `0x801F6B0C..0x801F6B30` saves it on arm 0, and both walk arms write
        // it back (`0x801F6CDC..0x801F6CFC`).
        save_victim(st, v);
        arm.victim_write.x = Some(st.horns_pin[2]);
        arm.victim_write.z = Some(st.horns_pin[3]);
        return arm;
    }
    match phase {
        2 => {
            save_victim(st, v);
            arm.victim_write.x = Some(st.horns_pin[2]);
            arm.victim_write.z = Some(st.horns_pin[3]);
            st.horns_pin[0] = c.x;
            st.horns_pin[1] = c.z;
            st.pin_saved |= 2;
            // `0x801F6D58..0x801F6D80`: the bearing off the victim's body pair.
            let on = st.victim_body.unwrap_or(v);
            let facing = bearing_12bit_approx(on.z, on.x, c.z, c.x).wrapping_add(0x800) & 0xFFF;
            arm.caster_write.facing = Some(facing);
            arm.shot = Some(shot(facing, 0xA00, 0, 0x600, 0, c));
            st.countdown.0 = SPEED_SCALAR << 6;
            // The party row hidden, then the victim shown over it
            // (`0x801F6E14..0x801F6E6C`).
            arm.party_write = HIDDEN;
            arm.party_skip_victim = true;
            arm.victim_write.colour = SHOWN.colour;
            arm.victim_write.render_flag = SHOWN.render_flag;
        }
        3 => {
            let [cx, cz, vx, vz] = st.horns_pin;
            arm.caster_write.x = Some(cx);
            arm.caster_write.z = Some(cz);
            arm.victim_write.x = Some(vx);
            arm.victim_write.z = Some(vz);
            arm.caster_write.stage = Some(0);
            let held = st.countdown.drain_above(0);
            let cd = st.countdown.0;
            let pinned = ModuleSeat { x: cx, z: cz, ..c };
            arm.shot = Some(shot(c.facing, 0xBC0, cd / 8, 0x700, cd / 2, pinned));
            if held {
                arm.hold = true;
            } else {
                st.countdown.add(1 << 8);
            }
        }
        4 => {
            let [cx, cz, ..] = st.horns_pin;
            arm.caster_write.x = Some(cx);
            arm.caster_write.z = Some(cz);
            let held = st.countdown.drain_above(0);
            let cd = st.countdown.0;
            let on = st.victim_body.unwrap_or(v);
            arm.shot = Some(shot(c.facing, 0xB80, cd / 32, 0x900, cd / 8, on));
            arm.hold = held;
        }
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
                x: 388,
                y: 0,
                z: 575,
                facing: 0x600,
            },
            victim: ModuleSeat {
                x: 104,
                y: 0,
                z: 404,
                facing: 0,
            },
            ..Default::default()
        }
    }

    #[test]
    fn the_arms_hold_for_their_seeds() {
        let mut st = ModuleCamState::default();
        astral_slash_camera(&mut st, 2, seats());
        let mut left = 0;
        while astral_slash_camera(&mut st, 3, seats()).hold {
            left += 1;
        }
        assert_eq!(left, 63, "scalar << 6 at the product's drain");
        let mut left = 0;
        while astral_slash_camera(&mut st, 4, seats()).hold {
            left += 1;
        }
        assert_eq!(left, 255, "scalar << 8 at the product's drain");
    }

    #[test]
    fn arm_three_pulls_in_on_the_saved_caster() {
        let mut st = ModuleCamState::default();
        astral_slash_camera(&mut st, 2, seats());
        let a = astral_slash_camera(&mut st, 3, seats());
        let cd = st.countdown.0;
        let shot = a.shot.expect("arm 3 re-aims every pass");
        assert_eq!(shot.angles[0], 0x40);
        assert_eq!(i32::from(shot.angles[1]), cd / 8 + 0xBC0 - 0x600);
        assert_eq!(i32::from(shot.tr[2]), 0x700 - cd / 2);
        assert_eq!(shot.focus, focus_on(seats().caster));
        assert_eq!(a.caster_write.stage, Some(0));
    }

    #[test]
    fn arm_four_frames_the_victim_on_the_retail_constants() {
        let mut st = ModuleCamState::default();
        astral_slash_camera(&mut st, 2, seats());
        while astral_slash_camera(&mut st, 3, seats()).hold {}
        let a = astral_slash_camera(&mut st, 4, seats());
        let cd = st.countdown.0;
        let shot = a.shot.expect("arm 4 re-aims every pass");
        assert_eq!(i32::from(shot.angles[1]), cd / 32 + 0xB80 - 0x600);
        assert_eq!(i32::from(shot.tr[2]), 0x900 - cd / 8);
        assert_eq!(shot.focus, focus_on(seats().victim));
        assert!(a.hold);
    }
}
