//! PROT 0955's **Kiss of Death** body (`0x801F86A4`, action `0x6E`): the
//! camera arms and the countdown that paces them.

use super::{
    CaptureCamArm, CaptureDrift, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleShot,
    ModuleSpawn, SpawnAnchor, SpawnRecord, bearing_12bit_approx, focus_on, yaw_from,
};

/// Arm 0's two records on the caster (`0x801F87A4..0x801F87C8`).
pub const KISS_OF_DEATH_ARM0_RECORDS: [u32; 2] = [0x801F_9440, 0x801F_94E8];
/// The record arms 1 and 2 seat on the caster every other pass
/// (`0x801F893C..0x801F8950`, `0x801F8AA8..0x801F8ABC`).
pub const KISS_OF_DEATH_PASS_RECORD: u32 = 0x801F_9568;

/// A record on the caster seat itself: `a0 = caster + 0x34`,
/// `a1 = caster + 0x44`, scale `0x1000`.
const fn on_caster_seat(record: u32) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor: SpawnAnchor::CasterShifted { dy: 0, dz: 0 },
    }
}

/// PROT 0955's Kiss of Death tick body, the trampoline's `0x6E` arm.
pub const KISS_OF_DEATH_BODY: u32 = 0x801F_86A4;

/// Arm 0's shot length: the builder `FUN_801D829C` (`jal` at `0x801F883C`)
/// takes `a3 = 0xC` frames (`li a3,0xc` at `0x801F8808`).
pub const KISS_OF_DEATH_ARM0_SHOT_FRAMES: u16 = 0xC;

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
        st.pass_count = 0;
        let [a, b] = KISS_OF_DEATH_ARM0_RECORDS;
        let mut pass_spawns = [None; 5];
        pass_spawns[0] = Some(on_caster_seat(a));
        pass_spawns[1] = Some(on_caster_seat(b));
        return CaptureCamArm {
            shot: Some(on_caster(
                -0x20,
                third(seats.depth_raw),
                KISS_OF_DEATH_ARM0_SHOT_FRAMES,
            )),
            pass_spawns,
            // Every standing seat hidden (`sw zero,0x4` / `sb 0xFF,0x21c`
            // behind the HP test, `0x801F887C..0x801F88B0`), then the caster
            // shown again (`0x801F88B4..0x801F88C4`).
            others_write: CaptureSeatWrite {
                colour: Some(0),
                render_flag: Some(0xFF),
                ..Default::default()
            },
            others_live_only: true,
            caster_write: CaptureSeatWrite {
                colour: Some(0x2008_0200),
                render_flag: Some(0),
                ..Default::default()
            },
            ..Default::default()
        };
    }
    if phase == crate::cast_module_ticks::CHOREOGRAPHY_DONE_PHASE {
        // `0x801F8E8C..0x801F8ED4`: every combat slot `0..7` whose HP
        // `+0x14C` is non-zero shown again (`+0x04 = 0x20080200`,
        // `+0x21C = 0`), the caster among them.
        return CaptureCamArm {
            others_write: super::v_windhash::SHOWN,
            others_live_only: true,
            ..Default::default()
        };
    }
    if !(1..=5).contains(&phase) {
        return CaptureCamArm::default();
    }
    // Arms 1 and 2 seat the record on an odd count, ahead of their gate,
    // once a pass (the director runs every tick of it).
    let mut pass_spawns = [None; 5];
    if phase <= 2 && st.opens_pass {
        if st.pass_count & 1 == 1 {
            pass_spawns[0] = Some(on_caster_seat(KISS_OF_DEATH_PASS_RECORD));
        }
        st.pass_count = st.pass_count.wrapping_add(1);
    }
    if st.countdown.drain_above(0) {
        return CaptureCamArm {
            drift: push,
            hold: true,
            pass_spawns,
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
        pass_spawns,
        ..Default::default()
    };
    match phase {
        1 => {
            arm.shot = Some(on_caster(0x80, half(seats.depth_raw), 0xA0));
            // The caster's tint state `7`, the soft red (`li v0,0x7` /
            // `sb v0,0x21c(s2)`, `0x801F8A28..0x801F8A34`).
            arm.caster_write.render_flag = Some(7);
        }
        3 => {
            let v = seats.victim;
            let facing = (bearing_12bit_approx(c.z, c.x, v.z, v.x).wrapping_add(0x800)) & 0xFFF;
            // The cut onto the victim shows it and hides the caster
            // (`0x801F8C24..0x801F8C50`: `+0x04 = 0x20080200` and
            // `+0x21C = 0` on the victim, `+0x04 = 0` and `+0x21C = 0xFF` on
            // the caster): arm 0 hid every seat but the caster.
            arm.victim_write = CaptureSeatWrite {
                facing: Some(facing),
                ..super::v_windhash::SHOWN
            };
            arm.caster_write = super::v_windhash::HIDDEN;
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

    /// Arm 0 seats its two records on the caster and hides every standing
    /// seat but the caster; arms 1 and 2 seat the burst record on every other
    /// pass; arm 1's passing pass tints the caster soft red.
    #[test]
    fn the_arms_seat_their_records_and_tint_the_caster() {
        let mut st = ModuleCamState::default();
        let a = kiss_of_death_camera(&mut st, 0, seats());
        let recs: Vec<_> = a.pass_spawns.iter().flatten().map(|s| s.record).collect();
        assert_eq!(
            recs,
            KISS_OF_DEATH_ARM0_RECORDS.map(SpawnRecord::Module).to_vec()
        );
        assert_eq!(a.others_write.render_flag, Some(0xFF));
        assert!(a.others_live_only);
        assert_eq!(a.caster_write.render_flag, Some(0));
        // Seated on the passes the count reads odd, ticks between passes none.
        let mut seated = Vec::new();
        for pass in 0..4 {
            st.opens_pass = true;
            let a = kiss_of_death_camera(&mut st, 1, seats());
            seated.push(a.pass_spawns[0].is_some());
            st.opens_pass = false;
            let between = kiss_of_death_camera(&mut st, 1, seats());
            assert!(between.pass_spawns[0].is_none(), "pass {pass}");
        }
        assert_eq!(seated, [false, true, false, true]);
        st.countdown.0 = 0;
        st.opens_pass = false;
        let a = kiss_of_death_camera(&mut st, 1, seats());
        assert_eq!(a.caster_write.render_flag, Some(7));
    }

    /// Arm 3's cut shows the victim and hides the caster
    /// (`0x801F8C24..0x801F8C50`); the `0xFF` arm shows every living seat
    /// (`0x801F8E8C..0x801F8ED4`).
    #[test]
    fn the_cut_swaps_the_shown_seat_and_the_exit_shows_every_living_one() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = 0;
        let a = kiss_of_death_camera(&mut st, 3, seats());
        assert_eq!(a.victim_write.colour, Some(0x2008_0200));
        assert_eq!(a.victim_write.render_flag, Some(0));
        assert!(a.victim_write.facing.is_some());
        assert_eq!(a.caster_write.colour, Some(0));
        assert_eq!(a.caster_write.render_flag, Some(0xFF));
        let a = kiss_of_death_camera(&mut st, 0xFF, seats());
        assert_eq!(a.others_write.colour, Some(0x2008_0200));
        assert_eq!(a.others_write.render_flag, Some(0));
        assert!(a.others_live_only);
    }
}
