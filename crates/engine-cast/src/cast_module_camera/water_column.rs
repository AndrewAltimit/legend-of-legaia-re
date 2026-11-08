//! PROT 0945's **Water Column** body (`0x801F6EDC`, action `0x54`): the camera
//! arms, the seat swap and the record calls.
//!
//! See `ghidra/scripts/funcs/overlay_cast_water_column_0945_801f6edc.txt`.

use super::v_windhash::{HIDDEN, SHOWN};
use super::{
    CaptureCamArm, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleShot, ModuleSpawn,
    SPEED_SCALAR, SpawnAnchor, SpawnRecord, bearing_12bit_approx, focus_on, yaw_from,
};

/// The module's countdown word (`lui 0x801F` / `0x7E5C`).
pub const WATER_COLUMN_COUNTDOWN: u32 = 0x801F_7E5C;

const fn on(record: u32, anchor: SpawnAnchor) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor,
    }
}

/// Arm 1's call on the caster's `+0x34` / `+0x44` (`0x801F710C..0x801F7130`).
pub(super) const ARM1_SPAWNS: [ModuleSpawn; 1] = [on(0x801F_7860, SpawnAnchor::Caster { y: 0 })];
/// Arm 4's nine calls on the victim's (`0x801F7348..0x801F7408`).
pub(super) const ARM4_SPAWNS: [ModuleSpawn; 9] = [
    on(0x801F_7910, SpawnAnchor::Victim),
    on(0x801F_79AC, SpawnAnchor::Victim),
    on(0x801F_7A1C, SpawnAnchor::Victim),
    on(0x801F_7A78, SpawnAnchor::Victim),
    on(0x801F_7AD4, SpawnAnchor::Victim),
    on(0x801F_7B30, SpawnAnchor::Victim),
    on(0x801F_7B8C, SpawnAnchor::Victim),
    on(0x801F_7BA4, SpawnAnchor::Victim),
    on(0x801F_7BF4, SpawnAnchor::Victim),
];

/// **Water Column** (`sltiu 8` through the table at `0x801F69D8`; caster
/// `actor_table[ctx + 0x13]`, victim `actor_table[caster + 0x1DD]`):
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 0 (`0x801F6F94`) | the caster turned on the victim, `+0x21C = 6`; the victim hidden; pitch `-0x20`, yaw `0x800 - facing`, TR `(0, 0x400, depth / 3)`, `0x20` frames | `= scalar << 6` |
/// | 1 (`0x801F7084`) | on the gate: pitch `0x180`, TR z `depth * 2`, `0x80` frames | `+= scalar << 7` |
/// | 2 (`0x801F7140`) | on the gate: the victim shown, the caster hidden; a cut on the victim, pitch `-0x20`, yaw `-facing`, TR `(0, -6 * height, 3 * radius)` | - |
/// | 3 (`0x801F7200`) | ungated: pitch `-0x20`, yaw `0x800 - the victim's`, TR z `3 * radius / 2`, `0x20` frames | `+= scalar << 6` |
/// | 4 (`0x801F72A0`) | on the gate: pitch `0x180`, TR `(0, 0, 0x1800)`, `0x80` frames | `+= scalar << 7` |
/// | 5 (`0x801F7418`) | on the gate: the hit, the victim turned on the caster | `+= scalar * 0x60` |
/// | 6 (`0x801F75AC`) | on the gate | `+= scalar << 6` |
/// | 7 (`0x801F7604`) | case 8 on the caster each pass; on the settle the caster shown | - |
///
/// PORT: overlay_cast_water_column_0945_801f6edc (PROT 0945 `0x54`; the camera arms and the seat swap)
pub fn water_column_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (c, v) = (seats.caster, seats.victim);
    let mut arm = CaptureCamArm::default();
    let depth = i32::from(seats.depth_raw as i16);
    let height = st.victim_body.map_or(v.y, |b| b.y);
    let radius = i32::from(seats.victim_radius) * 3;
    if matches!(phase, 1 | 2 | 4 | 5 | 6) && st.countdown.drain_above(0) {
        arm.hold = true;
        return arm;
    }
    match phase {
        0 => {
            let facing = bearing_12bit_approx(v.z, v.x, c.z, c.x).wrapping_add(0x800) & 0xFFF;
            arm.caster_write = CaptureSeatWrite {
                facing: Some(facing),
                render_flag: Some(6),
                ..Default::default()
            };
            arm.victim_write = HIDDEN;
            arm.shot = Some(ModuleShot {
                angles: [-0x20, yaw_from(0x800, facing), 0],
                tr: [0, 0x400, (depth / 3) as i16],
                focus: focus_on(c),
                frames: 0x20,
            });
            st.countdown.0 = SPEED_SCALAR << 6;
        }
        1 => {
            arm.shot = Some(ModuleShot {
                angles: [0x180, yaw_from(0x800, c.facing), 0],
                tr: [0, 0, (depth * 2) as i16],
                focus: focus_on(c),
                frames: 0x80,
            });
            st.countdown.add(1 << 7);
        }
        2 => {
            arm.victim_write = SHOWN;
            arm.caster_write = HIDDEN;
            arm.shot = Some(ModuleShot {
                angles: [-0x20, (c.facing as i16).wrapping_neg(), 0],
                tr: [0, height.wrapping_mul(-6), radius as i16],
                focus: focus_on(v),
                frames: 1,
            });
        }
        3 => {
            arm.shot = Some(ModuleShot {
                angles: [-0x20, yaw_from(0x800, v.facing), 0],
                tr: [0, height.wrapping_mul(-6), (radius / 2) as i16],
                focus: focus_on(v),
                frames: 0x20,
            });
            st.countdown.add(1 << 6);
        }
        4 => {
            arm.shot = Some(ModuleShot {
                angles: [0x180, yaw_from(0x800, v.facing), 0],
                tr: [0, 0, 0x1800],
                focus: focus_on(v),
                frames: 0x80,
            });
            st.countdown.add(1 << 7);
        }
        5 => {
            arm.victim_write.facing =
                Some(bearing_12bit_approx(c.z, c.x, v.z, v.x).wrapping_add(0x800) & 0xFFF);
            st.countdown.0 += SPEED_SCALAR * 0x60;
        }
        6 => st.countdown.add(1 << 6),
        // Case 8 ahead of the settle's drain, held or not (`0x801F7614`).
        7 => arm.end_frame = true,
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::super::ModuleSeat;
    use super::*;

    #[test]
    fn the_swap_cuts_to_the_victim_on_arm_two_and_arm_three_moves_at_once() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats {
            caster: ModuleSeat {
                x: 500,
                y: 0,
                z: 800,
                facing: 0x800,
            },
            victim: ModuleSeat {
                x: 0,
                y: 0,
                z: -800,
                facing: 0,
            },
            depth_raw: 0xC00,
            victim_radius: 0x200,
            caster_scale: 0x1000,
            ..Default::default()
        };
        let a0 = water_column_camera(&mut st, 0, seats);
        assert_eq!(a0.shot.expect("a shot").tr, [0, 0x400, 0x400]);
        assert_eq!(a0.caster_write.render_flag, Some(6));
        st.countdown.0 = SPEED_SCALAR;
        let a2 = water_column_camera(&mut st, 2, seats);
        assert_eq!(a2.caster_write.render_flag, Some(0xFF));
        assert_eq!(a2.shot.expect("a cut").tr[2], 0x600);
        st.countdown.0 = 0;
        let a3 = water_column_camera(&mut st, 3, seats);
        assert!(!a3.hold, "arm 3 has no gate");
        assert_eq!(a3.shot.expect("a shot").tr[2], 0x300);
    }
}
