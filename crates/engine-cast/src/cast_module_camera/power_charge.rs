//! PROT 0955's **Power Charge** body (`0x801F7158`, action `0x72`): the
//! camera arms, the aura record and the seat hides.
//!
//! See `ghidra/scripts/funcs/overlay_cast_white_shield_0955_801f7158.txt`.

use super::v_windhash::{HIDDEN, SHOWN};
use super::{
    CaptureCamArm, CaptureDrift, CaptureSeatWrite, MODULE_DRAIN_PER_TICK, ModuleCamSeats,
    ModuleCamState, ModuleShot, ModuleSpawn, SPEED_SCALAR, SpawnAnchor, SpawnRecord, focus_on,
    yaw_from,
};

/// PROT 0955's Power Charge tick body, the trampoline's `0x72` arm.
pub const POWER_CHARGE_BODY: u32 = crate::cast_module_ticks::POWER_CHARGE_TICK;

/// The record arm 0 seats on the caster seat (`a0 = caster + 0x34`,
/// `a1 = caster + 0x44`, scale `0x1000`, `0x801F7314..0x801F733C`).
pub const POWER_CHARGE_RECORD: u32 = 0x801F_972C;

/// The clip arm 0 stages on the caster, with the `+0x1DC` bump
/// (`li v1,0xa` / `sb v1,0x1da(s1)`, `0x801F72A8..0x801F72B4`).
const CHARGE_CLIP: u8 = 0x0A;

/// The caster's tint state through the charge (`li v0,0x7` /
/// `sb v0,0x21c(s1)` at `0x801F7328`).
const CHARGE_TINT: u8 = 7;

/// `ctx[+0x6D0] / 2` as `srl 31` / `addu` / `sra 1` take it.
fn half(depth_raw: i32) -> i16 {
    (i32::from(depth_raw as i16) / 2) as i16
}

/// **Power Charge** (PROT 0955, body `0x801F7158`, a `beq` / `slti` chain
/// over `0..=3` and `0xFF` at `0x801F71D4..0x801F721C`; caster
/// `actor_table[ctx + 0x13]`):
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 0 (`0x801F7224`) | a shot on the caster's display pair `+0x3C` / `+0x40`: pitch `-0x80`, yaw `0x800 - caster[+0x46]`, TR `(0, 0x300, ctx[+0x6D0] / 2)`, `0xC` frames; clip `0x0A` staged with the bump; every seat with HP hidden (`+0x04 = 0`, `+0x21C = 0xFF`), then the caster shown on tint `7`; the record `0x801F972C` on the caster | `= scalar * 0x60` |
/// | 1 (`0x801F734C`) | the staged byte cleared; every pass pitch `+2 * delta`, TR y and TR z `+scalar * delta` (`0x801F7394..0x801F73B8`). Once the gate passes, a cut on the caster's live pair: pitch `0x80`, the same yaw, TR `(0, 0x300, ctx[+0x6D0])`, `1` frame | `+= scalar << 7` |
/// | 2 (`0x801F743C`) | every pass TR z `+scalar * delta`; past the gate the buff, the caster's tint off | `+= scalar * 0x60` |
/// | 3 (`0x801F7544`) | the same drift; the gate, then the target's settle | - |
/// | `0xFF` (`0x801F7604`) | every seat with HP shown (`+0x04 = 0x20080200`, `+0x21C = 0`) | - |
///
/// The phase chain and the buff are
/// [`crate::cast_module_ticks::power_charge_tick`]'s, so `next` stays `None`.
///
/// PORT: overlay_cast_white_shield_0955_801f7158 (PROT 0955 `0x72`; the camera arms, the record and the seat hides)
pub fn power_charge_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let depth = seats.depth_raw;
    let mut arm = CaptureCamArm {
        others_live_only: true,
        ..Default::default()
    };
    let push = CaptureDrift {
        tr_z: MODULE_DRAIN_PER_TICK as i16,
        ..CaptureDrift::default()
    };
    match phase {
        0 => {
            let body = st.caster_body.unwrap_or(c);
            arm.shot = Some(ModuleShot {
                angles: [-0x80, yaw_from(0x800, c.facing), 0],
                tr: [0, 0x300, half(depth)],
                focus: focus_on(body),
                frames: 0xC,
            });
            st.countdown.0 = SPEED_SCALAR * 0x60;
            arm.others_write = HIDDEN;
            arm.caster_write = CaptureSeatWrite {
                stage: Some(CHARGE_CLIP),
                restage: true,
                render_flag: Some(CHARGE_TINT),
                ..SHOWN
            };
            arm.pass_spawns[0] = Some(ModuleSpawn {
                record: SpawnRecord::Module(POWER_CHARGE_RECORD),
                anchor: SpawnAnchor::CasterShifted { dy: 0, dz: 0 },
            });
        }
        1 => {
            arm.caster_write.stage = Some(0);
            arm.drift = Some(CaptureDrift {
                pitch: 2,
                tr_y: MODULE_DRAIN_PER_TICK as i16,
                tr_z: MODULE_DRAIN_PER_TICK as i16,
                ..CaptureDrift::default()
            });
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            arm.shot = Some(ModuleShot {
                angles: [0x80, yaw_from(0x800, c.facing), 0],
                tr: [0, 0x300, depth as i16],
                focus: focus_on(c),
                frames: 1,
            });
            st.countdown.add(1 << 7);
        }
        2 => {
            arm.drift = Some(push);
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            st.countdown.add(0x60);
        }
        3 => {
            arm.drift = Some(push);
            if st.countdown.drain_above(0) || !st.victim_settled {
                arm.hold = true;
            }
        }
        0xFF => arm.others_write = SHOWN,
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
                x: 600,
                y: 0,
                z: 800,
                facing: 0x900,
            },
            depth_raw: 3008,
            ..Default::default()
        }
    }

    /// Arm 0 frames the caster's body, hides the field, shows the caster on
    /// tint 7 with the charge clip and seats the aura on it.
    #[test]
    fn arm_0_hides_the_field_and_seats_the_aura() {
        let mut st = ModuleCamState {
            caster_body: Some(ModuleSeat {
                x: 610,
                y: 0,
                z: 790,
                facing: 0x900,
            }),
            ..Default::default()
        };
        let a = power_charge_camera(&mut st, 0, seats());
        let s = a.shot.unwrap();
        assert_eq!(s.angles, [-0x80, 0xF00, 0]);
        assert_eq!(s.tr, [0, 0x300, 1504]);
        assert_eq!(s.focus, [-610, 0, -790]);
        assert_eq!(s.frames, 0xC);
        assert_eq!(st.countdown.0, SPEED_SCALAR * 0x60);
        assert_eq!(a.others_write, HIDDEN);
        assert!(a.others_live_only);
        assert_eq!(a.caster_write.render_flag, Some(7));
        assert_eq!(a.caster_write.colour, Some(0x2008_0200));
        assert_eq!(a.caster_write.stage, Some(0x0A));
        assert!(a.caster_write.restage);
        assert_eq!(
            a.pass_spawns[0].map(|s| s.record),
            Some(SpawnRecord::Module(POWER_CHARGE_RECORD))
        );
    }

    /// Arm 1 drifts every pass and cuts once the word drains; arm 2 re-arms;
    /// `0xFF` shows the field again.
    #[test]
    fn the_arms_drift_hold_and_show() {
        let mut st = ModuleCamState::default();
        power_charge_camera(&mut st, 0, seats());
        let mut held = 0;
        loop {
            let a = power_charge_camera(&mut st, 1, seats());
            assert_eq!(a.drift.unwrap().pitch, 2);
            if !a.hold {
                let s = a.shot.unwrap();
                assert_eq!(s.angles[0], 0x80);
                assert_eq!(s.tr, [0, 0x300, 3008]);
                assert_eq!(s.frames, 1);
                break;
            }
            held += 1;
        }
        assert_eq!(held, 0x5F);
        assert_eq!(st.countdown.0, SPEED_SCALAR << 7);
        let a = power_charge_camera(&mut st, 0xFF, seats());
        assert_eq!(a.others_write, SHOWN);
    }
}
