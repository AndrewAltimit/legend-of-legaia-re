//! PROT 0949's **Water Crystals** body (`0x801F6A10`, action `0x59`): the camera
//! arms, the seat swap and the two record sets.
//!
//! See `ghidra/scripts/funcs/overlay_cast_water_crystals_0949_801f6a10.txt`.

use super::v_windhash::{HIDDEN, SHOWN, seat};
use super::{
    CaptureCamArm, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleShot, ModuleSpawn,
    SPEED_SCALAR, SpawnAnchor, SpawnRecord, bearing_12bit_approx, focus_on, yaw_from,
};

/// The module's countdown word (`lui 0x8020` / `-0x7E00`).
pub const WATER_CRYSTALS_COUNTDOWN: u32 = 0x801F_8200;

/// The lanes the frozen victim draws under (`lui 0x3FF2` / `ori 0x80` at
/// `0x801F72B0`), held by `+0x21C = 0xFF`.
const FROZEN_COLOUR: u32 = 0x3FF2_0080;

const fn on_victim(record: u32) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor: SpawnAnchor::Victim,
    }
}

/// Arm 0's fourteen calls at the stack's zeroed trios (`0x801F6AF0..0x801F6C60`).
pub(super) const ARM0_SPAWNS: [ModuleSpawn; 14] = [
    seat(0x801F_76BC, [0; 3], [0; 3]),
    seat(0x801F_7C18, [0; 3], [0; 3]),
    seat(0x801F_7C6C, [0; 3], [0; 3]),
    seat(0x801F_7CC0, [0; 3], [0; 3]),
    seat(0x801F_7D14, [0; 3], [0; 3]),
    seat(0x801F_7D84, [0; 3], [0; 3]),
    seat(0x801F_7DF4, [0; 3], [0; 3]),
    seat(0x801F_7E64, [0; 3], [0; 3]),
    seat(0x801F_7ED4, [0; 3], [0; 3]),
    seat(0x801F_7F44, [0; 3], [0; 3]),
    seat(0x801F_7FB4, [0; 3], [0; 3]),
    seat(0x801F_8024, [0; 3], [0; 3]),
    seat(0x801F_8094, [0; 3], [0; 3]),
    seat(0x801F_7720, [0; 3], [0; 3]),
];
/// Arm 0's last record, the one arm 2 keeps alive and moves to the victim.
pub(super) const ORB_RECORD: u32 = 0x801F_7720;
/// The twelve records arm 2 halts by handle.
pub(super) const ARM2_HALTS: [u32; 12] = [
    0x801F_7C18,
    0x801F_7C6C,
    0x801F_7CC0,
    0x801F_7D14,
    0x801F_7D84,
    0x801F_7DF4,
    0x801F_7E64,
    0x801F_7ED4,
    0x801F_7F44,
    0x801F_7FB4,
    0x801F_8024,
    0x801F_8094,
];
/// Arm 2's sixteen calls on the victim's `+0x34` / `+0x44`
/// (`0x801F7140..0x801F72A8`), after the halt bit on arm 0's first thirteen.
pub(super) const ARM2_SPAWNS: [ModuleSpawn; 16] = [
    on_victim(0x801F_7774),
    on_victim(0x801F_77D0),
    on_victim(0x801F_782C),
    on_victim(0x801F_7888),
    on_victim(0x801F_78E4),
    on_victim(0x801F_7940),
    on_victim(0x801F_799C),
    on_victim(0x801F_79F8),
    on_victim(0x801F_7A5C),
    on_victim(0x801F_7AA8),
    on_victim(0x801F_7B04),
    on_victim(0x801F_7B60),
    on_victim(0x801F_7BBC),
    on_victim(0x801F_80F0),
    on_victim(0x801F_8148),
    on_victim(0x801F_81A4),
];

/// **Water Crystals** (`sltiu 6` through the table at `0x801F69D8`; caster
/// `actor_table[ctx + 0x13]`, victim `actor_table[caster + 0x1DD]`):
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 0 (`0x801F6ACC`) | the caster turned on the victim and shown, everyone else hidden; pitch `0x20`, yaw `0x800 - facing`, TR `(0x140, 0x300, depth / 2)`, `0x20` frames | `= scalar << 5` |
/// | 1 (`0x801F6E30`) | on the gate: pitch `0`, TR z `2 * depth / 3`, `0x80` frames | `+= scalar << 7` |
/// | 2 (`0x801F6EF8`) | on the gate: the victim shown frozen, the caster hidden; a cut on the victim, pitch `0x20`, yaw `-facing - 0x80`, TR `(0, -4 * height, min(3 * radius, 0xC00))` | `+= scalar << 7` |
/// | 3 (`0x801F72C4`) | on the gate: the hit; the victim's hold off | `+= scalar << 6` |
/// | 4 | on the gate | `+= scalar * 12` |
/// | 5 | on the gate: every seat shown again | - |
///
/// PORT: overlay_cast_water_crystals_0949_801f6a10 (PROT 0949 `0x59`; the camera arms, the seat swap and the spawn calls)
pub fn water_crystals_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (c, v) = (seats.caster, seats.victim);
    let mut arm = CaptureCamArm::default();
    let depth = i32::from(seats.depth_raw as i16);
    if (1..=5).contains(&phase) && st.countdown.drain_above(0) {
        arm.hold = true;
        return arm;
    }
    match phase {
        0 => {
            let facing = bearing_12bit_approx(v.z, v.x, c.z, c.x).wrapping_add(0x800) & 0xFFF;
            arm.ctx_278 = Some(2);
            arm.others_write = HIDDEN;
            arm.caster_write = CaptureSeatWrite {
                facing: Some(facing),
                ..SHOWN
            };
            arm.shot = Some(ModuleShot {
                angles: [0x20, yaw_from(0x800, facing), 0],
                tr: [0x140, 0x300, (depth / 2) as i16],
                focus: focus_on(c),
                frames: 0x20,
            });
            st.countdown.0 = SPEED_SCALAR << 5;
        }
        1 => {
            arm.shot = Some(ModuleShot {
                angles: [0, yaw_from(0x800, c.facing), 0],
                tr: [0x140, 0x300, (depth * 2 / 3) as i16],
                focus: focus_on(c),
                frames: 0x80,
            });
            st.countdown.add(1 << 7);
        }
        2 => {
            arm.victim_write = CaptureSeatWrite {
                colour: Some(FROZEN_COLOUR),
                render_flag: Some(0xFF),
                ..Default::default()
            };
            arm.caster_write = HIDDEN;
            let height = st.victim_body.map_or(v.y, |b| b.y);
            let reach = (i32::from(seats.victim_radius) * 3).min(0xC00);
            arm.shot = Some(ModuleShot {
                angles: [0x20, (c.facing as i16).wrapping_neg().wrapping_sub(0x80), 0],
                tr: [0, height.wrapping_mul(-4), reach as i16],
                focus: focus_on(v),
                frames: 1,
            });
            st.countdown.add(1 << 7);
        }
        3 => {
            arm.victim_write.render_flag = Some(0);
            st.countdown.add(1 << 6);
        }
        4 => st.countdown.0 += SPEED_SCALAR * 12,
        5 => arm.others_write = SHOWN,
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::super::ModuleSeat;
    use super::*;

    #[test]
    fn the_swap_cuts_to_the_victim_on_the_gate() {
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
            ..Default::default()
        };
        let a0 = water_crystals_camera(&mut st, 0, seats);
        assert_eq!(a0.shot.expect("a shot").tr, [0x140, 0x300, 0x600]);
        st.countdown.0 = SPEED_SCALAR;
        let a2 = water_crystals_camera(&mut st, 2, seats);
        assert_eq!(a2.caster_write.render_flag, Some(0xFF));
        assert_eq!(a2.victim_write.colour, Some(FROZEN_COLOUR));
        assert_eq!(a2.shot.expect("a cut").tr[2], 0x600);
    }
}
