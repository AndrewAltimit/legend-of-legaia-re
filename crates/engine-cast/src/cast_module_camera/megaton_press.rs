//! PROT 0959 (**Megaton Press**, `0x7A`, Che Delilas): the tick body
//! `FUN_801F69F0`'s camera arms, the seat stores the phase chain
//! ([`crate::cast_module_ticks::MEGATON_PRESS_CHAIN`]) does not carry, and the
//! per-arm part spawns.
//!
//! Every arm from 1 on opens on the same drain of the module countdown
//! `0x801F9624` (`scalar * delta` a pass, held while it stays positive) and
//! re-arms it as it passes; the re-arms are the
//! [`super::capture_countdown`] table's. Two more module words carry state
//! between arms: `0x801F9628` (the arm-9 shake amplitude, then the arm-B
//! jump clock, then arm `0x0F`'s hit threshold) and `0x801F962C` (the shake's
//! sign toggle), kept in [`ModuleCamState::module_words`].
//!
//! See `ghidra/scripts/funcs/overlay_cast_megaton_press_0959_801f69f0.txt`.

use super::capture::{CaptureCamArm, CaptureDrift, CapturePoseSet, CaptureSeatWrite};
use super::*;

/// PROT 0959's `0x7A` tick body.
pub const MEGATON_PRESS_BODY: u32 = 0x801F_69F0;
/// PROT 0959's countdown word (`lui 0x8020` / `-0x69DC`).
pub const MEGATON_PRESS_COUNTDOWN: u32 = 0x801F_9624;

/// The shake amplitude arm 8 seeds into `0x801F9628` (`li v0,0x100` at
/// `0x801F742C`).
const SHAKE_SEED: i32 = 0x100;

/// The arm-B jump arc: `-(sin(12 * w) * 1920 / 4017)`, the caster's height
/// `+0x36` off the jump clock `w` (`0x801F7750..0x801F779C`: the sine table
/// `*0x8007B81C` read at `w * 24` bytes, scaled `* 15 << 7` and divided by
/// the magic `0x82846DDF` multiply-high, `>> 11`).
pub fn megaton_press_jump_height(w: i32) -> i16 {
    let s = i32::from(legaia_engine_system::trig::sin12((w * 12) as u16));
    let v = s * 1920;
    // `mult v1, 0x82846DDF; mfhi; addu v1; sra 11; - (v1 >> 31)`.
    let hi = ((i64::from(v) * i64::from(0x8284_6DDFu32 as i32)) >> 32) as i32;
    let q = ((hi + v) >> 11) - (v >> 31);
    (-q) as i16
}

/// The camera and seat half of **Megaton Press** (PROT 0959, body
/// `0x801F69F0`, a `beq` / `slti` chain over phases `0..=0x11` and `0xFF`),
/// by the module phase `ctx[+0x279]` the body is about to run. With `s` the
/// speed scalar and `d` the frame step (one engine tick):
///
/// | arm | every pass | as it passes |
/// |---|---|---|
/// | 0 (`0x801F6B80`) | - | caster to `(0, 0x140)` facing `0x800`, victim to `(0, -0x140)` facing `0`; yaw `0`, TR z `0x1000`, focus `(0, 0)`; shot pitch `0`, yaw `0x800 - caster`, TR `(0, 0x300, 3/4 depth)`, focus the caster, `0xC` frames |
/// | 1 (`0x801F6C78`) | TR z `+s/2` | shot pitch `0x80`, TR `(0, 0x600, 3/4 depth)`, focus the caster's body, `0x40` frames |
/// | 2 (`0x801F6D60`) | TR z `+s/2` | - |
/// | 3 (`0x801F6DD4`) | TR z `+s/2`, yaw `+4s` | yaw `0x800 - caster` |
/// | 4 (`0x801F6E70`) | TR z `+s/2` | pool flush; cut pitch `-0x80`, yaw `0x8C0 - caster`, TR `(0, 0x600, 0x600)`; victim hidden (`+0x04 = 0`, `+0x21C = 0xFF`) |
/// | 5 (`0x801F6F68`) | TR z `+s/2` | cut pitch `0xC0`, yaw `0x700 - caster`, TR `(0, 0x500, 0x800)` |
/// | 6 (`0x801F703C`) | TR z `+s/2` | cut pitch `0x80`, yaw `0x800 - caster`, TR `(0, 0x700, 0x400)`; victim shown (`+0x04 = 0x20080200`) |
/// | 7, 8 | TR z `+16s`, TR y `-4s`, pitch `+s/8` | arm 8 seeds the shake word `0x100` |
/// | 9 (`0x801F7440`) | the shake: amplitude `-d`, TR y `+-` amplitude on alternate passes | TR y `-0x100` |
/// | `0x0A` (`0x801F753C`) | TR z `+s`, TR y `+4s`, pitch `-s/2`; victim height `-3s/2` | caster z `-0x2C0`; cut pitch `0x80`, yaw `-victim`, TR `(0, 0x2400, 0x400)` on the victim's body; victim height `-0x780`, its clip cursor `0` |
/// | `0x0B` (`0x801F7698`) | pitch `-s/8`, TR z `+s`, TR y `-s`; the caster's jump arc ([`megaton_press_jump_height`]) | caster height `-0x780` |
/// | `0x0D` (`0x801F78D0`) | TR z `+28s`, TR y `-4s`, focus z `+2s`, pitch `+s`, yaw `-s`; caster z `-s/4` | victim `+0x21C = 7` |
/// | `0x0E` (`0x801F7B04`) | victim height `+60d`, TR y `-208d`, TR z `+4s`, focus z `-s`, yaw `-s` | victim clip cursor `0`, height `0`; the threshold word `+ (s << 8)` |
/// | `0x0F` (`0x801F7E28`) | TR z `+4s`, yaw `-s` | victim shown, caster height `0`; cut pitch `0x100`, yaw `0xC0 - victim`, TR `(0, 0x100, 0x400)` on the victim's body |
/// | `0x10` (`0x801F804C`) | TR y `+2s`, TR x `-s/2`, TR z `-s` (net `0` when the lead party member is roster `1`) | - |
/// | `0x11` (`0x801F8118`) | - | caster z `0x200`, victim z `-0x200` |
///
/// The anim-rate stores `+0x21D` ride the same seat writes: caster `6` through
/// arms 5 and 6 and `8` from 6's pass, victim `2` from 7's pass, caster `0` /
/// victim `2` through arm 9, caster `2` / victim `0` through `0x0A`, both
/// frozen on `0x0B` / `0x0D`'s passes, caster `0x0C` / victim `1` on `0x0C`'s
/// and both back at `8` on `0x11`'s.
///
/// Arm 9's shake is stepped once a battle frame ([`ModuleCamState::opens_pass`]):
/// its sign toggles per pass, so run per tick under the frame cadence it
/// would cancel inside each pass.
///
/// PORT: FUN_801F69F0 (PROT 0959; the camera arms, the countdown gates and
/// the position / height / visibility stores)
pub fn megaton_press_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let s = SPEED_SCALAR as i16;
    let c = seats.caster;
    let cb = st.caster_body.unwrap_or(c);
    let vb = st.victim_body.unwrap_or(seats.victim);
    let v = seats.victim;
    // `3 * depth / 4`, truncated toward zero (`bgez; addiu 3; sra 2`).
    let depth34 = (seats.depth_raw * 3 / 4) as i16;
    let shot = |pitch: i16, yaw: i16, tr: [i16; 3], on: ModuleSeat, frames: u16| ModuleShot {
        angles: [pitch, yaw, 0],
        tr,
        focus: focus_on(on),
        frames,
    };
    let rearm = |phase: u8| match phase {
        1 | 3 | 4 | 5 | 0x0B | 0x10 => Some(1 << 6),
        2 | 9 | 0x0F => Some(1 << 7),
        6 => Some(0x3C),
        7 => Some(1 << 2),
        8 => Some(0xE0),
        0x0A => Some(0x60),
        0x0C => Some(1 << 4),
        0x0D => Some(1 << 5),
        0x0E => Some(1 << 8),
        _ => None,
    };
    // The host's default order (shot, then drift) measures `.004` better
    // over the cast than the body's own drift-first order; arms 0, 3 and 9
    // take the drift-first path through their stores anyway.
    let mut arm = CaptureCamArm::default();
    if phase == 0 {
        st.countdown.0 = SPEED_SCALAR * 12;
        let caster_at = ModuleSeat {
            x: 0,
            z: 0x140,
            ..c
        };
        arm.pose_set = Some(CapturePoseSet {
            yaw: Some(0),
            tr_z: Some(0x1000),
            focus_xz: Some([0, 0]),
            ..Default::default()
        });
        arm.caster_write = CaptureSeatWrite {
            x: Some(0),
            z: Some(0x140),
            facing: Some(0x800),
            ..Default::default()
        };
        arm.victim_write = CaptureSeatWrite {
            x: Some(0),
            z: Some(-0x140),
            facing: Some(0),
            ..Default::default()
        };
        arm.shot = Some(shot(
            0,
            yaw_from(0x800, 0x800),
            [0, 0x300, depth34],
            caster_at,
            0xC,
        ));
        return arm;
    }
    if !(1..=0x11).contains(&phase) {
        return arm;
    }
    let [mut w, mut toggle] = st.module_words;
    // The writes each arm makes on every pass, ahead of its gate.
    let drift = |pitch: i16, yaw: i16, tr_y: i16, tr_z: i16| CaptureDrift {
        pitch,
        yaw,
        tr_y,
        tr_z,
        tr_x: 0,
    };
    match phase {
        1..=6 => {
            let yaw = if phase == 3 { 4 * s } else { 0 };
            arm.drift = Some(drift(0, yaw, 0, s >> 1));
        }
        7 | 8 => arm.drift = Some(drift(s / 8, 0, -4 * s, 16 * s)),
        9 => {
            if st.opens_pass && w >= 0 {
                w -= i32::from(st.frame_step.max(1));
                if w > 0 {
                    let up = toggle & 1 != 0;
                    toggle += 1;
                    let amp = w as i16;
                    arm.drift = Some(drift(0, 0, if up { amp } else { -amp }, 0));
                }
            }
        }
        0x0A => {
            arm.drift = Some(drift(-(s >> 1), 0, 4 * s, s));
            arm.victim_write.dy = -((3 * s) >> 1);
        }
        0x0B => {
            arm.drift = Some(drift(-(s / 8), 0, -s, s));
            w += 1;
            arm.caster_write.y = Some(megaton_press_jump_height(w));
        }
        0x0D => {
            arm.drift = Some(drift(s, -s, -4 * s, 28 * s));
            arm.focus_z = 2 * s;
            arm.caster_write.dz = -(s / 4);
        }
        0x0E => {
            arm.victim_write.dy = 60;
            arm.drift = Some(drift(0, -s, -208, 4 * s));
            arm.focus_z = -s;
        }
        0x0F => arm.drift = Some(drift(0, -s, 0, 4 * s)),
        0x10 => {
            let tr_z = if st.lead_roster == 1 { 0 } else { -s };
            arm.drift = Some(CaptureDrift {
                tr_x: -(s >> 1),
                ..drift(0, 0, 2 * s, tr_z)
            });
        }
        _ => {}
    }
    let hold = if phase == 0x0F {
        // `0x801F7E9C..0x801F7F44`: the drain, then a hit for every pass the
        // threshold word stands at or above the countdown (the phase chain
        // lands the cast's outcome as one), then the hold.
        st.countdown.drain();
        if w >= st.countdown.0 {
            w -= SPEED_SCALAR << 5;
            arm.hit_site = Some(2);
        }
        st.countdown.0 > 0
    } else {
        st.countdown.drain_above(0)
    };
    arm.hold = hold;
    if !hold {
        if let Some(n) = rearm(phase) {
            st.countdown.add(n);
        }
        match phase {
            1 => {
                arm.shot = Some(shot(
                    0x80,
                    yaw_from(0x800, c.facing),
                    [0, 0x600, depth34],
                    cb,
                    0x40,
                ))
            }
            3 => {
                arm.pose_set = Some(CapturePoseSet {
                    yaw: Some(yaw_from(0x800, c.facing)),
                    ..Default::default()
                })
            }
            4 => {
                arm.shot = Some(shot(
                    -0x80,
                    yaw_from(0x8C0, c.facing),
                    [0, 0x600, 0x600],
                    cb,
                    1,
                ));
                arm.victim_write.colour = Some(0);
                arm.victim_write.render_flag = Some(0xFF);
            }
            5 => {
                arm.shot = Some(shot(
                    0xC0,
                    yaw_from(0x700, c.facing),
                    [0, 0x500, 0x800],
                    cb,
                    1,
                ))
            }
            6 => {
                arm.shot = Some(shot(
                    0x80,
                    yaw_from(0x800, c.facing),
                    [0, 0x700, 0x400],
                    cb,
                    1,
                ));
                arm.victim_write.colour = Some(0x2008_0200);
                arm.victim_write.render_flag = Some(0);
            }
            // `FUN_801DD6B4(0x80)` at `0x801F71E8`, as the arm passes.
            7 => arm.hit_site = Some(0),
            8 => {
                w = SHAKE_SEED;
                toggle = 0;
            }
            9 => {
                arm.pose_set = Some(CapturePoseSet {
                    tr_y: Some(-0x100),
                    ..Default::default()
                })
            }
            0x0A => {
                w = 0;
                arm.caster_write.z = Some(-0x2C0);
                arm.shot = Some(shot(0x80, yaw_from(0, v.facing), [0, 0x2400, 0x400], vb, 1));
                arm.victim_write.y = Some(-0x780);
                arm.victim_write.cursor_reset = true;
            }
            0x0B => arm.caster_write.y = Some(-0x780),
            // `FUN_801DD6B4(0x80)` at `0x801F7A90`.
            0x0D => {
                arm.victim_write.render_flag = Some(7);
                arm.hit_site = Some(1);
            }
            0x0E => {
                arm.victim_write.y = Some(0);
                arm.victim_write.cursor_reset = true;
                w += SPEED_SCALAR << 8;
            }
            0x0F => {
                arm.victim_write.render_flag = Some(0);
                arm.caster_write.y = Some(0);
                arm.shot = Some(shot(
                    0x100,
                    yaw_from(0xC0, v.facing),
                    [0, 0x100, 0x400],
                    vb,
                    1,
                ));
            }
            0x11 => {
                arm.caster_write.z = Some(0x200);
                arm.victim_write.z = Some(-0x200);
            }
            _ => {}
        }
    }
    // The anim-rate stores `+0x21D`. Those an arm makes on every pass
    // stand on a held pass; on the pass that leaves, the arm's later stores
    // land over them, so the pass carries the arm's final value.
    let (cr, vr): (Option<u8>, Option<u8>) = match (phase, hold) {
        (5, _) => (Some(6), None),
        (6, true) => (Some(6), None),
        (6, false) => (Some(8), None),
        (7, true) => (Some(8), None),
        (7, false) => (Some(8), Some(2)),
        (8, _) => (None, Some(2)),
        (9, _) => (Some(0), Some(2)),
        (0x0A, _) => (Some(2), Some(0)),
        (0x0B, true) => (None, Some(2)),
        (0x0B | 0x0D, false) => (Some(0), Some(0)),
        // `sb a0,0x21d(s2)` with `a0` still the phase byte, and the busy
        // word's low byte (`lbu t6,0x28(sp)`) into the victim's.
        (0x0C, false) => (Some(0x0C), Some(1)),
        (0x11, false) => (Some(8), Some(8)),
        _ => (None, None),
    };
    arm.caster_write.rate = cr;
    arm.victim_write.rate = vr;
    st.module_words = [w, toggle];
    arm
}

// The per-arm part spawns, every one `FUN_80050ED4(a0, a1, record, scale)`.
const fn spawn(record: u32, anchor: SpawnAnchor) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor,
    }
}
/// `a0 = caster + 0x34`, `a1 = caster + 0x44`: the caster's live trio,
/// height included (arm B seats its records mid-jump, before the arm's
/// height store).
const CASTER_LIVE: SpawnAnchor = SpawnAnchor::CasterShifted { dy: 0, dz: 0 };
/// `0x801F6C68`, arm 0.
pub(super) const E959_0: [ModuleSpawn; 1] = [spawn(0x801F_884C, CASTER_LIVE)];
/// `0x801F7348..0x801F7420`, arm 8's pass.
pub(super) const E959_8: [ModuleSpawn; 10] = [
    spawn(0x801F_8920, CASTER_LIVE),
    spawn(0x801F_8984, CASTER_LIVE),
    spawn(0x801F_8A34, CASTER_LIVE),
    spawn(0x801F_8A88, CASTER_LIVE),
    spawn(0x801F_8B78, CASTER_LIVE),
    spawn(0x801F_8BCC, CASTER_LIVE),
    spawn(0x801F_8C4C, CASTER_LIVE),
    spawn(0x801F_8C9C, CASTER_LIVE),
    spawn(0x801F_8CF8, CASTER_LIVE),
    spawn(0x801F_8D68, CASTER_LIVE),
];
/// `0x801F77E8..0x801F7848`, arm B's pass, after its pool flush.
pub(super) const E959_B: [ModuleSpawn; 5] = [
    spawn(0x801F_8F28, CASTER_LIVE),
    spawn(0x801F_8F80, CASTER_LIVE),
    spawn(0x801F_8FD8, CASTER_LIVE),
    spawn(0x801F_9030, CASTER_LIVE),
    spawn(0x801F_9088, CASTER_LIVE),
];
/// `0x801F7A08..0x801F7A68`, arm D's pass: `a0 = victim + 0x3C`, `a1 =
/// caster + 0x44`.
pub(super) const E959_D: [ModuleSpawn; 5] = [
    spawn(0x801F_90E0, SpawnAnchor::VictimBodyCasterRot),
    spawn(0x801F_9144, SpawnAnchor::VictimBodyCasterRot),
    spawn(0x801F_91A8, SpawnAnchor::VictimBodyCasterRot),
    spawn(0x801F_91E8, SpawnAnchor::VictimBodyCasterRot),
    spawn(0x801F_9228, SpawnAnchor::VictimBodyCasterRot),
];
const fn ground(record: u32, scale: u16) -> ModuleSpawn {
    spawn(record, SpawnAnchor::VictimBodyGround { scale })
}
/// `0x801F7D28..0x801F7E18`, arm E's pass, after its pool flush: the
/// victim's `+0x3C..+0x43` copied to the stack with the height zeroed, on
/// the victim's angle trio, at the literal scales.
pub(super) const E959_E: [ModuleSpawn; 11] = [
    ground(0x801F_927C, 0xC00),
    ground(0x801F_92C4, 0xC00),
    ground(0x801F_930C, 0xC00),
    ground(0x801F_9354, 0xC00),
    ground(0x801F_939C, 0x1000),
    ground(0x801F_93F0, 0x800),
    ground(0x801F_945C, 0x1000),
    ground(0x801F_94CC, 0x1000),
    ground(0x801F_9520, 0x1000),
    ground(0x801F_9574, 0x1000),
    ground(0x801F_95CC, 0x1000),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn seats() -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 143,
                y: 0,
                z: -376,
                facing: 1933,
            },
            victim: ModuleSeat {
                x: 240,
                y: 0,
                z: -1000,
                facing: 3980,
            },
            depth_raw: 0x1000,
            ..Default::default()
        }
    }

    #[test]
    fn arm_0_stands_the_pair_on_the_axis_and_cuts_to_the_caster() {
        let mut st = ModuleCamState::default();
        let a = megaton_press_camera(&mut st, 0, seats());
        assert_eq!(st.countdown.0, SPEED_SCALAR * 12);
        assert_eq!(a.caster_write.z, Some(0x140));
        assert_eq!(a.victim_write.z, Some(-0x140));
        let s = a.shot.unwrap();
        assert_eq!(s.tr, [0, 0x300, 0xC00]);
        assert_eq!(s.focus, [0, 0, -0x140]);
        assert_eq!(s.frames, 0xC);
        assert!(!a.hold);
    }

    #[test]
    fn the_gates_run_the_countdown_table() {
        // Each arm's dwell in ticks is its predecessor's re-arm over the
        // per-tick drain, as `capture_countdown::MEGATON_PRESS` measures it.
        let mut st = ModuleCamState::default();
        megaton_press_camera(&mut st, 0, seats());
        let mut ticks = Vec::new();
        for phase in 1..=0x11u8 {
            let mut n = 0;
            loop {
                n += 1;
                st.opens_pass = true;
                if !megaton_press_camera(&mut st, phase, seats()).hold {
                    break;
                }
            }
            ticks.push(n);
        }
        assert_eq!(
            ticks,
            [
                12, 64, 128, 64, 64, 64, 60, 4, 224, 128, 96, 64, 16, 32, 256, 128, 64
            ]
        );
    }

    #[test]
    fn the_jump_arc_rises_and_falls_back() {
        assert_eq!(megaton_press_jump_height(0), 0);
        let peak = megaton_press_jump_height(85);
        assert!(peak < -1900 && peak > -2000, "{peak}");
        assert!(megaton_press_jump_height(170).abs() < 40);
    }
}
