//! PROT 0945's **Jugger Power** body (`0x801F69F8`, action `0xBA`, Van
//! Saryu's self-buff): the camera arms, the seat hides, the records and the
//! countdown gates. The surge itself is the body's
//! ([`crate::cast_module_ticks::all_stats_surge_tick`]).
//!
//! See `ghidra/scripts/funcs/overlay_cast_water_column_0945_801f69f8.txt`.

use super::v_windhash::{HIDDEN, seat};
use super::{
    CaptureCamArm, CaptureDrift, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleShot,
    ModuleSpawn, SpawnAnchor, SpawnRecord, focus_on, yaw_from,
};

/// PROT 0945's `0xBA` tick body.
pub const JUGGER_POWER_BODY: u32 = crate::cast_module_ticks::ALL_STATS_SURGE_TICK;

/// The three records arm 0 seats `0x1D00` units down the stage's z axis
/// (`0x801F6B0C`, `0x801F6B24`, `0x801F6B3C`).
const OPEN_RECORDS: [u32; 3] = [0x801F_7C44, 0x801F_7C8C, 0x801F_7CD4];
/// The three records arm 1's pass seats on the caster's live trio and angle
/// trio (`a0 = caster + 0x34`, `a1 = caster + 0x44`; `0x801F6CC0`,
/// `0x801F6CD8`, `0x801F6CF0`).
const CASTER_RECORDS: [u32; 3] = [0x801F_7D44, 0x801F_7D8C, 0x801F_7DD4];
/// `+0x04` arm 1 stores on the caster (`lui 0x2008` / `ori 0x200`).
const SHOWN_COLOUR: u32 = 0x2008_0200;
/// `+0x21C` arm 1 stores on the caster (`li v0,6` / `sb v0,0x21c`).
const CASTER_RENDER_FLAG: u8 = 6;

const fn on_caster(record: u32) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor: SpawnAnchor::CasterShifted { dy: 0, dz: 0 },
    }
}

/// The yaw / TR z drift arms 1 and 2 add on every pass, ahead of their gate:
/// yaw `+= step * 2` (`0x8007B792`, masked `0xFFF`) and TR z `+= step << 4`
/// on arm 1, `step << 3` on arm 2 (`0x800840C0`).
fn spin(tr_z: i16) -> CaptureDrift {
    CaptureDrift {
        yaw: 2,
        tr_z,
        ..Default::default()
    }
}

/// **Jugger Power** (a `beq` / `slti` chain over `0..=3`; caster
/// `actor_table[ctx + 0x13]`; countdown word `0x801F7E5C`, the one Water
/// Column gates on too):
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 0 (`0x801F6AA8`) | a cut far down the stage: pitch `0x80`, yaw `-0x80`, TR `(0, 0x1800, 0x4000)`, focus `(0, 0, -0x1D00)`; three records at `(0, 0, 0x1D00)` on yaw `-0x80`; every party seat and the caster hidden (`+0x04 = 0`, `+0x21C = 0xFF`) | seed `scalar << 7` |
/// | 1 (`0x801F6BCC`) | every pass: yaw `+2`, TR z `+16` a vsync; once the gate passes, the part pool flushed, a cut on the caster (pitch `0`, yaw `0x780 - facing`, TR `(0, 0x400, 0x800)`), the caster shown (`+0x04 = 0x20080200`, `+0x21C = 6`) with three records on it, and the message bar | `= scalar << 7` |
/// | 2 (`0x801F6D24`) | every pass: yaw `+2`, TR z `+8` a vsync; once the gate passes, the surge (the body's) | `= scalar << 6` |
/// | 3 (`0x801F6E6C`) | once the gate passes, the style clear and the `0x780` yaw base (the band's exit) | - |
///
/// The re-arms are stores, not adds. Nothing in the body shows the party
/// again, and nothing in it calls the damage wrapper, so the band's fold
/// owes the cast nothing ([`CaptureCamArm::skips_fold`]). The arm-0 cue `0x19D` is the module's head cue
/// ([`crate::cast_module_ticks`]'s voice scan); the arm-1 message bar
/// (`0x800774AC = 0x801F7E44`, `FUN_801D8DE8(0x5B, 0)`) is the presentation
/// decode's.
///
/// PORT: overlay_cast_water_column_0945_801f69f8 (PROT 0945 `0xBA`; the camera arms, the seat hides, the records and the countdown gates)
pub fn jugger_power_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    // The body has no damage site: its one `jal` set is the cue, the
    // framing builder, the two spawn wrappers, the pool flush and the
    // message bar. Water Column's power, which the module's catalog row
    // carries, is not this cast's.
    let mut arm = CaptureCamArm {
        skips_fold: true,
        ..Default::default()
    };
    match phase {
        0 => {
            arm.shot = Some(ModuleShot {
                angles: [0x80, -0x80, 0],
                tr: [0, 0x1800, 0x4000],
                focus: [0, 0, -0x1D00],
                frames: 1,
            });
            for (slot, record) in arm.pass_spawns.iter_mut().zip(OPEN_RECORDS) {
                *slot = Some(seat(record, [0, 0, 0x1D00], [0, -0x80, 0]));
            }
            arm.party_write = HIDDEN;
            arm.caster_write = HIDDEN;
            st.countdown.arm(7);
        }
        1 | 2 => {
            arm.drift = Some(spin(if phase == 1 { 16 } else { 8 }));
            arm.drift_first = true;
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            if phase == 1 {
                arm.flushes_pool = true;
                arm.shot = Some(ModuleShot {
                    angles: [0, yaw_from(0x780, c.facing), 0],
                    tr: [0, 0x400, 0x800],
                    focus: focus_on(c),
                    frames: 1,
                });
                arm.caster_write = CaptureSeatWrite {
                    colour: Some(SHOWN_COLOUR),
                    render_flag: Some(CASTER_RENDER_FLAG),
                    ..Default::default()
                };
                for (slot, record) in arm.pass_spawns.iter_mut().zip(CASTER_RECORDS) {
                    *slot = Some(on_caster(record));
                }
                st.countdown.arm(7);
            } else {
                st.countdown.arm(6);
            }
        }
        3 => {
            arm.hold = st.countdown.drain_above(0);
        }
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::super::{ModuleSeat, SPEED_SCALAR};
    use super::*;

    fn seats() -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 0,
                y: 0,
                z: -0x600,
                facing: 0,
            },
            depth_raw: 3000,
            action: 0xBA,
            ..Default::default()
        }
    }

    /// Ticks an arm holds before it passes, at one drain a tick.
    fn dwell(st: &mut ModuleCamState, phase: u8) -> u32 {
        let mut n = 1;
        while jugger_power_camera(st, phase, seats()).hold {
            n += 1;
        }
        n
    }

    #[test]
    fn arm_zero_hides_the_field_and_seats_the_far_records() {
        let mut st = ModuleCamState::default();
        let a = jugger_power_camera(&mut st, 0, seats());
        assert_eq!(a.party_write, HIDDEN);
        assert_eq!(a.caster_write, HIDDEN);
        assert_eq!(a.pass_spawns.iter().flatten().count(), 3);
        assert_eq!(a.shot.unwrap().focus, [0, 0, -0x1D00]);
        assert_eq!(st.countdown.0, SPEED_SCALAR << 7);
        assert!(a.skips_fold);
    }

    #[test]
    fn arms_hold_on_the_stored_countdowns() {
        let mut st = ModuleCamState::default();
        jugger_power_camera(&mut st, 0, seats());
        assert_eq!(dwell(&mut st, 1), 128);
        assert_eq!(dwell(&mut st, 2), 128);
        assert_eq!(dwell(&mut st, 3), 64);
    }

    #[test]
    fn arm_one_drifts_while_held_and_shows_the_caster_as_it_passes() {
        let mut st = ModuleCamState::default();
        jugger_power_camera(&mut st, 0, seats());
        let held = jugger_power_camera(&mut st, 1, seats());
        assert!(held.hold);
        assert_eq!(held.drift.map(|d| (d.yaw, d.tr_z)), Some((2, 16)));
        assert!(held.shot.is_none());
        st.countdown.0 = 1;
        let pass = jugger_power_camera(&mut st, 1, seats());
        assert!(!pass.hold && pass.flushes_pool);
        assert_eq!(pass.caster_write.render_flag, Some(CASTER_RENDER_FLAG));
        assert_eq!(pass.shot.unwrap().angles[1], 0x780);
        assert_eq!(pass.pass_spawns.iter().flatten().count(), 3);
    }
}
