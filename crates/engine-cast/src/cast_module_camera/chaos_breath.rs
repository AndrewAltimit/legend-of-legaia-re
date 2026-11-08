//! PROT 0938's **Chaos Breath** body (`0x801F726C`, action `0x4E`): the two
//! camera arms and their records, over
//! [`crate::cast_module_ticks::chaos_breath_tick`].
//!
//! See `ghidra/scripts/funcs/overlay_cast_chaos_breath_0938_801f726c.txt`.

use super::capture::{CaptureCamArm, CaptureSeatWrite};
use super::capture_countdown::{CHAOS_BREATH, arm_countdown};
use super::{
    ModuleCamSeats, ModuleCamState, ModuleShot, ModuleSpawn, SpawnAnchor, SpawnRecord,
    bearing_12bit_approx, focus_on, yaw_from,
};
use legaia_engine_system::trig::trig12;

/// PROT 0938's Chaos Breath tick body.
pub const CHAOS_BREATH_BODY: u32 = crate::cast_module_ticks::CHAOS_BREATH_TICK;

/// Arm 0's record on the caster (`0x801F7480`).
const BREATH_RECORD: u32 = 0x801F_7BC8;
/// The record arm 1 seats ahead of the caster as it passes (`0x801F772C`).
const CLOUD_RECORD: u32 = 0x801F_7C40;

/// **Chaos Breath** (`beq` / `slti` over `0..=3` and `0xFF`; caster
/// `actor_table[ctx + 0x13]`):
///
/// | arm | camera and records | word `0x801F8040` |
/// |---|---|---|
/// | 0 (`0x801F733C`) | the caster turned on its target (the row's centroid for a group, `FUN_801DCEAC`); pitch `-0x40`, yaw `0x800 - facing`, TR `(0, depth / 3, depth / 2)`, `0x100` frames; the breath on the caster at its model scale | `= 0x800` |
/// | 1 (`0x801F74A4`) | gate; pitch `0x40`, TR `(0, depth / 2, 3 * depth)`, `0x100` frames; a record ahead of the caster at `scale / 6`, `-5 * scale / 32` high | `+= 0x400` |
/// | 2, 3 | the sweep and the settle, no camera | table |
///
/// Arm 1's gate and its re-arm are the table's ([`CHAOS_BREATH`]). A director
/// turns the table off for its body, so this one runs it.
///
/// PORT: overlay_cast_chaos_breath_0938_801f726c (PROT 0938 `0x4E`; the camera arms and their records)
pub fn chaos_breath_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (c, v) = (seats.caster, seats.victim);
    let depth = i32::from(seats.depth_raw as i16);
    let scale = i32::from(seats.caster_scale);
    let mut arm = CaptureCamArm::default();
    if phase == 1 {
        // Each pass ahead of the gate pulls every party seat `2 * step` in
        // towards the caster (`0x801F74B4..0x801F7568`).
        arm.party_shove = Some(2);
    }
    if let Some(a) = arm_countdown(CHAOS_BREATH, phase) {
        if a.holds(&mut st.countdown) {
            arm.hold = true;
            return arm;
        }
        a.pass(&mut st.countdown);
    }
    match phase {
        0 => {
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
            arm.caster_write = CaptureSeatWrite {
                facing: Some(onto),
                ..Default::default()
            };
            arm.shot = Some(ModuleShot {
                angles: [-0x40, yaw_from(0x800, onto), 0],
                tr: [0, (depth / 3) as i16, (depth / 2) as i16],
                focus: focus_on(c),
                frames: 0x100,
            });
            arm.pass_spawns[0] = Some(ModuleSpawn {
                record: SpawnRecord::Module(BREATH_RECORD),
                anchor: SpawnAnchor::LiteralScaled {
                    pos: [c.x, c.y, c.z],
                    rot: [0, onto as i16, 0],
                    scale: seats.caster_scale,
                },
            });
        }
        1 => {
            arm.shot = Some(ModuleShot {
                angles: [0x40, yaw_from(0x800, c.facing), 0],
                tr: [0, (depth / 2) as i16, (3 * depth) as i16],
                focus: focus_on(c),
                frames: 0x100,
            });
            let (s, co) = trig12(c.facing);
            let ahead = |t: i16| ((i32::from(t) * scale / 6) >> 12) as i16;
            arm.pass_spawns[0] = Some(ModuleSpawn {
                record: SpawnRecord::Module(CLOUD_RECORD),
                anchor: SpawnAnchor::LiteralScaled {
                    pos: [
                        c.x.wrapping_add(ahead(s)),
                        (-(scale * 0x280) / 0x1000) as i16,
                        c.z.wrapping_add(ahead(co)),
                    ],
                    rot: [0, 0, 0],
                    scale: 0x1000,
                },
            });
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
                x: 0,
                y: 0,
                z: 813,
                facing: 2048,
            },
            victim: ModuleSeat {
                x: 0,
                y: 0,
                z: -812,
                facing: 0,
            },
            depth_raw: 4915,
            caster_scale: 0x1000,
            ..Default::default()
        }
    }

    #[test]
    fn the_opening_shot_is_retails_table() {
        let mut st = ModuleCamState::default();
        let a = chaos_breath_camera(&mut st, 0, seats());
        let shot = a.shot.expect("a shot");
        assert_eq!(shot.angles[0], -0x40);
        assert_eq!(shot.tr, [0, 4915 / 3, 4915 / 2]);
        assert_eq!(shot.frames, 0x100);
        assert_eq!(st.countdown.0, 0x800);
    }

    #[test]
    fn arm_one_holds_then_pulls_out_and_rearms() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = 0x800;
        let held = chaos_breath_camera(&mut st, 1, seats());
        assert!(held.hold && held.shot.is_none());
        st.countdown.0 = 1;
        let a = chaos_breath_camera(&mut st, 1, seats());
        assert!(!a.hold);
        assert_eq!(a.shot.expect("a shot").tr, [0, 4915 / 2, 3 * 4915]);
        assert!(st.countdown.0 > 0x3F0);
    }
}
