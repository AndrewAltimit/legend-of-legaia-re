//! PROT 0956's **Paralyzing Wave** body (`0x801F69D8`, action `0x75`): the
//! camera arms, the seat hides and shows, and the records. The phase chain is
//! [`crate::cast_module_ticks::WATER_HAZARD_75_CHAIN`], its countdowns
//! [`super::capture_countdown::PARALYZING_WAVE`].
//!
//! See `ghidra/scripts/funcs/overlay_cast_water_hazard_0956_801f69d8.txt`.

use super::v_windhash::{HIDDEN, SHOWN, seat};
use super::{
    CaptureCamArm, CaptureDrift, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleShot,
    ModuleSpawn, SPEED_SCALAR, SpawnAnchor, SpawnRecord, focus_on, yaw_from,
};
use legaia_engine_system::trig::bearing12;

/// PROT 0956's `0x75` tick body.
pub const PARALYZING_WAVE_BODY: u32 = 0x801F_69D8;

const fn on_shot(record: u32) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor: SpawnAnchor::ShotFocus,
    }
}

/// Arm 0's four pool calls on the shot's trios (`0x801F6B9C..0x801F6C0C`).
const ARM0_SPAWNS: [ModuleSpawn; 4] = [
    on_shot(0x801F_8260),
    on_shot(0x801F_838C),
    on_shot(0x801F_83E4),
    on_shot(0x801F_843C),
];

/// Arm 1's three pool calls on the zeroed stack trios (`0x801F6D14..0x801F6D5C`),
/// after the pool flush.
const ARM1_RECORDS: [u32; 3] = [0x801F_8494, 0x801F_8560, 0x801F_85B4];

/// Arm 2's call on the victim's `+0x34` / `+0x44` (`0x801F6ED0..0x801F6EE4`).
const ARM2_RECORD: u32 = 0x801F_8608;

/// Arm 3's fade: the word `0x801F86A8` climbs `4 * scalar * delta` a pass into
/// every pooled part's level `+0x78`, and past `0x1000` ends the cast
/// (`0x801F7120..0x801F7180`).
const FADE_DONE: i32 = 0x1000;

/// **Paralyzing Wave** (a `beq` / `slti` tree over `0..=3` and `0xFF`; caster
/// `s5`, victim `s4`, module word `0x801F86A0`):
///
/// | arm | camera | seats and records | countdown |
/// |---|---|---|---|
/// | 0 (`0x801F6AB4`) | the caster turned to the victim; a cut: yaw `0x800 - facing`, TR `(0, 0x280, ctx[+0x6D0] / 2)` on the caster's display pair | every seat with HP hidden, the caster shown; four records on the shot | seed `scalar << 8` |
/// | 1 (`0x801F6CB4`) | TR z `+s / 2` a pass; on the gate a cut on the victim: yaw `0x800 - facing`, TR `(0, 0x380, 0x600)` | on the gate the pool flush, three records, the victim shown and the caster hidden | `+= scalar << 8` |
/// | 2 (`0x801F6E74`) | TR z `+s / 2` | on the gate a record on the victim and the hit | `+= scalar << 9` |
/// | 3 (`0x801F70E4`) | TR z `+s / 2` | the pooled parts' level climbs to `0x1000`, which ends the cast | drained, then the victim's settle |
/// | `0xFF` (`0x801F7200`) | - | every seat with HP shown at rate `8` | - |
///
/// PORT: FUN_801F69D8 (PROT 0956 `0x75`; the camera arms, the seat writes and the records)
pub fn paralyzing_wave_camera(
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
        0 => {
            // `FUN_80019B28(victim z, victim x, caster z, caster x) + 0x800`.
            let facing = (bearing12(
                i32::from(c.z) - i32::from(v.z),
                i32::from(c.x) - i32::from(v.x),
            ) + 0x800)
                & 0xFFF;
            let body = st.caster_body.unwrap_or(c);
            arm.shot = Some(ModuleShot {
                angles: [0, yaw_from(0x800, facing), 0],
                tr: [0, 0x280, (seats.depth_raw / 2) as i16],
                focus: focus_on(body),
                frames: 1,
            });
            arm.others_write = HIDDEN;
            arm.others_live_only = true;
            arm.caster_write = CaptureSeatWrite {
                facing: Some(facing),
                ..SHOWN
            };
            for (slot, spawn) in arm.pass_spawns.iter_mut().zip(ARM0_SPAWNS) {
                *slot = Some(spawn);
            }
            st.countdown.0 = SPEED_SCALAR << 8;
        }
        1..=3 => {
            arm.drift = Some(push(s >> 1));
            if phase == 3 {
                st.module_words[0] = (st.module_words[0] + 4 * SPEED_SCALAR).min(FADE_DONE);
                arm.parts_level = Some(st.module_words[0] as u16);
                arm.hold = st.module_words[0] < FADE_DONE && st.countdown.drain_above(0);
                return arm;
            }
            arm.hold = st.countdown.drain_above(0);
            if arm.hold {
                return arm;
            }
            if phase == 1 {
                arm.shot = Some(ModuleShot {
                    angles: [0, yaw_from(0x800, v.facing), 0],
                    tr: [0, 0x380, 0x600],
                    focus: focus_on(v),
                    frames: 1,
                });
                arm.victim_write = SHOWN;
                arm.caster_write = HIDDEN;
                for (slot, &r) in arm.pass_spawns.iter_mut().zip(&ARM1_RECORDS) {
                    *slot = Some(seat(r, [0; 3], [0; 3]));
                }
                st.countdown.add(1 << 8);
            } else {
                arm.pass_spawns[0] = Some(ModuleSpawn {
                    record: SpawnRecord::Module(ARM2_RECORD),
                    anchor: SpawnAnchor::Victim,
                });
                st.module_words[0] = 0;
                st.countdown.add(1 << 9);
            }
        }
        0xFF => {
            arm.others_write = CaptureSeatWrite {
                rate: Some(8),
                ..SHOWN
            };
            arm.others_live_only = true;
            arm.caster_write = CaptureSeatWrite {
                rate: Some(8),
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
    fn arm_three_fades_the_parts_in_and_ends_at_full_level() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = i32::MAX / 2;
        let seats = ModuleCamSeats::default();
        let mut ticks = 0;
        loop {
            ticks += 1;
            let a = paralyzing_wave_camera(&mut st, 3, seats);
            if !a.hold {
                assert_eq!(a.parts_level, Some(0x1000));
                break;
            }
        }
        assert_eq!(ticks, 0x1000 / (4 * SPEED_SCALAR));
    }
}
