//! PROT 0955's **Kiss of Death** body (`0x801F86A4`, action `0x6E`): the
//! camera arms and the countdown that paces them.

use super::{
    CaptureCamArm, CaptureDrift, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleShot,
    bearing_12bit_approx, focus_on, yaw_from,
};

/// PROT 0955's Kiss of Death tick body, the trampoline's `0x6E` arm.
pub const KISS_OF_DEATH_BODY: u32 = 0x801F_86A4;

/// PROT 0955's countdown word (`lui 0x8020` / `-0x62D8`).
pub const KISS_OF_DEATH_COUNTDOWN: u32 = 0x801F_9D28;

/// `ctx[+0x6D0] / 3` as `mult 0x55555556` / `mfhi` / `subu sign` take it:
/// the quotient rounded toward zero.
fn third(depth_raw: i32) -> i16 {
    (i32::from(depth_raw as i16) / 3) as i16
}

/// `ctx[+0x6D0] / 2` as `srl 31` / `addu` / `sra 1` take it: rounded toward
/// zero.
fn half(depth_raw: i32) -> i16 {
    (i32::from(depth_raw as i16) / 2) as i16
}

/// **Kiss of Death** (PROT 0955, body `0x801F86A4`, a `beq` / `slti` chain
/// over `0..=5` and `0xFF` at `0x801F8728..0x801F8788`; caster
/// `actor_table[ctx + 0x13]`, victim `actor_table[caster + 0x1DD]`):
///
/// | arm | camera | countdown |
/// |---|---|---|
/// | 0 (`0x801F878C`) | shot on the caster: pitch `-0x20`, yaw `0x800 - caster[+0x46]`, TR `(0, 0x400, ctx[+0x6D0] / 3)`, `0xC` frames (`0x801F883C`) | `= scalar * 12` (`0x801F8878`) |
/// | 1 (`0x801F88C8`) | once the gate passes, shot on the caster: pitch `0x80`, the same yaw, TR `(0, 0x400, ctx[+0x6D0] / 2)`, `0xA0` frames (`0x801F8A10`) | `+= scalar << 7` |
/// | 2 (`0x801F8A38`) | - | `+= scalar << 5` |
/// | 3 (`0x801F8B84`) | turns the victim to face the caster (`FUN_80019B28` + `0x800` into `+0x46`), then a cut on it: pitch `0`, yaw `0x800 - victim[+0x46]`, TR `(0, 0x500, 0x600)`, `1` frame (`0x801F8C1C`) | `+= scalar << 7` |
/// | 4 (`0x801F8C68`) | drift TR z `+scalar * delta` a pass, held or not (`0x801F8C90..0x801F8CA8`) | `+= scalar << 7` |
/// | 5 (`0x801F8DCC`) | the same drift | - |
///
/// Every arm from 1 on drains the word by `scalar * delta` and holds while
/// it stays positive (`bgtz` to the epilogue with the busy `1`). The phase
/// chain - the roll, the status mark and the one-point hit of arm 4, arm 5's
/// settle - is ported
/// ([`crate::cast_module_ticks::kiss_of_death_tick`]), so `next` stays
/// `None`; this director owns the gates.
///
/// `battle_map01_lippian_kiss_of_death_mid_cast` sits in arm 1 with retail's
/// camera mid-way through arm 0's twelve-frame tween onto pitch `-0x20`, yaw
/// `0x800 - 2278`, TR z `prescale(3008 / 3) = 1603`.
///
/// PORT: overlay_cast_white_shield_0955_801f86a4 (PROT 0955 `0x6E`; the camera arms and the countdown)
pub fn kiss_of_death_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let on_caster = |pitch: i16, tr_z: i16, frames: u16| ModuleShot {
        angles: [pitch, yaw_from(0x800, c.facing), 0],
        tr: [0, 0x400, tr_z],
        focus: focus_on(c),
        frames,
    };
    // `0x800840C0 += scalar * delta`, stored ahead of the gate.
    let push = (phase == 4 || phase == 5).then_some(CaptureDrift {
        tr_z: super::MODULE_DRAIN_PER_TICK as i16,
        ..CaptureDrift::default()
    });
    if phase == 0 {
        st.countdown.0 = super::SPEED_SCALAR * 12;
        return CaptureCamArm {
            shot: Some(on_caster(-0x20, third(seats.depth_raw), 0xC)),
            ..Default::default()
        };
    }
    if !(1..=5).contains(&phase) {
        return CaptureCamArm::default();
    }
    if st.countdown.drain_above(0) {
        return CaptureCamArm {
            drift: push,
            hold: true,
            ..Default::default()
        };
    }
    match phase {
        1 => st.countdown.add(1 << 7),
        2 => st.countdown.add(1 << 5),
        3 | 4 => st.countdown.add(1 << 7),
        _ => {}
    }
    let mut arm = CaptureCamArm {
        drift: push,
        ..Default::default()
    };
    match phase {
        1 => arm.shot = Some(on_caster(0x80, half(seats.depth_raw), 0xA0)),
        3 => {
            let v = seats.victim;
            let facing = (bearing_12bit_approx(c.z, c.x, v.z, v.x).wrapping_add(0x800)) & 0xFFF;
            arm.victim_write = CaptureSeatWrite {
                facing: Some(facing),
                ..Default::default()
            };
            arm.shot = Some(ModuleShot {
                angles: [0, yaw_from(0x800, facing), 0],
                tr: [0, 0x500, 0x600],
                focus: focus_on(v),
                frames: 1,
            });
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
                x: 600,
                y: 0,
                z: 813,
                facing: 2278,
            },
            victim: ModuleSeat {
                x: 0,
                y: 0,
                z: -812,
                facing: 0,
            },
            depth_raw: 3008,
            ..Default::default()
        }
    }

    #[test]
    fn arm_zero_frames_the_caster_on_a_third_of_the_depth() {
        let mut st = ModuleCamState::default();
        let a = kiss_of_death_camera(&mut st, 0, seats());
        let shot = a.shot.unwrap();
        assert_eq!(shot.angles, [-0x20, 3866, 0]);
        assert_eq!(shot.tr, [0, 0x400, 1002]);
        assert_eq!(shot.focus, [-600, 0, -813]);
        assert_eq!(shot.frames, 0xC);
        assert!(!a.hold);
        assert_eq!(st.countdown.0, super::super::SPEED_SCALAR * 12);
    }

    #[test]
    fn arm_one_holds_twelve_ticks_then_cuts_the_long_pull_in() {
        let mut st = ModuleCamState::default();
        kiss_of_death_camera(&mut st, 0, seats());
        for _ in 0..11 {
            let a = kiss_of_death_camera(&mut st, 1, seats());
            assert!(a.hold && a.shot.is_none());
        }
        let a = kiss_of_death_camera(&mut st, 1, seats());
        assert!(!a.hold);
        let shot = a.shot.unwrap();
        assert_eq!(
            (shot.angles[0], shot.tr[2], shot.frames),
            (0x80, 1504, 0xA0)
        );
    }

    #[test]
    fn arms_four_and_five_push_the_camera_back_every_pass() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = 100;
        for phase in [4, 5] {
            let a = kiss_of_death_camera(&mut st, phase, seats());
            assert_eq!(
                a.drift.unwrap().tr_z,
                super::super::MODULE_DRAIN_PER_TICK as i16
            );
        }
    }
}
