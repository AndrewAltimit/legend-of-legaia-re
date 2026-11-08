//! PROT 0939's **Spore Gas** body (`0x801F69D8`, action `0x4F`): the camera
//! arms and the seat swap. The four hits are [`crate::cast_module_ticks::SporeGasBeat`].
//!
//! See `ghidra/scripts/funcs/overlay_cast_spore_gas_0939_801f69d8.txt`.

use super::capture::CaptureDrift;
use super::v_windhash::{HIDDEN, SHOWN, seat};
use super::{
    CaptureCamArm, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleSeat, ModuleShot,
    SPEED_SCALAR, bearing_12bit_approx, focus_on, yaw_from,
};
use legaia_engine_system::trig::trig12;

/// The shot on the caster the first three arms build.
fn on_caster(c: ModuleSeat, pitch: i16, tr: [i16; 2], frames: u16) -> ModuleShot {
    ModuleShot {
        angles: [pitch, yaw_from(0x800, c.facing), 0],
        tr: [0, tr[0], tr[1]],
        focus: focus_on(c),
        frames,
    }
}

/// The cloud arm 1 seats, with the yaw each call holds. The arm keeps one
/// angle trio on its stack and steps its `y` between calls, `+0x100` twice,
/// `-0x300`, then `-0x100`.
const CLOUD_RECORDS: [(u32, i16); 8] = [
    (0x801F_7598, 0),
    (0x801F_767C, 0),
    (0x801F_7928, 0),
    (0x801F_7760, 0x100),
    (0x801F_7844, 0x100),
    (0x801F_7928, 0x200),
    (0x801F_7760, -0x100),
    (0x801F_7844, -0x100),
];
/// Arm 1's ninth call (`0x801F6E24`), past [`CaptureCamArm::charge_spawns`]'s eight.
const CLOUD_LAST: (u32, i16) = (0x801F_7928, -0x200);
/// `li v0,-0x1C2` / `sh v0,0x22(sp)` at `0x801F6D20`.
const CLOUD_Y: i16 = -0x1C2;
/// The burst each of arm 4's hits seats (`0x801F70B0..0x801F70C8`): camera-locked at
/// `(0, 0, 0x100)` on the prologue's zeroed trios.
pub const HIT_RECORD: u32 = 0x801F_74BC;

/// The spawn each of arm 4's hits makes ([`HIT_RECORD`]).
pub const fn spore_gas_hit_spawn() -> super::ModuleSpawn {
    seat(HIT_RECORD, [0, 0, 0x100], [0; 3])
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
/// | 4 (`0x801F6FD0`) | TR z `+= step * scalar` every pass; the hits; leaves on the gate | - |
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
    if phase == 4 {
        // Every pass of arm 4 pulls TR z back `step * scalar` ahead of its
        // gate (`0x801F6FD0..0x801F7014`, the store into `0x800840C0`).
        arm.drift = Some(CaptureDrift {
            tr_z: SPEED_SCALAR as i16,
            ..Default::default()
        });
        arm.drift_first = true;
    }
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
            // The victim goes off while the gas rises (`0x801F6C60..0x801F6C68`).
            arm.victim_write = HIDDEN;
            // The cloud, `0x190` ahead of the caster (`0x801F6C6C..0x801F6E24`).
            let (s, co) = trig12(c.facing);
            let ahead = |t: i16| (i32::from(t) * 400 / 4096) as i16;
            let pos = [
                c.x.wrapping_add(ahead(s)),
                CLOUD_Y,
                c.z.wrapping_add(ahead(co)),
            ];
            for (slot, (record, yaw)) in arm.charge_spawns.iter_mut().zip(CLOUD_RECORDS) {
                *slot = Some(seat(record, pos, [0, yaw, 0]));
            }
            arm.pass_spawns[0] = Some(seat(CLOUD_LAST.0, pos, [0, CLOUD_LAST.1, 0]));
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
    fn arm_one_hides_the_victim_and_seats_the_cloud_ahead_of_the_caster() {
        let mut st = ModuleCamState::default();
        let mut s = seats();
        s.caster.facing = 0;
        let arm = spore_gas_camera(&mut st, 1, s);
        assert_eq!(arm.victim_write.render_flag, Some(0xFF));
        assert_eq!(arm.charge_spawns.iter().flatten().count(), 8);
        let first = arm.charge_spawns[0].expect("a record");
        // Facing `0`: `0x190` along +Z from the caster, at `-0x1C2`.
        assert!(matches!(
            first.anchor,
            crate::cast_module_camera::SpawnAnchor::LiteralScaled {
                pos: [-6, CLOUD_Y, 588],
                ..
            }
        ));
    }

    #[test]
    fn arm_four_pulls_tr_z_back_while_it_holds() {
        // `0x801F6FD0..0x801F7014`: TR z `+= step * scalar` before the gate.
        // Retail's `teien_berserker_pre_spore_gas` film rises `8` a vsync.
        let mut st = ModuleCamState::default();
        st.countdown.0 = 1 << 20;
        let held = spore_gas_camera(&mut st, 4, seats());
        assert!(held.hold);
        assert_eq!(held.drift.map(|d| d.tr_z), Some(SPEED_SCALAR as i16));
        assert!(held.drift_first);
        assert!(spore_gas_camera(&mut st, 2, seats()).drift.is_none());
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
