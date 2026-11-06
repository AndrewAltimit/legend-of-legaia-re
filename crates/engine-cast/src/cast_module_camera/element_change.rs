//! PROT 0964's **Element Change** body (`0x801F88EC`, action `0xAF`): the
//! camera arms and the party's hide.
//!
//! See `ghidra/scripts/funcs/overlay_cast_element_change_0964_801f88ec.txt`.

use super::v_windhash::seat;
use super::{
    CaptureCamArm, CaptureDrift, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleShot,
    SPEED_SCALAR, focus_on, yaw_from,
};

/// PROT 0964's Element Change tick body.
pub const ELEMENT_CHANGE_BODY: u32 = 0x801F_88EC;

/// The module's countdown word (`lui 0x8020` / `-0x648C`).
pub const ELEMENT_CHANGE_COUNTDOWN: u32 = 0x801F_9B74;

/// Arm 0's record on the caster (`0x801F8AB0`).
const ELEMENT_CHANGE_RECORD: u32 = 0x801F_8EB8;

/// **Element Change** (`beq v1,1` / `slti v1,2` / `beq v1,2`; caster
/// `actor_table[ctx + 0x13]`):
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 0 (`0x801F8990`) | a cut on the caster: pitch `0`, yaw `0x800 - facing`, TR `(0, 0x600, 0x600)`, one frame; the record and the element roll are the body's | seed `scalar << 8` |
/// | 1 (`0x801F8AC0`) | every pass TR z `+= k` and every party seat hidden (`+0x04 = 0`, `+0x21C = 0xFF`) | `+= scalar << 6` |
/// | 2 (`0x801F8B94`) | the wait; on its pass the body's exit | - |
///
/// PORT: overlay_cast_element_change_0964_801f88ec (PROT 0964 `0xAF`; the camera arms and the party's hide)
pub fn element_change_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let mut arm = CaptureCamArm::default();
    match phase {
        0 => {
            arm.shot = Some(ModuleShot {
                angles: [0, yaw_from(0x800, c.facing), 0],
                tr: [0, 0x600, 0x600],
                focus: focus_on(c),
                frames: 1,
            });
            st.countdown.0 = SPEED_SCALAR << 8;
            arm.pass_spawns[0] = Some(seat(
                ELEMENT_CHANGE_RECORD,
                [c.x, c.y, c.z],
                [0, c.facing as i16, 0],
            ));
        }
        1 => {
            arm.drift = Some(CaptureDrift {
                tr_z: SPEED_SCALAR as i16,
                ..Default::default()
            });
            arm.party_write = CaptureSeatWrite {
                colour: Some(0),
                render_flag: Some(0xFF),
                ..Default::default()
            };
            if st.countdown.drain_above(0) {
                arm.hold = true;
            } else {
                st.countdown.add(1 << 6);
            }
        }
        2 if st.countdown.drain_above(0) => arm.hold = true,
        _ => {}
    }
    arm
}

/// PROT 0964's `0xB0`..`0xB2` body, the load base of its image.
pub const ELEMENT_STRIKE_BODY: u32 = crate::cast_module_ticks::ELEMENT_STRIKE_CHAIN.body;

/// One element's records, by the arms that seat them: `k = 1`'s two on the
/// caster and one at the negated point, `k = 3`'s two, `k = 4`'s run.
struct StrikeRecords {
    k1: [Option<u32>; 3],
    k3: [u32; 2],
    k4: &'static [u32],
    k4_far: Option<u32>,
}

/// Selector `2` (phases `2..=8`, `0x801F80C4..0x801F84D4`).
const FLAME: StrikeRecords = StrikeRecords {
    k1: [Some(0x801F_91E8), Some(0x801F_9258), Some(0x801F_923C)],
    k3: [0x801F_92C0, 0x801F_94A0],
    k4: &[
        0x801F_9310,
        0x801F_9374,
        0x801F_93D8,
        0x801F_943C,
        0x801F_950C,
    ],
    k4_far: Some(0x801F_94F0),
};
/// Selector `0` (phases `0x32..=0x38`, `0x801F78FC..0x801F7CC0`).
const WIND: StrikeRecords = StrikeRecords {
    k1: [Some(0x801F_9550), Some(0x801F_95D8), None],
    k3: [0x801F_9640, 0x801F_9690],
    k4: &[
        0x801F_972C,
        0x801F_9790,
        0x801F_97F4,
        0x801F_9858,
        0x801F_96E8,
    ],
    k4_far: None,
};
/// Selector `1` (phases `0x64..=0x6A`, `0x801F703C..0x801F7708`).
const THUNDER: StrikeRecords = StrikeRecords {
    k1: [Some(0x801F_98BC), Some(0x801F_9910), Some(0x801F_9978)],
    k3: [0x801F_9994, 0x801F_99E4],
    k4: &[0x801F_9A78],
    k4_far: None,
};
/// Thunder's `k = 4` ring, eight round the caster (`0x801F73C8..0x801F7444`).
const THUNDER_RING: u32 = 0x801F_9A34;
/// Thunder's `k = 5` spray pair, every pass (`0x801F76F0`, `0x801F7708`).
pub const THUNDER_SPRAY: [u32; 2] = [0x801F_9AD0, 0x801F_9B20];
/// Arm 0's records: on the caster's display trio `+0x3C..+0x40`, then at the negated point, then
/// the element's own there (`0x801F6C94..0x801F6D28`).
const OPEN_BODY: u32 = 0x801F_8FF0;
const OPEN_FAR: u32 = 0x801F_9058;
const OPEN_ELEMENT: [u32; 3] = [0x801F_90BC, 0x801F_9120, 0x801F_9184];

/// **Rogue Wind / Thunder / Flame** (the `beq` / `slti` tree at
/// `0x801F6A68`; caster `actor_table[ctx + 0x13]`). The ids run it alike; arm 1
/// forks on the last Element Change roll into three six-arm variants, base `2`,
/// `0x32` or `0x64`, and `k` is the phase over the base:
///
/// | arm | camera | countdown |
/// |---|---|---|
/// | 0 (`0x801F6BCC`) | a cut: pitch `0`, yaw `0x800 - facing`, TR `(0, 0x600, 0x600)`; the opening records | seed `scalar << 8` |
/// | 1 (`0x801F6D8C`) | TR z `+= k` a pass, every party seat hidden | `+= scalar << 7` |
/// | `k = 0` | TR z `-= 2k`, TR y `-= k >> 2` (Wind: TR z `-= 3k`, TR y `-= k >> 1`) | `+= scalar * 0x60` |
/// | `k = 1` | on its pass a cut: TR `(0, 0x300, 0x300)` (Wind: y `0x280`); records | `+= scalar << 8` |
/// | `k = 2` | TR y `+= k >> 1` (Wind: `k >> 2`) | `+= scalar << 5` |
/// | `k = 3` | on its pass a cut: pitch `0x180`, TR `(0, 0x200, 0x600)`; records | `+= scalar * 0x70` |
/// | `k = 4` | TR z `+= 16k`, TR y `-= 3k`; on its pass the blast's records | `+= scalar * 0xC0` |
/// | `k = 5` | held, Thunder's spray each pass; on its pass TR `(0, 0x500, 0xA00)` | `+= scalar << 7` |
/// | closing (`0x801F8800`) | TR z `-= k` | - |
///
/// The phase chain, the creature's clips and the hit are `ELEMENT_STRIKE_CHAIN`.
///
/// PORT: overlay_cast_element_change_0964_801f69d8 (PROT 0964 `0xB0`..`0xB2`; the camera arms and the records)
pub fn element_strike_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let mut arm = CaptureCamArm::default();
    let at_caster = [c.x, c.y, c.z];
    let far = [c.x.wrapping_neg(), 0, c.z.wrapping_neg()];
    let rot = [0, c.facing as i16, 0];
    let cut = |pitch: i16, y: i16, z: i16| ModuleShot {
        angles: [pitch, yaw_from(0x800, c.facing), 0],
        tr: [0, y, z],
        focus: focus_on(c),
        frames: 1,
    };
    let drift = |tr_y: i32, tr_z: i32| {
        Some(CaptureDrift {
            tr_y: tr_y as i16,
            tr_z: tr_z as i16,
            ..Default::default()
        })
    };
    match phase {
        0 => {
            arm.shot = Some(cut(0, 0x600, 0x600));
            st.countdown.0 = SPEED_SCALAR << 8;
            let roll = (st.module_words[0] as usize).min(2);
            // `FUN_80050ED4(caster + 0x3C, caster + 0x44, ..)`
            // (`0x801F6C70..0x801F6C94`): the display trio, whose height
            // `+0x3E` the pose decoder raises by the pose centroid - the
            // crystal's middle, where the rays fan out from.
            let body = st.caster_body.unwrap_or(c);
            arm.pass_spawns[0] = Some(seat(OPEN_BODY, [body.x, body.y, body.z], rot));
            arm.pass_spawns[1] = Some(seat(OPEN_FAR, far, rot));
            arm.pass_spawns[2] = Some(seat(OPEN_ELEMENT[roll], far, rot));
            arm.element_stream = true;
            return arm;
        }
        1 => {
            arm.drift = drift(0, SPEED_SCALAR);
            arm.party_write = CaptureSeatWrite {
                colour: Some(0),
                render_flag: Some(0xFF),
                ..Default::default()
            };
            if st.countdown.drain_above(0) {
                arm.hold = true;
            } else {
                st.countdown.add(1 << 7);
                // The arm's pass seats the element's creature and hides the
                // caster: `jal 0x801F19EC` at `0x801F6E90`, `sb s0,0x21c(s7)`
                // with `s0 = 0xFF` and `sw zero,0x4(s7)` right behind it.
                arm.seats_creature = true;
                arm.flushes_pool = true;
                arm.caster_write = CaptureSeatWrite {
                    colour: Some(0),
                    render_flag: Some(0xFF),
                    ..Default::default()
                };
            }
            return arm;
        }
        _ => {}
    }
    let (k, records, wind) = match phase {
        2..=8 => (phase - 2, &FLAME, false),
        0x32..=0x38 => (phase - 0x32, &WIND, true),
        0x64..=0x6A => (phase - 0x64, &THUNDER, false),
        _ => return arm,
    };
    arm.drift = match k {
        // Wind's: `TR y -= frame << 2`, `TR z -= 3k` (`0x801F7804..0x801F7828`).
        0 if wind => drift(-(SPEED_SCALAR >> 1), -3 * SPEED_SCALAR),
        0 => drift(-(SPEED_SCALAR >> 2), -2 * SPEED_SCALAR),
        // Wind climbs half as fast: `sra a0,a0,0x2` at `0x801F7960`.
        2 if wind => drift(SPEED_SCALAR >> 2, 0),
        2 => drift(SPEED_SCALAR >> 1, 0),
        4 => drift(-3 * SPEED_SCALAR, 16 * SPEED_SCALAR),
        6 => drift(0, -SPEED_SCALAR),
        _ => None,
    };
    if k == 5 && std::ptr::eq(records, &THUNDER) {
        arm.spray = Some(THUNDER_SPRAY);
    }
    if st.countdown.drain_above(0) {
        arm.hold = true;
        return arm;
    }
    match k {
        0 => st.countdown.add(0x60),
        1 => {
            arm.shot = Some(cut(0, if wind { 0x280 } else { 0x300 }, 0x300));
            st.countdown.add(1 << 8);
            for (slot, r) in arm.pass_spawns.iter_mut().zip(records.k1) {
                *slot = r.map(|r| seat(r, at_caster, rot));
            }
            if let Some(r) = records.k1[2] {
                arm.pass_spawns[2] = Some(seat(r, far, rot));
            }
        }
        2 => st.countdown.add(1 << 5),
        3 => {
            arm.shot = Some(cut(0x180, 0x200, 0x600));
            st.countdown.add(0x70);
            for (slot, r) in arm.pass_spawns.iter_mut().zip(records.k3) {
                *slot = Some(seat(r, at_caster, rot));
            }
        }
        4 => {
            st.countdown.add(0xC0);
            for (slot, &r) in arm.pass_spawns.iter_mut().zip(records.k4) {
                *slot = Some(seat(r, at_caster, rot));
            }
            if let Some(r) = records.k4_far {
                arm.charge_spawns[0] = Some(seat(r, far, rot));
            }
            if std::ptr::eq(records, &THUNDER) {
                for i in 0..8u16 {
                    let (s, co) = legaia_engine_system::trig::trig12(i << 9);
                    arm.charge_spawns[usize::from(i)] = Some(seat(
                        THUNDER_RING,
                        [
                            (i32::from(c.x) - i32::from(s) / 16) as i16,
                            c.y,
                            (i32::from(c.z) - i32::from(co) / 16) as i16,
                        ],
                        [0, (i << 9) as i16, 0],
                    ));
                }
            }
        }
        5 => {
            arm.shot = Some(cut(0, 0x500, 0xA00));
            st.countdown.add(1 << 7);
            // The pass flushes the pool, takes the creature off and lights
            // the caster in the element's colour, which `0xFF` holds: `0xFFC00`
            // at `0x801F7F6C`, `0x3FF` at `0x801F8784` (`0x3FF00000` for
            // Thunder, `0x801F77CC`), `+0x0C = 0x1000` beside it.
            arm.flushes_pool = true;
            arm.hides_creature = true;
            arm.caster_write = CaptureSeatWrite {
                colour: Some(if wind {
                    0x000F_FC00
                } else if std::ptr::eq(records, &THUNDER) {
                    0x3FF0_0000
                } else {
                    0x0000_03FF
                }),
                blend: Some(0x1000),
                ..Default::default()
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
    fn each_variant_arm_runs_the_same_shape() {
        for base in [2u8, 0x32, 0x64] {
            let mut st = ModuleCamState::default();
            st.countdown.0 = SPEED_SCALAR;
            let a = element_strike_camera(&mut st, base + 1, ModuleCamSeats::default());
            assert_eq!(a.shot.unwrap().tr[2], 0x300);
            assert_eq!(st.countdown.0, SPEED_SCALAR << 8);
            st.countdown.0 = SPEED_SCALAR;
            let a = element_strike_camera(&mut st, base + 3, ModuleCamSeats::default());
            assert_eq!(a.shot.unwrap().angles[0], 0x180);
        }
    }

    /// Arm 1's pass seats the creature and hides the caster; `k = 5`'s takes
    /// it off and lights the caster in each variant's colour.
    #[test]
    fn the_creature_stands_from_arm_one_to_k_five() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = SPEED_SCALAR << 4;
        let held = element_strike_camera(&mut st, 1, ModuleCamSeats::default());
        assert!(held.hold && !held.seats_creature);
        st.countdown.0 = 0;
        let pass = element_strike_camera(&mut st, 1, ModuleCamSeats::default());
        assert!(pass.seats_creature && pass.flushes_pool);
        assert_eq!(pass.caster_write.render_flag, Some(0xFF));
        for (base, word) in [(2u8, 0x3FFu32), (0x32, 0xF_FC00), (0x64, 0x3FF0_0000)] {
            st.countdown.0 = 0;
            let a = element_strike_camera(&mut st, base + 5, ModuleCamSeats::default());
            assert!(a.hides_creature && a.flushes_pool);
            assert_eq!(a.caster_write.colour, Some(word));
            assert_eq!(a.caster_write.blend, Some(0x1000));
        }
    }

    #[test]
    fn the_hide_runs_on_held_passes() {
        let mut st = ModuleCamState::default();
        let open = element_change_camera(&mut st, 0, ModuleCamSeats::default());
        assert_eq!(open.shot.unwrap().tr, [0, 0x600, 0x600]);
        let held = element_change_camera(&mut st, 1, ModuleCamSeats::default());
        assert!(held.hold);
        assert_eq!(held.party_write.render_flag, Some(0xFF));
    }
}
