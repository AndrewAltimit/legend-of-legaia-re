//! PROT 0957's **Thunder Storm** body (`0x801F6A14`, action `0x77`): the
//! camera arms, the seat hides, the records and the party sweep.
//!
//! See `ghidra/scripts/funcs/overlay_summon_effect_table_0957_801f6a14.txt`.

use super::capture::{CaptureCamArm, CaptureDrift, CaptureSeatWrite, CastRibbonRing};
use super::v_windhash::{HIDDEN, SHOWN, seat};
use super::{ModuleCamSeats, ModuleCamState, ModuleSeat, ModuleShot, focus_on, yaw_from};

/// PROT 0957's Thunder Storm tick body.
pub const THUNDER_STORM_BODY: u32 = crate::cast_module_ticks::SUMMON_EFFECT_TICK_A;

/// The module's countdown word (`lui 0x8020` / `-0x5D88`).
pub const THUNDER_STORM_COUNTDOWN: u32 = 0x801F_A278;

/// Arm 0's record on the caster (`0x801F6BE8`).
const GATHER_RECORD: u32 = 0x801F_A06C;
/// Arm 3's two records on the caster (`0x801F6E40`, `0x801F6E58`).
const STRIKE_RECORDS: [u32; 2] = [0x801F_A0D4, 0x801F_A174];
/// Arm 3's ring of six rays (`0x801F6FF8`).
pub const STORM_RAY_RECORD: u32 = 0x801F_A194;
/// The pair arms 4 to 6 seat four times a pass on `rand()` points
/// (`0x801F711C`, `0x801F7134`).
pub const STORM_SCATTER_RECORDS: [u32; 2] = [0x801F_A1D8, 0x801F_A228];
/// The pairs a pass seats.
pub const STORM_SCATTER_PAIRS: u8 = 4;
/// The streak ribbons' trail (`li a1,0xB` at `0x801F7478`).
const RING_TRAIL: u8 = 0xB;
/// Six streaks, and rays, a sixth of a turn apart (`i * 0x1000 / 6`).
const RING_COUNT: usize = 6;
/// Arm 6 stops widening the ring past this word (`slti v0,v1,0xC1`).
const RING_WIDEST: i32 = 0xC0;
/// The rates the body stores: the caster's wind-up and strike, then full, and
/// the party's through the closing arm (`li a1,6` at `0x801F7668`).
const WIND_UP_RATE: u8 = 2;
const STRIKE_RATE: u8 = 1;
const FULL_RATE: u8 = 8;
const PARTY_CLOSE_RATE: u8 = 6;
/// The fade-out word's ceiling (`slti v0,v0,0x1001`).
const FADE_FULL: i32 = 0x1000;

/// The ring's six points at `word` (arm 5's and 6's `FUN_801E1D98` calls,
/// `0x801F73E8..0x801F749C`): `angle = i * 0x1000 / 6 + (word & 0xFF)`,
/// `x + sin * word / 128`, `z + cos * word / 128`.
fn storm_ring(caster: ModuleSeat, word: i32) -> CastRibbonRing {
    let mut points = [[0i16; 3]; 16];
    for (i, p) in points.iter_mut().take(RING_COUNT).enumerate() {
        let angle = (i as i32 * 0x1000 / RING_COUNT as i32) + (word & 0xFF);
        let (sin, cos) = legaia_engine_system::trig::trig12(angle as u16);
        *p = [
            (i32::from(caster.x) + i32::from(sin) * word / 128) as i16,
            0,
            (i32::from(caster.z) + i32::from(cos) * word / 128) as i16,
        ];
    }
    CastRibbonRing {
        points,
        trail_id: RING_TRAIL,
        count: RING_COUNT as u8,
    }
}

/// The ring word's gain a battle frame: `+= *(0x1F800393)`, the frame step,
/// once a pass (arm 5 `0x801F73D0..0x801F73E4`, arm 6 `0x801F76E4..0x801F76F0`).
fn ring_step(st: &ModuleCamState) -> i32 {
    i32::from(st.frame_step.max(1))
}

/// **Thunder Storm** (a `beq` / `slti` chain over `0..=6` and `0xFF`; caster
/// `actor_table[ctx + 0x13]`). The word starts at `0x600` and drains
/// `8 * delta` a pass, so a passing arm's seed over eight is its dwell:
///
/// | arm | camera and seats | word |
/// |---|---|---|
/// | 0 (`0x801F6AF8`) | every live seat hidden, the caster shown at rate `2`; pitch `0`, yaw `0x800 - facing`, TR `(0, 0x500, depth / 2)`, `0xC` frames, on the caster's body; the gather record | `= 0x600` |
/// | 1 (`0x801F6BF8`) | TR z `+16`; then clip `0x0A` and the cut: pitch `0x400`, yaw `-facing`, TR `(0, 0, depth / 2)`; the living party shown | `+= 0x400` |
/// | 2 (`0x801F6D28`) | TR z `+0xC0`, TR y `-8` | `+= 0x100` |
/// | 3 (`0x801F6D9C`) | gate; the living party hidden, the caster's rate `1`, the pool flushed, two records and the ring of rays | `+= 0x800` |
/// | 4 (`0x801F7048`) | TR z `-32`, yaw `+8`, the scatter; then the caster's rate `8` and clip `0`, a cut: pitch `0x80`, yaw `0x800 - facing`, TR `(0, 0, 0x1000)`; the living party shown | `+= 0x200` |
/// | 5 (`0x801F72D0`) | TR z `+128`, the scatter, the widening ribbons; then the party's hit | ring `+= step` |
/// | 6 (`0x801F7658`) | every party rate `6`, TR z `+16`, the ribbons while the ring word is at most `0xC0`, the scatter; every part's level `+0x78` climbs `16` a vsync to `0x1000`, then the flush | ring `+= step`; `= 0x200` |
/// | `0xFF` (`0x801F78F0`) | every live seat shown at rate `8`; done | - |
///
/// [`ModuleCamState::module_words`] holds `[ring word 0x801FA27C, fade word
/// 0x801FA280]`. The ring word gains the frame step once a battle frame
/// ([`ring_step`]), so a step-`4` pass's ring stands four words out, and the
/// ring's radius (`32` units a word) and with it each ribbon's projected
/// segment count, and so its `rand()` draws, follow retail's. This director owns the phase: nothing else ports the body.
/// The armed washes, the screen fades and the stage byte are the
/// presentation decoder's (`legaia_overlay_images::cast_presentation`).
///
/// PORT: FUN_801F6A14, overlay_summon_effect_table_0957_801f6a14 (PROT 0957 `0x77`; the whole body)
pub fn thunder_storm_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let body = st.caster_body.unwrap_or(c);
    let depth = i32::from(seats.depth_raw as i16);
    let drift = |tr_z: i16, tr_y: i16, yaw: i16| {
        Some(CaptureDrift {
            tr_z,
            tr_y,
            yaw,
            ..Default::default()
        })
    };
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    let gate = |st: &mut ModuleCamState| st.countdown.drain_above(0);
    match phase {
        0 => {
            arm.others_write = HIDDEN;
            arm.others_live_only = true;
            arm.caster_write = CaptureSeatWrite {
                rate: Some(WIND_UP_RATE),
                ..SHOWN
            };
            arm.shot = Some(ModuleShot {
                angles: [0, yaw_from(0x800, c.facing), 0],
                tr: [0, 0x500, (depth / 2) as i16],
                focus: focus_on(body),
                frames: 0xC,
            });
            arm.pass_spawns[0] = Some(seat(
                GATHER_RECORD,
                [c.x, c.y, c.z],
                [0, c.facing as i16, 0],
            ));
            st.countdown.0 = 0x600;
            st.module_words = [0, 0];
            arm.next = Some(1);
        }
        1 => {
            arm.drift = drift(16, 0, 0);
            if gate(st) {
                arm.hold = true;
            } else {
                st.countdown.0 += 0x400;
                arm.shot = Some(ModuleShot {
                    angles: [0x400, yaw_from(0, c.facing), 0],
                    tr: [0, 0, (depth / 2) as i16],
                    focus: focus_on(body),
                    frames: 1,
                });
                arm.party_write = SHOWN;
                arm.party_live_only = true;
                arm.next = Some(2);
            }
        }
        2 => {
            arm.drift = drift(0xC0, -8, 0);
            if gate(st) {
                arm.hold = true;
            } else {
                st.countdown.0 += 0x100;
                arm.next = Some(3);
            }
        }
        3 => {
            if gate(st) {
                arm.hold = true;
            } else {
                arm.party_write = HIDDEN;
                arm.party_live_only = true;
                arm.caster_write.rate = Some(STRIKE_RATE);
                let on = [c.x, c.y, c.z];
                let rot = [0, c.facing as i16, 0];
                arm.charge_spawns[0] = Some(seat(STRIKE_RECORDS[0], on, rot));
                arm.charge_spawns[1] = Some(seat(STRIKE_RECORDS[1], on, rot));
                for i in 0..RING_COUNT {
                    let angle = i as i32 * 0x1000 / RING_COUNT as i32;
                    let (sin, cos) = legaia_engine_system::trig::trig12(angle as u16);
                    arm.charge_spawns[2 + i] = Some(seat(
                        STORM_RAY_RECORD,
                        [c.x.wrapping_sub(sin / 16), 0, c.z.wrapping_sub(cos / 16)],
                        [0, angle as i16, 0],
                    ));
                }
                st.countdown.0 += 0x800;
                arm.next = Some(4);
            }
        }
        4 => {
            arm.drift = drift(-32, 0, 8);
            arm.scatter_pairs = true;
            if gate(st) {
                arm.hold = true;
            } else {
                arm.caster_write = CaptureSeatWrite {
                    rate: Some(FULL_RATE),
                    stage: Some(0),
                    restage: true,
                    ..Default::default()
                };
                arm.shot = Some(ModuleShot {
                    angles: [0x80, yaw_from(0x800, c.facing), 0],
                    tr: [0, 0, 0x1000],
                    focus: focus_on(body),
                    frames: 1,
                });
                st.countdown.0 += 0x200;
                st.module_words[0] = 0;
                arm.party_write = SHOWN;
                arm.party_live_only = true;
                arm.next = Some(5);
            }
        }
        5 => {
            arm.drift = drift(128, 0, 0);
            arm.scatter_pairs = true;
            if st.opens_pass {
                st.module_words[0] += ring_step(st);
            }
            arm.ribbons = Some(storm_ring(c, st.module_words[0]));
            // The arm's ribbons take their height off the stack word its
            // scatter loop left (`sh v0,0x22(sp)` at `0x801F73A4`).
            arm.ribbons_on_scatter_y = true;
            if gate(st) {
                arm.hold = true;
            } else {
                arm.folds_hit = true;
                arm.party_faces_caster = true;
                st.module_words[1] = 0;
                arm.next = Some(6);
            }
        }
        6 => {
            arm.party_write.rate = Some(PARTY_CLOSE_RATE);
            arm.drift = drift(16, 0, 0);
            if st.opens_pass && st.module_words[0] <= RING_WIDEST {
                st.module_words[0] += ring_step(st);
                st.ring_widening = true;
            } else if st.opens_pass {
                st.ring_widening = false;
            }
            if st.ring_widening {
                arm.ribbons = Some(storm_ring(c, st.module_words[0]));
            }
            arm.scatter_pairs = true;
            st.module_words[1] += 16;
            if st.module_words[1] > FADE_FULL {
                st.module_words[1] = FADE_FULL;
                st.countdown.0 = 0x200;
                arm.next = Some(0xFF);
            } else {
                arm.hold = true;
            }
            arm.parts_level = Some(st.module_words[1] as u16);
        }
        // `0x801F78F0`: the Done arm. Its seat loop walks all seven seats.
        _ => {
            let shown = CaptureSeatWrite {
                rate: Some(FULL_RATE),
                ..SHOWN
            };
            arm.others_write = shown;
            arm.others_live_only = true;
        }
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cast_module_camera::SPEED_SCALAR;

    fn seats() -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 100,
                y: 0,
                z: -200,
                facing: 0x400,
            },
            action: 0x77,
            depth_raw: 0xC00,
            ..Default::default()
        }
    }

    /// The arm dwells the retail recording reads off its game clock: `192`,
    /// `128`, `32`, `256`, `64` vsyncs, then `256` on the fade word.
    #[test]
    fn the_arms_dwell_retails_lengths() {
        let mut st = ModuleCamState::default();
        let mut dwell = [0u32; 7];
        let mut phase = 0u8;
        for _ in 0..4000 {
            if phase == 0xFF {
                break;
            }
            dwell[usize::from(phase)] += 1;
            let a = thunder_storm_camera(&mut st, phase, seats());
            if !a.hold {
                phase = a.next.expect("every arm before 0xFF has a next");
            }
        }
        assert_eq!(phase, 0xFF);
        assert_eq!(SPEED_SCALAR, 8);
        assert_eq!(
            dwell,
            [
                1,
                0x600 / 8,
                0x400 / 8,
                0x100 / 8,
                0x800 / 8,
                0x200 / 8,
                257
            ]
        );
    }

    /// The hit lands on the pass that leaves arm 5, with the party turned on
    /// the caster; the Done arm has no next.
    #[test]
    fn the_party_is_struck_leaving_arm_5() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = 8;
        let a = thunder_storm_camera(&mut st, 5, seats());
        assert!(a.folds_hit && a.party_faces_caster && !a.hold);
        assert_eq!(a.next, Some(6));
        let done = thunder_storm_camera(&mut st, 0xFF, seats());
        assert_eq!(done.next, None);
        assert_eq!(done.others_write.rate, Some(8));
    }

    /// The ring's first streak sits on the caster at word `0` and walks out
    /// with the word.
    #[test]
    fn the_ring_widens_with_its_word() {
        let c = seats().caster;
        let r = storm_ring(c, 0);
        assert_eq!((r.points[0][0], r.points[0][2]), (c.x, c.z));
        assert_eq!((r.count, r.trail_id), (6, 0xB));
        let r = storm_ring(c, 128);
        let (sin, cos) = legaia_engine_system::trig::trig12(0x80);
        assert_eq!(i32::from(r.points[0][0]), i32::from(c.x) + i32::from(sin));
        assert_eq!(i32::from(r.points[0][2]), i32::from(c.z) + i32::from(cos));
    }

    /// The ring word gains the frame step on the tick that opens a battle
    /// frame and nothing on the rest of its ticks (`0x801F73D0..0x801F73E4`).
    #[test]
    fn the_ring_word_gains_the_step_once_a_frame() {
        let mut st = ModuleCamState {
            frame_step: 4,
            ..Default::default()
        };
        st.countdown.0 = 0x7FFF_0000;
        let seats = ModuleCamSeats::default();
        for pass in 1..=3 {
            for tick in 0..4 {
                st.opens_pass = tick == 0;
                let arm = thunder_storm_camera(&mut st, 5, seats);
                assert!(arm.ribbons.is_some());
                assert_eq!(st.module_words[0], 4 * pass);
            }
        }
    }
}
