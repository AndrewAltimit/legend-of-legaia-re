//! PROT 0955's **Terror Scream** body (`0x801F767C`, action `0x70`): the
//! camera arms, the ring records and the seat hides.
//!
//! See `ghidra/scripts/funcs/overlay_cast_white_shield_0955_801f767c.txt`.

use super::v_windhash::{HIDDEN, SHOWN, seat};
use super::{
    CaptureCamArm, CaptureDrift, CaptureSeatWrite, MODULE_DRAIN_PER_TICK, ModuleCamSeats,
    ModuleCamState, ModuleSeat, ModuleShot, ModuleSpawn, SPEED_SCALAR, bearing_12bit_approx,
    focus_on, yaw_from,
};

/// PROT 0955's Terror Scream tick body.
pub const TERROR_SCREAM_BODY: u32 = crate::cast_module_ticks::TERROR_SCREAM_TICK;

/// The caster whose arms 0 and 1 run the far framing and the slow drift
/// (`li v0,0x7F`, `0x801F78C4` and `0x801F7A38`): Terror Face.
const TERROR_FACE: u8 = 0x7F;

/// The four ring records arms 1 and 2 seat on the seat they framed
/// (`0x801F7D2C..0x801F7D90`).
const RING_RECORDS: [u32; 4] = [0x801F_95DC, 0x801F_9630, 0x801F_9684, 0x801F_96D8];

fn ring(on: ModuleSeat) -> [Option<ModuleSpawn>; 5] {
    let rot = [0, on.facing as i16, 0];
    let pos = [on.x, 0, on.z];
    [
        Some(seat(RING_RECORDS[0], pos, rot)),
        Some(seat(RING_RECORDS[1], pos, rot)),
        Some(seat(RING_RECORDS[2], pos, rot)),
        Some(seat(RING_RECORDS[3], pos, rot)),
        None,
    ]
}

/// **Terror Scream** (a `beq` / `slti` chain over `0..=4` and `0xFF`; caster
/// `actor_table[ctx + 0x13]`, victim `actor_table[caster + 0x1DD]`):
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 0 (`0x801F7758`) | every live seat hidden, the caster shown; a shot on the caster's body: pitch `-0x80`, yaw `0x800 - facing`, TR `(0, 0x300, ctx[+0x6D0] / 2)` for Terror Face and yaw `0x680 - facing`, TR `(0, 0x280, ctx[+0x6D0] / 3)` otherwise, `0xC` frames | seed `scalar * 0x60` |
/// | 1 (`0x801F7A0C`) | every pass a drift: Terror Face pitch `+2 * delta`, TR y `+product / 2`, TR z `-product / 8`; otherwise pitch `+delta`, yaw `+product`, TR y `+product`. Once the gate passes, a cut on the caster: pitch `0x180`, yaw `0x800 - facing`, TR `(0, 0, ctx[+0x6D0])`, and four records on the caster | `+= scalar << 7` |
/// | 2 (`0x801F7C04`) | every pass TR z `+2 * product`. Once the gate passes, the victim turns on the caster; a cut on it: pitch `0x180`, yaw `0x800 - facing`, TR `(0, 0, 0xC00)`; the victim shown, the caster hidden, four records on the victim | `+= scalar * 0x60` |
/// | 3 (`0x801F7D98`) | the same drift; the steal is the body's | `+= scalar << 6` |
/// | 4 (`0x801F7E64`) | the same drift; the gate, then the victim's settle | - |
/// | `0xFF` (`0x801F7F28`) | every live seat shown | - |
///
/// PORT: overlay_cast_white_shield_0955_801f767c (PROT 0955 `0x70`; the camera arms, the ring and the seat hides)
pub fn terror_scream_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (c, v) = (seats.caster, seats.victim);
    let depth = i32::from(seats.depth_raw as i16);
    let far = seats.caster_monster == TERROR_FACE;
    let mut arm = CaptureCamArm {
        others_live_only: true,
        ..Default::default()
    };
    let push = CaptureDrift {
        tr_z: (MODULE_DRAIN_PER_TICK * 2) as i16,
        ..CaptureDrift::default()
    };
    match phase {
        0 => {
            arm.others_write = HIDDEN;
            arm.caster_write = SHOWN;
            let body = st.caster_body.unwrap_or(c);
            let (yaw_base, tr_y, tr_z) = if far {
                (0x800, 0x300, depth / 2)
            } else {
                (0x680, 0x280, depth / 3)
            };
            arm.shot = Some(ModuleShot {
                angles: [-0x80, yaw_from(yaw_base, c.facing), 0],
                tr: [0, tr_y, tr_z as i16],
                focus: focus_on(body),
                frames: 0xC,
            });
            st.countdown.0 = SPEED_SCALAR * 0x60;
        }
        1 => {
            arm.drift = Some(if far {
                CaptureDrift {
                    pitch: 2,
                    tr_y: (MODULE_DRAIN_PER_TICK / 2) as i16,
                    tr_z: -((MODULE_DRAIN_PER_TICK / 8) as i16),
                    ..CaptureDrift::default()
                }
            } else {
                CaptureDrift {
                    pitch: 1,
                    yaw: MODULE_DRAIN_PER_TICK as i16,
                    tr_y: MODULE_DRAIN_PER_TICK as i16,
                    ..CaptureDrift::default()
                }
            });
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            arm.shot = Some(ModuleShot {
                angles: [0x180, yaw_from(0x800, c.facing), 0],
                tr: [0, 0, depth as i16],
                focus: focus_on(c),
                frames: 1,
            });
            arm.pass_spawns = ring(c);
            st.countdown.add(1 << 7);
        }
        2 => {
            arm.drift = Some(push);
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            let facing = bearing_12bit_approx(c.z, c.x, v.z, v.x).wrapping_add(0x800) & 0xFFF;
            arm.victim_write = CaptureSeatWrite {
                facing: Some(facing),
                ..SHOWN
            };
            arm.caster_write = HIDDEN;
            arm.shot = Some(ModuleShot {
                angles: [0x180, yaw_from(0x800, facing), 0],
                tr: [0, 0, 0xC00],
                focus: focus_on(v),
                frames: 1,
            });
            arm.pass_spawns = ring(ModuleSeat { facing, ..v });
            st.countdown.0 += SPEED_SCALAR * 0x60;
        }
        3 => {
            arm.drift = Some(push);
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            st.countdown.add(1 << 6);
        }
        4 => {
            arm.drift = Some(push);
            if st.countdown.drain_above(0) || !st.victim_settled {
                arm.hold = true;
            }
        }
        0xFF => arm.others_write = SHOWN,
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seats(monster: u8) -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 900,
                y: 0,
                z: 100,
                facing: 0x400,
            },
            victim: ModuleSeat {
                x: 0,
                y: 0,
                z: 700,
                facing: 0,
            },
            caster_monster: monster,
            depth_raw: 3000,
            action: 0x70,
            ..Default::default()
        }
    }

    #[test]
    fn terror_face_frames_its_opening_further_out_than_the_others() {
        let mut st = ModuleCamState::default();
        let far = terror_scream_camera(&mut st, 0, seats(TERROR_FACE))
            .shot
            .unwrap();
        assert_eq!(far.tr, [0, 0x300, 1500]);
        let near = terror_scream_camera(&mut st, 0, seats(0x83)).shot.unwrap();
        assert_eq!(near.tr, [0, 0x280, 1000]);
    }

    #[test]
    fn arm_one_drifts_while_held_and_rings_the_caster_on_its_pass() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = SPEED_SCALAR * 2;
        let held = terror_scream_camera(&mut st, 1, seats(TERROR_FACE));
        assert!(held.hold);
        assert_eq!(held.drift.unwrap().pitch, 2);
        let pass = terror_scream_camera(&mut st, 1, seats(TERROR_FACE));
        assert!(!pass.hold);
        assert_eq!(pass.pass_spawns.iter().flatten().count(), 4);
        assert_eq!(pass.shot.unwrap().frames, 1);
    }

    #[test]
    fn arm_four_waits_for_the_victim() {
        let mut st = ModuleCamState::default();
        assert!(terror_scream_camera(&mut st, 4, seats(TERROR_FACE)).hold);
        st.victim_settled = true;
        assert!(!terror_scream_camera(&mut st, 4, seats(TERROR_FACE)).hold);
    }
}
