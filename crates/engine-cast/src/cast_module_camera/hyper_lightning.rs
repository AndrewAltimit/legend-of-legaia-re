//! PROT 0937's **Hyper Lightning** body (`0x801F69D8`, action `0x4C`): the
//! camera arms, the seat hides and the records. The phase chain is
//! [`crate::cast_module_ticks::HYPER_LIGHTNING_CHAIN`], its countdowns
//! [`super::capture_countdown::HYPER_LIGHTNING`].
//!
//! See `ghidra/scripts/funcs/overlay_cast_hyper_lightning_0937_801f69d8.txt`.

use super::hyper_crush::ahead;
use super::v_windhash::{HIDDEN, SHOWN, seat};
use super::{
    CaptureCamArm, CaptureDrift, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleShot,
    SPEED_SCALAR, focus_on, yaw_from,
};
use legaia_engine_system::trig::bearing12;

/// Arm 1's three records (`0x801F6D80..0x801F6DE0`).
const ARM1_RECORDS: [u32; 3] = [0x801F_79DC, 0x801F_7A2C, 0x801F_7A7C];
/// Arm 2's six records (`0x801F6F7C..0x801F7028`).
const ARM2_RECORDS: [u32; 6] = [
    0x801F_7B48,
    0x801F_7B94,
    0x801F_7BE0,
    0x801F_7C38,
    0x801F_7C90,
    0x801F_7CE8,
];
/// Arm 4's five records (`0x801F7170..0x801F7204`).
const ARM4_RECORDS: [u32; 5] = [
    0x801F_7E54,
    0x801F_7EA0,
    0x801F_7EEC,
    0x801F_7F44,
    0x801F_7F9C,
];
/// Arm 5's two records on the victim (`0x801F73F4..0x801F7464`).
const ARM5_RECORDS: [u32; 2] = [0x801F_7FE0, 0x801F_8038];

/// **Hyper Lightning** (a `beq` / `slti` tree over `0..=7` and `0xFF`; caster
/// `s3`, victim `s2`, module word `0x801F8090`). Arms 0 and 1 are Hyper
/// Crush's ([`super::hyper_crush_camera`]) on their own records:
///
/// | arm | camera | seats and records | countdown |
/// |---|---|---|---|
/// | 0 (`0x801F6AE0`) | the caster turned to the victim; shot: pitch `-0x40`, yaw `0x800 - facing`, TR `(-0x100, 0x500, 0x400)`, `0x40` frames | - | seed `scalar * 0x60` |
/// | 1 (`0x801F6BB8`) | every pass a `0x40`-frame shot off the word `w` | on the gate three records on the point `(104, 88)` off the caster, height `-0x258` | `+= scalar * 0x60` |
/// | 2 (`0x801F6E08`) | on the gate: pitch `-0x20`, yaw `0x900 - facing`, TR `(0x1C0, 0x480, 0x800)`, `0x20` frames | the party hidden; six records `600` ahead of the caster at height `-0x166` | `+= scalar << 6` |
/// | 3 (`0x801F7040`) | - | on the gate the caster's `+0x21C = 9` | `+= scalar << 5` |
/// | 4 (`0x801F7090`) | TR z `+4 s` | on the gate five records `600` ahead | `+= scalar << 7` |
/// | 5 (`0x801F7314`) | TR z `+4 s`; on the gate the victim turned to the caster and a cut on it: pitch `-0x20`, yaw `0xA80 - facing`, TR `(0, 0x480, 0x500)` | the victim shown, the caster hidden, two records on the victim | `+= scalar * 0x60` |
/// | 6 (`0x801F7500`) | TR z `+s` | - | the hit words cleared |
/// | 7 (`0x801F757C`) | TR z `+s` until the first hit, then a `0xC`-frame cut on the victim's display pair every pass (pitch `-0x20`, yaw `0xA80 - facing`, TR `(0, 0x480, 0x800)`) | each of the four hits flashes the victim and turns it to the caster | the hit word re-arms `scalar << 3` |
/// | `0xFF` (`0x801F77F8`) | - | the caster shown, `+0x21B` / `+0x176` cleared | - |
///
/// PORT: FUN_801F69D8 (PROT 0937 `0x4C`; the camera arms, the seat writes and the records)
pub fn hyper_lightning_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let s = SPEED_SCALAR as i16;
    let c = seats.caster;
    let v = seats.victim;
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    let push = |k: i16| CaptureDrift {
        tr_z: k,
        ..Default::default()
    };
    match phase {
        0 | 1 => {
            let crush = super::hyper_crush_camera(st, phase, seats);
            arm.shot = crush.shot;
            arm.hold = crush.hold;
            if phase == 1 && !arm.hold {
                let (x, z) = ahead(c, 104, 88);
                for (slot, &r) in arm.pass_spawns.iter_mut().zip(&ARM1_RECORDS) {
                    *slot = Some(seat(r, [x, -0x258, z], [0; 3]));
                }
            }
        }
        2 => {
            arm.hold = st.countdown.drain_above(0);
            if !arm.hold {
                arm.party_write = HIDDEN;
                arm.shot = Some(ModuleShot {
                    angles: [-0x20, yaw_from(0x900, c.facing), 0],
                    tr: [0x1C0, 0x480, 0x800],
                    focus: focus_on(c),
                    frames: 0x20,
                });
                let (x, z) = ahead(c, 600, 0);
                for (slot, &r) in arm.charge_spawns.iter_mut().zip(&ARM2_RECORDS) {
                    *slot = Some(seat(r, [x, -0x166, z], [0, c.facing as i16, 0]));
                }
                st.countdown.add(1 << 6);
            }
        }
        3 => {
            arm.hold = st.countdown.drain_above(0);
            if !arm.hold {
                arm.caster_write.render_flag = Some(9);
                st.countdown.add(1 << 5);
            }
        }
        4 | 5 => {
            arm.drift = Some(push(4 * s));
            arm.hold = st.countdown.drain_above(0);
            if arm.hold {
                return arm;
            }
            if phase == 4 {
                let (x, z) = ahead(c, 600, 0);
                for (slot, &r) in arm.pass_spawns.iter_mut().zip(&ARM4_RECORDS) {
                    *slot = Some(seat(r, [x, -0x166, z], [0, c.facing as i16, 0]));
                }
                st.countdown.add(1 << 7);
            } else {
                // `FUN_80019B28(caster z, caster x, victim z, victim x) + 0x800`.
                let facing = (bearing12(
                    i32::from(v.z) - i32::from(c.z),
                    i32::from(v.x) - i32::from(c.x),
                ) + 0x800)
                    & 0xFFF;
                arm.victim_write = CaptureSeatWrite {
                    facing: Some(facing),
                    ..SHOWN
                };
                arm.caster_write = HIDDEN;
                arm.shot = Some(ModuleShot {
                    angles: [-0x20, yaw_from(0xA80, facing), 0],
                    tr: [0, 0x480, 0x500],
                    focus: focus_on(v),
                    frames: 1,
                });
                let y = st.victim_body.map_or(0, |b| b.y);
                for (slot, &r) in arm.pass_spawns.iter_mut().zip(&ARM5_RECORDS) {
                    *slot = Some(seat(r, [v.x, y, v.z], [0, facing as i16, 0]));
                }
                st.countdown.add(0x60);
            }
        }
        6 => {
            arm.drift = Some(push(s));
            arm.hold = st.countdown.drain_above(0);
            if !arm.hold {
                // `0x801F7548..0x801F7558`: the hit word and the hit count.
                st.module_words = [0, 0];
            }
        }
        7 => {
            // `0x801F757C..0x801F77A0`. Once a hit has landed every pass
            // re-cuts to the victim's display pair; before it the arm only
            // pushes TR z.
            let hits = st.module_words[0];
            if hits != 0 {
                let body = st.victim_body.unwrap_or(v);
                arm.shot = Some(ModuleShot {
                    angles: [-0x20, yaw_from(0xA80, v.facing), 0],
                    tr: [0, 0x480, 0x800],
                    focus: focus_on(body),
                    frames: 0xC,
                });
            } else {
                arm.drift = Some(push(s));
            }
            // The arm's exit tests the count before the settle
            // (`bne v1,v0` on `4` at `0x801F77AC`): it holds until the
            // fourth hit has landed.
            arm.hold = hits < 4;
            if hits < 4 {
                st.module_words[1] -= SPEED_SCALAR;
                if st.module_words[1] <= 0 {
                    st.module_words[0] += 1;
                    arm.split_hit = Some(st.module_words[0] as u8);
                    st.module_words[1] += SPEED_SCALAR << 3;
                    // Each hit flashes the victim (`+0x04 = 0x200FFE00`,
                    // `+0x0C = 0x1000`) and turns it to the caster.
                    let facing = (bearing12(
                        i32::from(v.z) - i32::from(c.z),
                        i32::from(v.x) - i32::from(c.x),
                    ) + 0x800)
                        & 0xFFF;
                    arm.victim_write = CaptureSeatWrite {
                        colour: Some(0x200F_FE00),
                        blend: Some(0x1000),
                        facing: Some(facing),
                        ..Default::default()
                    };
                }
            }
        }
        // `0x801F77F8..0x801F7818`: the caster shown again, its clip's park
        // released.
        0xFF => {
            arm.caster_write = CaptureSeatWrite {
                loop_release: true,
                ..SHOWN
            };
        }
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_two_cuts_and_seats_six_records_ahead() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        st.countdown.0 = SPEED_SCALAR;
        let a = hyper_lightning_camera(&mut st, 2, seats);
        assert!(!a.hold);
        assert_eq!(a.shot.expect("the cut").tr, [0x1C0, 0x480, 0x800]);
        assert_eq!(a.charge_spawns.iter().flatten().count(), 6);
    }
}
