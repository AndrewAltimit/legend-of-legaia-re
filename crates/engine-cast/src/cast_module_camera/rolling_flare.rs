//! PROT 0950's **Rolling Flare** body (`0x801F79F8`, action `0x5A`): the long
//! walk, the shot on the victim and its unwinding orbit.
//!
//! See `ghidra/scripts/funcs/overlay_cast_rolling_flare_0950_801f79f8.txt`.

use super::{
    CaptureCamArm, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleSeat, ModuleShot,
    SPEED_SCALAR, bearing_12bit_approx, focus_on, yaw_from,
};

/// PROT 0950's Rolling Flare tick body.
pub const ROLLING_FLARE_BODY: u32 = crate::cast_arm_ticks::ROLLING_FLARE_TICK;

/// What arm 0 stores for the walk (`li v0,0x34` / `sb v0,0x1F(record)` at
/// `0x801F7AE0`, `li v0,0x680` / `sh v0,0x58(<+0x22C>)` at `0x801F7AEC`): the
/// range poll stops the caster at `0x34 << 4` units of its victim, and the
/// separation pass stands it off by the same body radius.
pub const ROLLING_FLARE_WALK_REACH: (u8, i16) = (0x34, 0x680);
/// What arm 2 stores (`0x801F7E04`, `0x801F7E14`).
const STRIKE_REACH: (u8, i16) = (8, 0x100);
/// What the finishing arm stores back (`0x801F8154`, `0x801F8160`): literals,
/// not a saved copy.
const REST_REACH: (u8, i16) = (0x25, 0x4A0);

fn orbit(on: ModuleSeat, facing: u16, cd: i32, base: i32, far: i32) -> ModuleShot {
    let half = cd / 2;
    ModuleShot {
        angles: [-0x20, (-i32::from(facing as i16) - (half - base)) as i16, 0],
        tr: [0, 0x400, (far - half) as i16],
        focus: focus_on(on),
        frames: 0xC,
    }
}

/// **Rolling Flare** (`sltiu 5` through the table at `0x801F6A10`; caster
/// `s3`, victim `s2`). Arms 0 and 1 are the walk the approach leg carries,
/// with [`ROLLING_FLARE_WALK_REACH`].
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 2 (`0x801F7CF0`) | the caster's pair kept, the victim's put back; the caster turns on the victim; a shot on the victim: pitch `-0x20`, yaw `0x600 - facing`, TR `(0, 0x400, 0x800)`, `0xC` frames | `= scalar << 7` |
/// | 3 (`0x801F7E20`) | both pairs held; every pass on the victim's live pair: yaw `0x800 - cd / 2 - facing`, TR z `0xA00 - cd / 2`; the `0x100` hit on the gate | `+= scalar << 6` |
/// | 4 (`0x801F8028`) | every pass on the victim's body pair: yaw `0x900 - cd / 2 - facing`, TR z `0xB00 - cd / 2`; the gate, then the victim's settle | drained twice a pass |
///
/// PORT: overlay_cast_rolling_flare_0950_801f79f8 (PROT 0950 `0x5A`; the camera arms, the seat pins and the walk's reach)
pub fn rolling_flare_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (c, v) = (seats.caster, seats.victim);
    let mut arm = CaptureCamArm::default();
    match phase {
        2 => {
            let target = st.victim_body.unwrap_or(v);
            let facing =
                bearing_12bit_approx(target.z, target.x, c.z, c.x).wrapping_add(0x800) & 0xFFF;
            arm.caster_write = CaptureSeatWrite {
                facing: Some(facing),
                ..Default::default()
            };
            st.horns_pin = [v.x, v.z, c.x, c.z];
            arm.shot = Some(ModuleShot {
                angles: [-0x20, yaw_from(0x600, facing), 0],
                tr: [0, 0x400, 0x800],
                focus: focus_on(v),
                frames: 0xC,
            });
            arm.caster_reach = Some(STRIKE_REACH);
            st.countdown.0 = SPEED_SCALAR << 7;
        }
        3 => {
            let [vx, vz, cx, cz] = st.horns_pin;
            arm.victim_write = CaptureSeatWrite {
                x: Some(vx),
                z: Some(vz),
                ..Default::default()
            };
            arm.caster_write = CaptureSeatWrite {
                x: Some(cx),
                z: Some(cz),
                ..Default::default()
            };
            let held = st.countdown.drain_above(0);
            arm.shot = Some(orbit(v, c.facing, st.countdown.0, 0x800, 0xA00));
            if held {
                arm.hold = true;
            } else {
                st.countdown.add(1 << 6);
                arm.folds_hit = true;
            }
        }
        4 => {
            // Two drains a pass: one ahead of the shot, which aims on it
            // (`0x801F8088..0x801F8090`), one behind it while the word stays
            // positive (`0x801F80B8..0x801F80E4`).
            let body = st.victim_body.unwrap_or(v);
            st.countdown.drain();
            arm.shot = Some(orbit(body, c.facing, st.countdown.0, 0x900, 0xB00));
            let held = st.countdown.0 > 0 && st.countdown.drain_above(0);
            if held || !st.victim_settled {
                arm.hold = true;
            } else {
                // The pass's first drain, the one ahead of the shot.
                st.countdown.drain_rest(super::MODULE_DRAIN_PER_TICK);
                arm.caster_reach = Some(REST_REACH);
            }
        }
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
                x: 637,
                y: 0,
                z: 326,
                facing: 0x400,
            },
            victim: ModuleSeat {
                x: -46,
                y: 0,
                z: -14,
                facing: 0,
            },
            action: 0x5A,
            ..Default::default()
        }
    }

    #[test]
    fn the_orbit_unwinds_with_the_countdown() {
        let mut st = ModuleCamState::default();
        let first = rolling_flare_camera(&mut st, 2, seats());
        assert_eq!(first.shot.unwrap().tr, [0, 0x400, 0x800]);
        assert_eq!(st.countdown.0, SPEED_SCALAR << 7);
        let a = rolling_flare_camera(&mut st, 3, seats()).shot.unwrap();
        assert!(a.tr[2] == (0xA00 - st.countdown.0 / 2) as i16);
        st.countdown.0 = 0;
        let pass = rolling_flare_camera(&mut st, 3, seats());
        assert!(!pass.hold);
        assert!(pass.folds_hit);
        assert!(st.countdown.0 > 0);
    }

    #[test]
    fn the_settle_arm_stores_the_rest_reach() {
        let mut st = ModuleCamState {
            victim_settled: true,
            ..Default::default()
        };
        assert_eq!(
            rolling_flare_camera(&mut st, 4, seats()).caster_reach,
            Some(REST_REACH)
        );
        st.victim_settled = false;
        assert!(rolling_flare_camera(&mut st, 4, seats()).hold);
    }
}
