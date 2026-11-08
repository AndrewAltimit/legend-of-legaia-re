//! PROT 0951's **Scythe Wind** body (`0x801F77E8`, action `0x5B`): the camera
//! arms, the slowed wind-up and the wind's records.
//!
//! See `ghidra/scripts/funcs/overlay_cast_chaos_flare_0951_801f77e8.txt`.

use super::v_windhash::seat;
use super::{CaptureCamArm, ModuleCamSeats, ModuleCamState, ModuleShot, SPEED_SCALAR, focus_on};

/// PROT 0951's Scythe Wind tick body.
pub const SCYTHE_WIND_BODY: u32 = crate::cast_module_ticks::SCYTHE_WIND_TICK;

/// The module's countdown word (`lui 0x8020` / `-0x7634`).
pub const SCYTHE_WIND_COUNTDOWN: u32 = 0x801F_89CC;

/// Arm 3's record on the victim (`0x801F7CE4`).
const WIND_RECORD: u32 = 0x801F_8348;
/// Arm 4's scattered record, four to a pass (`0x801F7F14`).
const SCATTER_RECORD: u32 = 0x801F_836C;
/// The animation rate arm 2 slows both seats to (`li v1,1` / `sb v1,0x21d`).
const SLOW_RATE: u8 = 1;
/// The rate arms 3 and 4 put each back to.
const FULL_RATE: u8 = 8;

/// The shot every held pass of arms 3 to 5 re-aims, at twelve frames: pitch
/// `0x40`, yaw `cd / 8 + base - caster[+0x46]`, TR `(0, 0x500, far - cd)`.
fn sweep(on: [i16; 3], facing: u16, cd: i32, base: i32, far: i32) -> ModuleShot {
    ModuleShot {
        angles: [0x40, (cd / 8 + base - i32::from(facing as i16)) as i16, 0],
        tr: [0, 0x500, (far - cd) as i16],
        focus: on,
        frames: 0xC,
    }
}

/// The pairs an arm writes back ([`scythe_wind_camera`]): the victim's as
/// arm 0 saved it, and from arm 3 the caster's as arm 2 saved it. `c` / `v`
/// take the written pairs, which the rest of the pass reads.
fn pin(
    st: &ModuleCamState,
    arm: &mut CaptureCamArm,
    c: &mut super::ModuleSeat,
    v: &mut super::ModuleSeat,
    caster: bool,
) {
    let [cx, cz, vx, vz] = st.horns_pin;
    if st.pin_saved & 1 != 0 {
        (v.x, v.y, v.z) = (vx, st.horns_lift, vz);
        arm.victim_write.x = Some(vx);
        arm.victim_write.y = Some(st.horns_lift);
        arm.victim_write.z = Some(vz);
    }
    if caster && st.pin_saved & 2 != 0 {
        (c.x, c.z) = (cx, cz);
        arm.caster_write.x = Some(cx);
        arm.caster_write.z = Some(cz);
    }
}

/// **Scythe Wind** (`sltiu 6` through the table at `0x801F6A08`; caster
/// `actor_table[ctx + 0x13]`, victim `actor_table[caster + 0x1DD]`):
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 0, 1 (`0x801F7874`) | the walk into reach, framed by the action SM's case 6 (the host's approach leg, `capture_approach_follows`) | - |
/// | 2 (`0x801F7B58`) | both seats' rate `1`; on the caster: pitch `0x40`, yaw `0xA00 - facing`, TR `(0, 0x500, 0x600)`, `0xC` frames | seed `scalar << 5` |
/// | 3 (`0x801F7C04`) | every pass on the caster: yaw base `0x9E0`, TR z `0x700 - cd`. On its pass, the wind's record on the victim and the caster's rate back | `+= scalar << 7` |
/// | 4 (`0x801F7D30`) | every pass on the victim: yaw base `0x960`, TR z `0xB00 - cd`; below `scalar << 6`, four scattered records a pass. On its pass, the hit and the victim's rate back | `+= scalar * 0x60` |
/// | 5 (`0x801F801C`) | every pass on the victim's body: yaw base `0x900`, TR z `0xE00 - cd`; then the victim's settle | drained twice a pass |
///
/// The phase chain is the arm table's
/// ([`crate::cast_module_ticks::SCYTHE_WIND_TICK`]); this director owns the
/// gates from arm 2 on.
///
/// The arms pin both seats' ground pairs ([`pin`]): arm 0 saves the victim's
/// `+0x34..+0x3B` (`0x801F78E8..0x801F790C`, at `0x801F89D4`) and arm 2 the
/// caster's (`0x801F7AEC..0x801F7B10`, at `0x801F89DC`); arms 0 to 2 write the
/// victim's back every pass and arms 3 and 4 both (`0x801F7BF4..0x801F7C44`,
/// `0x801F7D2C..0x801F7D78`), ahead of the shot, the wind's seat and the
/// knock-back bearing, which all read the written pairs. Arm 5 writes none,
/// and the knock-back carries the victim off.
///
/// PORT: overlay_cast_chaos_flare_0951_801f77e8 (PROT 0951 `0x5B`; the camera arms, the rates and the wind's records)
pub fn scythe_wind_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (mut c, mut v) = (seats.caster, seats.victim);
    let mut arm = CaptureCamArm::default();
    // Arm 0 runs one pass; a host that enters on arm 1 saves there.
    if phase <= 1 && st.pin_saved & 1 == 0 {
        st.horns_pin[2] = v.x;
        st.horns_pin[3] = v.z;
        st.horns_lift = v.y;
        st.pin_saved |= 1;
    }
    if phase == 2 {
        st.horns_pin[0] = c.x;
        st.horns_pin[1] = c.z;
        st.pin_saved |= 2;
    }
    if phase <= 4 {
        pin(st, &mut arm, &mut c, &mut v, phase >= 3);
    }
    match phase {
        // `0x801F79C8..0x801F79F4`, the arm's in-reach and walking paths both.
        0 => arm.vram_move = Some(super::GIMARD_BREATH_CLUT_MOVE),
        2 => {
            // `0x801F7B3C..0x801F7B64`: the caster turned onto the victim's
            // body pair (`FUN_80019B28(victim[+0x40], victim[+0x3C],
            // caster[+0x38], caster[+0x34]) + 0x800`), the heading every
            // later shot's yaw subtracts.
            let body = st.victim_body.unwrap_or(v);
            arm.caster_write.facing = Some(
                super::bearing_12bit_approx(body.z, body.x, c.z, c.x).wrapping_add(0x800) & 0xFFF,
            );
            arm.caster_write.rate = Some(SLOW_RATE);
            arm.victim_write.rate = Some(SLOW_RATE);
            arm.shot = Some(ModuleShot {
                angles: [
                    0x40,
                    (0xA00 - i32::from(arm.caster_write.facing.unwrap_or(c.facing) as i16)) as i16,
                    0,
                ],
                tr: [0, 0x500, 0x600],
                focus: focus_on(c),
                frames: 0xC,
            });
            st.countdown.0 = SPEED_SCALAR << 5;
        }
        3 => {
            let held = st.countdown.drain_above(0);
            arm.shot = Some(sweep(focus_on(c), c.facing, st.countdown.0, 0x9E0, 0x700));
            if held {
                arm.hold = true;
            } else {
                let wind = v;
                arm.pass_spawns[0] = Some(seat(
                    WIND_RECORD,
                    [wind.x, wind.y, wind.z],
                    [0, wind.facing as i16, 0],
                ));
                arm.caster_write.rate = Some(FULL_RATE);
                st.countdown.add(1 << 7);
            }
        }
        4 => {
            let held = st.countdown.drain_above(0);
            let cd = st.countdown.0;
            arm.shot = Some(sweep(focus_on(v), c.facing, cd, 0x960, 0xB00));
            if cd < SPEED_SCALAR << 6 {
                arm.scatter = Some((SCATTER_RECORD, 4));
            }
            if held {
                arm.hold = true;
            } else {
                st.countdown.0 += SPEED_SCALAR * 0x60;
                // `0x801F7FEC..0x801F8004`: the victim turned off the caster
                // (`FUN_80019B28` of the two live pairs, `+ 0x800`), so the
                // knock-back carries it away from the caster.
                arm.victim_write.rate = Some(FULL_RATE);
                arm.victim_write.facing = Some(
                    super::bearing_12bit_approx(c.z, c.x, v.z, v.x).wrapping_add(0x800) & 0xFFF,
                );
                arm.folds_hit = true;
            }
        }
        5 => {
            // The arm drains the word twice a pass: once ahead of the shot,
            // which aims on the drained value (`0x801F8058..0x801F806C`), and
            // again behind it while it is still positive
            // (`0x801F80B8..0x801F80E4`).
            let body = st.victim_body.unwrap_or(v);
            st.countdown.drain();
            let cd = st.countdown.0;
            arm.shot = Some(sweep(focus_on(body), c.facing, cd, 0x900, 0xE00));
            if (cd > 0 && st.countdown.drain_above(0)) || !st.victim_settled {
                arm.hold = true;
            } else {
                st.countdown.drain_rest(super::MODULE_DRAIN_PER_TICK);
            }
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
                x: 900,
                y: 0,
                z: 100,
                facing: 0x400,
            },
            victim: ModuleSeat {
                x: 0,
                y: 0,
                z: 700,
                facing: 0,
            },
            action: 0x5B,
            ..Default::default()
        }
    }

    #[test]
    fn the_walk_runs_on_case_six_from_the_arm_zero_yaw_store() {
        use crate::cast_module_camera::{capture_approach_follows, capture_approach_yaw_base};
        assert!(capture_approach_follows(951, Some(SCYTHE_WIND_BODY)));
        assert_eq!(
            capture_approach_yaw_base(951, Some(SCYTHE_WIND_BODY)),
            Some(0x200)
        );
        // Rolling Flare walks on case 6 with no store of its own.
        let flare = Some(crate::cast_arm_ticks::ROLLING_FLARE_TICK);
        assert!(capture_approach_follows(950, flare));
        assert_eq!(capture_approach_yaw_base(950, flare), None);
        // Steal frames on other arms, through its own director.
        assert!(!capture_approach_follows(
            941,
            Some(crate::cast_module_camera::STEAL_BODY)
        ));
    }

    #[test]
    fn arm_two_slows_both_seats_and_seeds_the_sweep() {
        let mut st = ModuleCamState::default();
        let a = scythe_wind_camera(&mut st, 2, seats());
        assert_eq!(a.caster_write.rate, Some(SLOW_RATE));
        assert_eq!(a.victim_write.rate, Some(SLOW_RATE));
        assert_eq!(st.countdown.0, SPEED_SCALAR << 5);
    }

    #[test]
    fn arm_three_reaims_every_pass_and_seats_the_wind_on_its_pass() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = SPEED_SCALAR * 2;
        let held = scythe_wind_camera(&mut st, 3, seats());
        assert!(held.hold && held.shot.is_some());
        let pass = scythe_wind_camera(&mut st, 3, seats());
        assert!(!pass.hold);
        assert_eq!(pass.pass_spawns.iter().flatten().count(), 1);
    }

    #[test]
    fn arm_four_scatters_only_near_its_end_and_lands_the_hit() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = SPEED_SCALAR << 7;
        assert!(scythe_wind_camera(&mut st, 4, seats()).scatter.is_none());
        st.countdown.0 = SPEED_SCALAR;
        let a = scythe_wind_camera(&mut st, 4, seats());
        assert_eq!(a.scatter, Some((SCATTER_RECORD, 4)));
        assert!(a.folds_hit && !a.hold);
    }

    /// Arm 0 saves the victim's pair and arm 2 the caster's; arms up to 4
    /// write them back, and the wind sits on the saved pair
    /// (`0x801F7BF4..0x801F7CFC`). Arm 5 writes none.
    #[test]
    fn the_arms_pin_both_pairs_until_the_last() {
        let mut st = ModuleCamState::default();
        scythe_wind_camera(&mut st, 0, seats());
        let mut drift = seats();
        drift.victim.x += 5;
        drift.victim.z -= 7;
        let a = scythe_wind_camera(&mut st, 1, drift);
        assert_eq!((a.victim_write.x, a.victim_write.z), (Some(0), Some(700)));
        assert_eq!(a.caster_write.x, None);
        scythe_wind_camera(&mut st, 2, seats());
        st.countdown.0 = 0;
        drift.caster.x -= 3;
        let a = scythe_wind_camera(&mut st, 3, drift);
        assert_eq!((a.caster_write.x, a.caster_write.z), (Some(900), Some(100)));
        let wind = a.pass_spawns[0].expect("the wind's record");
        assert!(matches!(
            wind.anchor,
            super::super::SpawnAnchor::LiteralScaled {
                pos: [0, 0, 700],
                ..
            }
        ));
        let a = scythe_wind_camera(&mut st, 5, drift);
        assert_eq!((a.victim_write.x, a.caster_write.x), (None, None));
    }

    /// Arm 5 drains the word twice a pass and aims on the first drain
    /// (`0x801F8058..0x801F80E4`).
    #[test]
    fn the_last_arm_drains_twice_a_pass() {
        let mut st = ModuleCamState {
            victim_settled: true,
            ..Default::default()
        };
        st.countdown.0 = 4 * super::super::MODULE_DRAIN_PER_TICK;
        let a = scythe_wind_camera(&mut st, 5, seats());
        assert!(a.hold);
        assert_eq!(st.countdown.0, 2 * super::super::MODULE_DRAIN_PER_TICK);
        let shot = a.shot.unwrap();
        assert_eq!(
            shot.tr[2],
            (0xE00 - 3 * super::super::MODULE_DRAIN_PER_TICK) as i16
        );
        assert!(!scythe_wind_camera(&mut st, 5, seats()).hold);
    }
}
