//! Cort's **Evil Seru Magic** (PROT 0966, action `0xAD`): the camera arms,
//! the countdown and the phase chain of the module's one tick body
//! `0x801F6A74`.
//!
//! The body (8944 bytes, frame `-0x60`) is the longest choreography in the
//! capture band: 29 arms behind `sltiu a1, 0x1D` through the image-head
//! table at `0x801F69D8`. Arms 5..9 are the table's default (`0x801F8A94`, the
//! shared tail) and are never entered - arm 4 writes `ctx[+0x279] = 10`
//! itself (`0x801F7010`) instead of stepping. Every other arm leaves through
//! `0x801F8994`, which steps the phase, or `0x801F8A94`, which holds it; the
//! body returns `s7`, `1` until arm 28's gate passes (`clear s7` at
//! `0x801F8A84`).
//!
//! Unlike the scalar-paced modules, the countdown `0x801FA464` is armed in
//! absolute vsyncs and drained by the frame delta `*(0x1F800393)` alone - no
//! speed scalar - so the engine, ticking once a vsync, drains it by `1`. Each
//! drift is likewise `k * delta` a battle frame, `k` a tick here.
//!
//! The camera globals the arms walk are pitch / yaw `0x8007B790` /
//! `0x8007B792` and TR x / y / z `0x800840B8` / `BC` / `C0`. Every shot is a
//! `FUN_801D829C` call whose focus trio is zero but for the z the arm stores
//! (the prologue zeroes all nine stack halfwords each pass).
//!
//! What the arms do to the seats (hide / show, the caster's stages, the
//! party-wide hit at arm 26) is [`crate::cast_module_ticks::evil_seru_magic_seat_writes`];
//! this module is the half that decides when an arm passes.
//!
//! Provenance: `ghidra/scripts/funcs/overlay_cast_evil_seru_magic_0966_801f6a74.txt`
//! (the image at slot-B base `0x801F69D8`).

use super::capture::{CaptureCamArm, CaptureDrift};
use super::{ModuleCamSeats, ModuleCamState, ModuleShot, focus_on, yaw_from};

/// PROT 0966's countdown word (`lui 0x8020` / `-0x5B9C`).
pub const EVIL_SERU_MAGIC_COUNTDOWN: u32 = 0x801F_A464;

/// The arm the party-wide hit lands on (`0x801F83C8`).
pub const EVIL_SERU_MAGIC_SWEEP_ARM: u8 = 26;

/// The arm on whose pass the module's move-VM stager sweep lands: arm 10
/// (`0x801F7160`) spawns record `0x801F937C`, whose script is `WAIT 0x7F`
/// then op `0x20` arm 4 - the stager `0x801F8D64`'s `0x100` party sweep.
/// The wait runs 127 vsyncs; arm 11's gate is `0x80`.
pub const EVIL_SERU_MAGIC_STAGER_HIT_ARM: u8 = 11;

/// The arm whose gate finishes the body (`0x801F89A8`).
pub const EVIL_SERU_MAGIC_LAST_ARM: u8 = 28;

/// The countdown drain per engine tick: `*(0x1F800393)` per battle frame is
/// one per vsync.
const DRAIN: i32 = 1;

fn d(pitch: i16, yaw: i16, tr_x: i16, tr_y: i16, tr_z: i16) -> Option<CaptureDrift> {
    Some(CaptureDrift {
        pitch,
        yaw,
        tr_y,
        tr_z,
        tr_x,
    })
}

/// A shot whose focus trio is `(0, 0, fz)` - every immediate-only shot of the
/// body.
fn fixed(angles: [i16; 2], tr: [i16; 3], fz: i16, frames: u16) -> Option<ModuleShot> {
    Some(ModuleShot {
        angles: [angles[0], angles[1], 0],
        tr,
        focus: [0, 0, fz],
        frames,
    })
}

/// Cort's **Evil Seru Magic**, PROT 0966's tick body `0x801F6A74`.
///
/// | arm | drift a tick | gate re-arm | on pass |
/// |---|---|---|---|
/// | 0 (`0x801F6B1C`) | - | `= 0x40` | shot pitch `-0x80`, yaw `0x790 - caster[+0x46]`, TR `(0, 0x800, 0x400)`, focus the caster, `0x30` frames |
/// | 1 (`0x801F6D5C`) | - | `+= 0xE0` | - |
/// | 2 (`0x801F6E00`) | pitch `+1`, yaw `+1`, TR y `-1`, TR z `+4` | `+= 0x40` | four records, a red flash |
/// | 3 (`0x801F6F38`) | - | `+= 0x40` | - |
/// | 4 (`0x801F6F64`) | TR y `+8` | `+= 0x40` | fade to white; phase `= 10` |
/// | 10 (`0x801F701C`) | TR y `+8` | `+= 0x80` | shot pitch `0xE0`, yaw `0x800`, TR `(0, 0x180, 0xC00)`, focus z `-0x190` |
/// | 11 (`0x801F7344`) | pitch `+1`, yaw `+4`, TR y `-4`, TR z `+12` | `+= 0x80` | - |
/// | 12 (`0x801F73E4`) | pitch `-2`, yaw `+4`, TR y `+4`, TR z `-6` | `+= 0x80` | - |
/// | 13, 14 (`0x801F75C8`, `0x801F7798`) | yaw `+4`, TR y `+2`, TR z `+6` | `+= 0x80` | - |
/// | 15 (`0x801F7964`) | - | `+= 0x80` | - |
/// | 16 (`0x801F7ABC`) | TR y `+4`, TR z `-8` | `+= 0x40` | - |
/// | 17 (`0x801F7B18`) | - | `+= 0x40` | shot yaw `0xD80`, TR `(0, 0xA00, 0x1000)`, focus z `-0x800` |
/// | 18 (`0x801F7B7C`) | - | `+= 0x80` | - |
/// | 19 (`0x801F7C68`) | yaw `+1`, TR x `-16`, TR z `+32` | `+= 0x70` | shot pitch `-0x100`, yaw `0x100`, TR `(0x80, 0x1800, 0x1400)`, focus z `-0x800` |
/// | 20 (`0x801F7D4C`) | pitch `+2`, TR x `+8`, TR y `-40`, TR z `+40` | `+= 0xB4` | the spell-name banner |
/// | 21 (`0x801F7E38`) | TR z `-4` | `+= 0xC0` | shot pitch `-0x50`, TR `(0, 0x300, 0x1C00)`, focus z `-0x400` |
/// | 22 (`0x801F7F60`) | - | `+= 0x100` | every pass a `0x24`-frame shot pitch `-0x50`, TR `(-0x14, 0x1228, 0)`; on pass a cut pitch `0x40`, yaw `0x80`, TR `(0, 0, 0x1A00)`; both focus z `-0x400` |
/// | 23 (`0x801F8008`) | yaw `-1`, TR z `+4` | `+= 0x180` | shot pitch `-0x80`, yaw `-0x80`, TR `(-0x600, 0x800, 0x1400)`, focus z `-0x600` |
/// | 24 (`0x801F82C8`) | yaw `+2`, TR x `+14`, TR y `+1`, TR z `+4` | `+= 0x80` | - |
/// | 25 (`0x801F8364`) | - | `+= 0x80` | shot pitch `-0x80`, TR `(0, 0x900, 0x1600)`, focus z `-0x400` |
/// | 26 (`0x801F83C8`) | TR z `+4` | `+= 0x100` | the party-wide hit |
/// | 27 (`0x801F86E4`) | TR z `+4` | `+= 0x80` | the seats restored; shot yaw `0x800 - caster[+0x46]`, TR `(0, 0x800, 0x400)`, focus the caster |
/// | 28 (`0x801F89A8`) | TR z `+2` | - | the body finishes |
///
/// Every gate drains first and holds while the word stays positive; the
/// drift above is applied on every pass of its arm, held or not. Every shot
/// but arm 0's and arm 22's held one is a one-frame cut. The body has no
/// other port, so the director owns the phase and the band leaves `0x70`
/// once arm 28 passes.
///
/// Arm 0 (and arm 27's restore) also re-seats the party in a row of half-size
/// models at `z = 0x190` and arm 10 parks the creature seat 7 at
/// `(0, _, 0x1C0)`; the engine keeps every seat where the battle put it, so
/// the cuts frame the stage the retail seats occupy rather than the seats.
///
/// PORT: FUN_801F6A74, overlay_cast_evil_seru_magic_0966_801f6a74 (PROT 0966; the camera arms 0..28, their countdown and the phase chain)
pub fn evil_seru_magic_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    // Arms 5..9 are the table's default, never entered (arm 4 jumps to 10);
    // carry a phase that lands there on.
    if (5..=9).contains(&phase) {
        return CaptureCamArm {
            next: Some(10),
            ..Default::default()
        };
    }
    if phase == 0 {
        st.countdown.0 = 0x40;
        return CaptureCamArm {
            shot: Some(ModuleShot {
                angles: [-0x80, yaw_from(0x790, c.facing), 0],
                tr: [0, 0x800, 0x400],
                focus: focus_on(c),
                frames: 0x30,
            }),
            next: Some(1),
            ..Default::default()
        };
    }
    if phase > EVIL_SERU_MAGIC_LAST_ARM {
        // Retail's `sltiu a1, 0x1D` sends these to the tail and returns busy
        // for good; nothing on the disc gets here, and holding would park
        // the band.
        return CaptureCamArm::default();
    }
    let drift = match phase {
        2 => d(1, 1, 0, -1, 4),
        4 | 10 => d(0, 0, 0, 8, 0),
        11 => d(1, 4, 0, -4, 12),
        12 => d(-2, 4, 0, 4, -6),
        13 | 14 => d(0, 4, 0, 2, 6),
        16 => d(0, 0, 0, 4, -8),
        19 => d(0, 1, -16, 0, 32),
        20 => d(2, 0, 8, -40, 40),
        21 => d(0, 0, 0, 0, -4),
        23 => d(0, -1, 0, 0, 4),
        24 => d(0, 2, 14, 1, 4),
        26 | 27 => d(0, 0, 0, 0, 4),
        28 => d(0, 0, 0, 0, 2),
        _ => None,
    };
    // Arm 22 re-arms its `0x24`-frame shot every pass, before its gate.
    let held_shot = (phase == 22).then(|| fixed([-0x50, 0], [-0x14, 0x1228, 0], -0x400, 0x24));
    st.countdown.0 -= DRAIN;
    if st.countdown.0 > 0 {
        return CaptureCamArm {
            shot: held_shot.flatten(),
            drift,
            hold: true,
            next: Some(phase),
            ..Default::default()
        };
    }
    let (rearm, shot, next): (i32, Option<ModuleShot>, Option<u8>) = match phase {
        1 => (0xE0, None, Some(2)),
        2 => (0x40, None, Some(3)),
        3 => (0x40, None, Some(4)),
        4 => (0x40, None, Some(10)),
        10 => (
            0x80,
            fixed([0xE0, 0x800], [0, 0x180, 0xC00], -0x190, 1),
            Some(11),
        ),
        11..=15 => (0x80, None, Some(phase + 1)),
        16 => (0x40, None, Some(17)),
        17 => (
            0x40,
            fixed([0, 0xD80], [0, 0xA00, 0x1000], -0x800, 1),
            Some(18),
        ),
        18 => (0x80, None, Some(19)),
        19 => (
            0x70,
            fixed([-0x100, 0x100], [0x80, 0x1800, 0x1400], -0x800, 1),
            Some(20),
        ),
        20 => (0xB4, None, Some(21)),
        21 => (
            0xC0,
            fixed([-0x50, 0], [0, 0x300, 0x1C00], -0x400, 1),
            Some(22),
        ),
        22 => (
            0x100,
            fixed([0x40, 0x80], [0, 0, 0x1A00], -0x400, 1),
            Some(23),
        ),
        23 => (
            0x180,
            fixed([-0x80, -0x80], [-0x600, 0x800, 0x1400], -0x600, 1),
            Some(24),
        ),
        24 => (0x80, None, Some(25)),
        25 => (
            0x80,
            fixed([-0x80, 0], [0, 0x900, 0x1600], -0x400, 1),
            Some(26),
        ),
        26 => (0x100, None, Some(27)),
        27 => (
            0x80,
            Some(ModuleShot {
                angles: [0, yaw_from(0x800, c.facing), 0],
                tr: [0, 0x800, 0x400],
                focus: focus_on(c),
                frames: 1,
            }),
            Some(28),
        ),
        // Arm 28: `clear s7`, `ctx[+0x0D] = 0`, `ctx[+0x6DA] = 0x780`.
        _ => {
            st.yaw_base = 0x780;
            (0, None, None)
        }
    };
    st.countdown.0 += rearm;
    CaptureCamArm {
        shot,
        drift,
        hold: false,
        next,
        ..Default::default()
    }
}

#[cfg(test)]
mod esm_camera_tests {
    use super::super::ModuleSeat;
    use super::*;

    /// Drive the director the way the band does: a held pass keeps the
    /// phase, a passing one moves to `next`, `None` finishes.
    fn run(seats: ModuleCamSeats) -> (Vec<u8>, u32, i32) {
        let mut st = ModuleCamState::default();
        let mut phase = 0u8;
        let mut visited = vec![];
        let mut ticks = 0u32;
        let mut tr_x = 0i32;
        loop {
            ticks += 1;
            assert!(ticks < 10_000, "the chain must finish");
            let arm = evil_seru_magic_camera(&mut st, phase, seats);
            if let Some(d) = arm.drift {
                tr_x += i32::from(d.tr_x);
            }
            if arm.hold {
                continue;
            }
            visited.push(phase);
            match arm.next {
                Some(n) => phase = n,
                None => break,
            }
        }
        (visited, ticks, tr_x)
    }

    #[test]
    fn esm_chain_walks_every_arm_once_and_skips_5_to_9() {
        let (visited, ticks, _) = run(ModuleCamSeats::default());
        let mut want: Vec<u8> = (0..=4).collect();
        want.extend(10..=28);
        assert_eq!(visited, want);
        // Arm 0 runs ungated; after it, one drain a tick for every vsync an
        // arm armed.
        let armed: u32 = [
            0x40, 0xE0, 0x40, 0x40, 0x40, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x40, 0x40, 0x80,
            0x70, 0xB4, 0xC0, 0x100, 0x180, 0x80, 0x80, 0x100, 0x80,
        ]
        .iter()
        .sum();
        assert_eq!(ticks, armed + 1);
    }

    #[test]
    fn esm_tr_x_walks_out_and_back() {
        // Arm 19 walks TR x left for arm 18's 0x80 vsyncs, arm 20 right for
        // arm 19's 0x70 and arm 24 right for arm 23's 0x180.
        let (_, _, tr_x) = run(ModuleCamSeats::default());
        assert_eq!(tr_x, -16 * 0x80 + 8 * 0x70 + 14 * 0x180);
    }

    #[test]
    fn esm_arm_0_frames_the_caster_from_behind() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats {
            caster: ModuleSeat {
                x: 56,
                y: 0,
                z: 768,
                facing: 0x800,
            },
            ..Default::default()
        };
        let arm = evil_seru_magic_camera(&mut st, 0, seats);
        let shot = arm.shot.unwrap();
        assert_eq!(shot.angles, [-0x80, 0xF90, 0]);
        assert_eq!(shot.tr, [0, 0x800, 0x400]);
        assert_eq!(shot.focus, [-56, 0, -768]);
        assert_eq!(shot.frames, 0x30);
        assert_eq!(st.countdown.0, 0x40);
    }
}
