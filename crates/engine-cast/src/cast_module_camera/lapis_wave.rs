//! PROT 0943's **Lapis Wave** body (`0x801F6A04`, action `0xB5`): the camera arms,
//! the party hidden and shown again, and the record calls.
//!
//! See `ghidra/scripts/funcs/overlay_cast_curse_0943_801f6a04.txt`.

use super::v_windhash::{HIDDEN, SHOWN, seat};
use super::{
    CaptureCamArm, CaptureDrift, ModuleCamSeats, ModuleCamState, ModuleShot, ModuleSpawn,
    SpawnAnchor, SpawnRecord, focus_on, yaw_from,
};

/// Arm 0's call on the shot's negated focus and angles (`0x801F6B68`).
pub(super) const ARM0_SPAWNS: [ModuleSpawn; 1] = [ModuleSpawn {
    record: SpawnRecord::Module(0x801F_7830),
    anchor: SpawnAnchor::ShotFocus,
}];
/// Arm 2's call at the stack's zeroed trios (`0x801F6C74`).
pub(super) const ARM2_SPAWNS: [ModuleSpawn; 1] = [seat(0x801F_7904, [0; 3], [0; 3])];
/// Arm 2's two calls on each party seat's `+0x34` / `+0x44`
/// (`0x801F6CC8`, `0x801F6CE4`).
pub const LAPIS_WAVE_SEAT_RECORDS: [u32; 2] = [0x801F_7964, 0x801F_79B4];

/// TR z `+8` a frame (`0x800840C0`), arms 1 and 2.
const SLOW: CaptureDrift = CaptureDrift {
    pitch: 0,
    yaw: 0,
    tr_y: 0,
    tr_z: 8,
    tr_x: 0,
};
/// TR z `+16` and yaw `+2` a frame (`0x800840C0`, `0x8007B792`), arms 3 and 4.
const FAST: CaptureDrift = CaptureDrift {
    pitch: 0,
    yaw: 2,
    tr_y: 0,
    tr_z: 16,
    tr_x: 0,
};

/// **Lapis Wave** (`sltiu 5` through the table at `0x801F69D8`), the one body
/// whose gate drains the bare step and whose seeds are literals:
///
/// | arm | drift | on the gate | countdown |
/// |---|---|---|---|
/// | 0 (`0x801F6A9C`) | - | the party hidden; pitch `-0x20`, yaw `0x800 - facing`, TR `(0, 0x600, 0x600)`, `0x20` frames on the caster | `= 0x100` |
/// | 1 (`0x801F6B78`) | TR z `+8` | the held fade | `+= 0x80` |
/// | 2 (`0x801F6C20`) | TR z `+8` | the pool flushed, the party shown; a cut at the origin, pitch `0`, yaw `-facing`, TR `(0, 0x400, 0x1000)` | `+= 0x100` |
/// | 3 (`0x801F6DE0`) | TR z `+16`, yaw `+2` | - | `+= 0x80` |
/// | 4 (`0x801F6E58`) | TR z `+16`, yaw `+2` | the body finishes | - |
///
/// PORT: overlay_cast_curse_0943_801f6a04 (PROT 0943 `0xB5`; the camera arms and the seat writes)
pub fn lapis_wave_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let mut arm = CaptureCamArm::default();
    if phase == 0 {
        arm.party_write = HIDDEN;
        arm.shot = Some(ModuleShot {
            angles: [-0x20, yaw_from(0x800, c.facing), 0],
            tr: [0, 0x600, 0x600],
            focus: focus_on(c),
            frames: 0x20,
        });
        st.countdown.0 = 0x100;
        return arm;
    }
    arm.drift = match phase {
        1 | 2 => Some(SLOW),
        3 | 4 => Some(FAST),
        _ => None,
    };
    st.countdown.0 -= 1;
    if st.countdown.0 > 0 {
        arm.hold = true;
        return arm;
    }
    match phase {
        1 | 3 => st.countdown.0 += 0x80,
        2 => {
            arm.party_write = SHOWN;
            arm.shot = Some(ModuleShot {
                angles: [0, (c.facing as i16).wrapping_neg(), 0],
                tr: [0, 0x400, 0x1000],
                focus: [0; 3],
                frames: 1,
            });
            st.countdown.0 += 0x100;
        }
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_drains_the_bare_step_and_arm_two_cuts_to_the_origin() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        let a0 = lapis_wave_camera(&mut st, 0, seats);
        assert_eq!(a0.party_write, HIDDEN);
        assert_eq!(st.countdown.0, 0x100);
        let a1 = lapis_wave_camera(&mut st, 1, seats);
        assert!(a1.hold);
        assert_eq!(a1.drift, Some(SLOW));
        assert_eq!(st.countdown.0, 0xFF);
        st.countdown.0 = 1;
        let a2 = lapis_wave_camera(&mut st, 2, seats);
        assert!(!a2.hold);
        assert_eq!(a2.party_write, SHOWN);
        assert_eq!(a2.shot.expect("a cut").focus, [0; 3]);
        assert_eq!(st.countdown.0, 0x100);
    }
}
