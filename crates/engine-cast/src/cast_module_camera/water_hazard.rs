//! PROT 0956's **Water Hazard** body (`0x801F7298`, action `0x71`): the camera arms,
//! the seat writes and the record calls.
//!
//! See `ghidra/scripts/funcs/overlay_cast_water_hazard_0956_801f7298.txt`.

use super::v_windhash::{HIDDEN, SHOWN};
use super::{
    CaptureCamArm, CaptureDrift, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleShot,
    ModuleSpawn, SPEED_SCALAR, SpawnAnchor, SpawnRecord, bearing_12bit_approx, focus_on, yaw_from,
};
use legaia_engine_system::trig::trig12;

const fn on_caster(record: u32) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor: SpawnAnchor::Caster { y: 0 },
    }
}

/// Arm 0's three calls on the caster's `+0x34` / `+0x44` (`0x801F74D8..0x801F7508`).
pub(super) const ARM0_SPAWNS: [ModuleSpawn; 3] = [
    on_caster(0x801F_7FC8),
    on_caster(0x801F_8024),
    on_caster(0x801F_809C),
];
/// Arm 2's last call on the caster (`0x801F7850`), after the ring's.
pub(super) const ARM2_SPAWNS: [ModuleSpawn; 1] = [on_caster(0x801F_8218)];

/// One step of Water Hazard's ring: arm 2's two calls at each of twelve bearings,
/// a third and a sixth of `sin` / `cos` off the caster's ground, on the caster's
/// `+0x44` (`0x801F7704..0x801F7838`). The body's only spawn off the seat's bearing.
pub fn water_hazard_ring(ground: (i16, i16), facing: i16) -> Vec<ModuleSpawn> {
    let (x0, z0) = ground;
    (0..12i32)
        .flat_map(|i| {
            let (sin, cos) = trig12((i * 0x1000 / 12) as u16);
            [(3, 0x801F_8168), (6, 0x801F_8110)].map(|(div, record)| ModuleSpawn {
                record: SpawnRecord::Module(record),
                anchor: SpawnAnchor::LiteralScaled {
                    pos: [x0.wrapping_add(sin / div), 0, z0.wrapping_add(cos / div)],
                    rot: [0, facing, 0],
                    scale: 0x1000,
                },
            })
        })
        .collect()
}

/// **Water Hazard** (a `beq` tree over `0..=3` and `0xFF`):
///
/// | arm | drift | on the gate | countdown |
/// |---|---|---|---|
/// | 0 (`0x801F738C`) | - | the caster turned on its target (`+0x46`), the living seats hidden, the caster shown with `+0x21C = 9`; yaw `0x800 - facing` off the new heading, TR `(0, 0x300, depth / 2)`, `0xC` frames on the caster's body | `= scalar << 7` |
/// | 1 (`0x801F7518`) | pitch `+1`, TR z `+32` | the living party shown; a cut on the caster, pitch `0x100`, yaw `-facing - 0x100`, TR `(0, 0, 0x2000)` | `+= scalar << 7` |
/// | 2 (`0x801F7634`) | TR z `+1`, yaw `+2` | the ring and the sweep | `+= scalar << 7` |
/// | 3 (`0x801F7B90`) | TR z `+1`, yaw `+2` | the settle, then `0xFF` | - |
/// | `0xFF` (`0x801F7DBC`) | - | every living seat shown at rate `8` | - |
///
/// The drifts are `scalar * step` shifted; a tick is one step.
///
/// PORT: overlay_cast_water_hazard_0956_801f7298 (PROT 0956 `0x71`; the camera arms and the seat writes)
pub fn water_hazard_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let depth = i32::from(seats.depth_raw as i16);
    let mut arm = CaptureCamArm::default();
    match phase {
        0 => {
            // The caster first turns on its target: a seat's live pair, or
            // the centroid of a group code (`FUN_801DCEAC`), through the
            // bearing `FUN_80019B28` plus a half turn (`0x801F7394..0x801F73E4`);
            // the shot frames the new heading.
            let v = seats.victim;
            let onto = match st.target_aim {
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
                colour: SHOWN.colour,
                render_flag: Some(9),
                facing: Some(onto),
                ..Default::default()
            };
            arm.shot = Some(ModuleShot {
                angles: [0, yaw_from(0x800, onto), 0],
                tr: [0, 0x300, (depth / 2) as i16],
                focus: focus_on(st.caster_body.unwrap_or(c)),
                frames: 0xC,
            });
            st.countdown.0 = SPEED_SCALAR << 7;
        }
        1 => {
            arm.drift = Some(CaptureDrift {
                pitch: 1,
                yaw: 0,
                tr_y: 0,
                tr_z: (SPEED_SCALAR * 4) as i16,
                tr_x: 0,
            });
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            arm.party_write = SHOWN;
            arm.party_live_only = true;
            arm.shot = Some(ModuleShot {
                angles: [
                    0x100,
                    (c.facing as i16).wrapping_neg().wrapping_sub(0x100),
                    0,
                ],
                tr: [0, 0, 0x2000],
                focus: focus_on(c),
                frames: 1,
            });
            st.countdown.add(1 << 7);
        }
        2 | 3 => {
            arm.drift = Some(CaptureDrift {
                pitch: 0,
                yaw: (SPEED_SCALAR / 4) as i16,
                tr_y: 0,
                tr_z: (SPEED_SCALAR / 8) as i16,
                tr_x: 0,
            });
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            if phase == 2 {
                st.countdown.add(1 << 7);
            }
        }
        0xFF => {
            arm.others_write = SHOWN;
            arm.others_live_only = true;
            arm.caster_write = SHOWN;
        }
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_seats_twelve_pairs_at_a_third_and_a_sixth() {
        let ring = water_hazard_ring((100, -100), 0x400);
        assert_eq!(ring.len(), 24);
        let at = |s: &ModuleSpawn| match s.anchor {
            SpawnAnchor::LiteralScaled { pos, .. } => pos,
            _ => panic!("a literal anchor"),
        };
        // Bearing 0: sin 0, cos 0x1000.
        assert_eq!(at(&ring[0]), [100, 0, -100 + 0x1000 / 3]);
        assert_eq!(at(&ring[1]), [100, 0, -100 + 0x1000 / 6]);
    }

    #[test]
    fn arm_one_cuts_on_its_gate_and_drifts_every_pass() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        let _ = water_hazard_camera(&mut st, 0, seats);
        let held = water_hazard_camera(&mut st, 1, seats);
        assert!(held.hold);
        assert_eq!(held.drift.expect("a drift").pitch, 1);
        st.countdown.0 = 0;
        let cut = water_hazard_camera(&mut st, 1, seats);
        assert!(!cut.hold);
        assert_eq!(cut.shot.expect("a cut").tr, [0, 0, 0x2000]);
    }
}
