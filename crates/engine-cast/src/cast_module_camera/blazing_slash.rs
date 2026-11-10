//! PROT 0958 (**Blazing Slash**, `0x79`, Gi Delilas): the tick body
//! `FUN_801F6DD8`'s camera arms, countdown gates, seat stores and part
//! spawns - the half the phase port
//! ([`crate::cast_module_ticks::blazing_slash_tick`]) does not carry.
//!
//! The body is Megaton Press's and Plasma Strike's sibling
//! ([`super::megaton_press`], [`super::plasma_strike`]): the same arm-0
//! staging on the seat axis and the same countdown word shape
//! (`0x801F93B8`, drained `scalar * delta` a pass, re-armed by **adding** as
//! each arm passes). It runs 26 arms off the 256-slot table at the image
//! head and then `0xFF`. A second module word, `0x801F93BC`, paces arm 22's
//! repeated flame record; it is kept in [`ModuleCamState::module_words`].
//!
//! See `ghidra/scripts/funcs/overlay_cast_blazing_slash_0958_801f6dd8.txt`.
//!
//! REF: FUN_801DD6B4 (the damage wrapper), FUN_8004FCC8 (the cue dispatcher)
//! REF: FUN_80050ED4 (the pool spawn wrapper)

use super::capture::{CaptureCamArm, CaptureDrift, CapturePoseSet, CaptureSeatWrite};
use super::*;

/// PROT 0958's `0x79` tick body.
pub const BLAZING_SLASH_BODY: u32 = 0x801F_6DD8;
/// PROT 0958's countdown word (`lui 0x8020` / `-0x6C48`).
pub const BLAZING_SLASH_COUNTDOWN: u32 = 0x801F_93B8;

/// The cursor (12.4) arms 10, 13 and 16 hold the victim's reaction clip at
/// or past (`slti v0,v0,0x10` / `sh 0x10,0x68`, `0x801F78EC..0x801F7908`).
const VICTIM_CURSOR_FLOOR: u16 = 0x10;
/// What arm 21 seeds the pacing word `0x801F93BC` with, in scalars
/// (`((4s + s) << 2) + s) << 2` at `0x801F87D8..0x801F87F8`).
const FLAME_SEED: i32 = 84;
/// What each of arm 22's paced calls takes off the pacing word, in scalars
/// (`0x801F88B4..0x801F88D0`).
const FLAME_STEP: i32 = 12;
/// The arm that stores `0x78` into the readout flag `0x8007B64C` as it
/// passes (`0x801F8ABC..0x801F8ACC`): every hit before it clears the flag
/// (`sb zero,-0x49b4`), so the damage cluster opens here.
pub const BLAZING_SLASH_READOUT_ARM: u8 = 23;
/// How many ticks arm 24 waits on the victim's reaction clip before the
/// engine lets it through. Retail's arm has no bound (the clip's own end
/// is one); this one keeps a clip the host never ends from parking the band.
const ARM_24_WAIT_BOUND: u16 = 0x400;

/// Arm 21's hop: the caster's height `+0x36` off the countdown word,
/// `-(sin(8 * w) / 8)` toward zero (`0x801F8614..0x801F8640`: the sine table
/// `*0x8007B81C` read at `w << 4` bytes, `bgez; addiu 7; sra 3`, negated).
/// The word runs `32 s` down to zero through the arm, so the angle sweeps a
/// half turn back to none.
pub fn blazing_slash_hop_height(word: i32) -> i16 {
    let s = i32::from(legaia_engine_system::trig::sin12((word * 8) as u16));
    (-(s / 8)) as i16
}

/// The camera, countdown and seat half of **Blazing Slash** (PROT 0958, body
/// `0x801F6DD8`, a `jr` through the 256-word table at the image head: arms
/// `0..=25` and `0xFF`), by the module phase `ctx[+0x279]` the body is about
/// to run. With `s` the speed scalar (one engine tick a frame step of one),
/// `C` / `V` the caster and victim and `depth` the framing depth
/// `ctx[+0x6D0]`:
///
/// | arm | every pass | as it passes |
/// |---|---|---|
/// | 0 (`0x801F6E9C`) | - | `C` to `(0, 0x80)` facing `0x800`, staged `0x0A` at rate `1`; `V` to `(0, -0x80)` facing `0`; yaw `0`, TR z `0x1000`, focus `(0, 0)`; shot pitch `0`, yaw `0x800 - C`, TR `(0, 0x300, depth / 2)` on `C`, `0xC` frames; word `= 12 s` |
/// | 1 (`0x801F6FAC`) | TR z `+s/4` | shot pitch `0x80`, TR `(0, 0x600, depth / 2)` on `C`'s body, `0x40` frames; `+ (s << 6)` |
/// | 2 (`0x801F7098`) | TR z `+s/4` | `+ (s << 7)` |
/// | 3 (`0x801F7114`) | TR z `+s/4`, yaw `+4s` | yaw `0x800 - C`; `+ (s << 6)` |
/// | 4 (`0x801F71C8`) | TR z `+s/4` | pool flush; cut pitch `-0x80`, yaw `0x8C0 - C`, TR `(0, 0x700, depth / 8)` on `C`'s body; `C` staged `0x0B`; stage byte `3`; `+ (s << 5)` |
/// | 5 (`0x801F72D8`) | TR z `+s/8` | cut pitch `0x100`, yaw `0xC0 - C`, TR `(0, 0x280, depth)` on `C`'s body; both `+0x21C = 7`; `+ (s << 6)` |
/// | 6 (`0x801F73D0`) | TR z `-4s`, pitch `-s/2` | pool flush; cut pitch `-0x40`, yaw `0x6C0 - V`, TR `(0, 0x300, 0x600)` on `V`; `V` at rate `2` into its knockdown; `C`'s loop released, `+0x21C = 0`; `+ (s << 5)` |
/// | 7 (`0x801F7568`) | - | `C` z `0x180`, staged `0x0C`; cut pitch `0`, yaw `0x800 - C`, TR `(0, 0x600, 0x400)` on `C`; `+ 12 s` |
/// | 8 (`0x801F7650`) | - | `C`'s body radius `0`; `+ 12 s` |
/// | 9, 10 (`0x801F76AC`, `0x801F78E0`) | focus z `-7s`, `C` z `-8s`, yaw `-2s`, pitch `+s/8`, TR z `+8s`, TR y `-3s` | 9: `V` into its knockdown, `+ 12 s`; 10: `+ (s << 4)` |
/// | 11 (`0x801F7A44`) | - | cut pitch `0x40`, yaw `-0x100 - V`, TR `(0, 0x300, 0x600)` on `V`'s body; `C` to z `-0x580` facing `0`, its cursor `0`; `+ 12 s` |
/// | 12, 13 (`0x801F7B0C`, `0x801F7D30`) | focus z `+s/2`, `C` z `+7s`, yaw `+4s`, TR z `+3s`, TR y `+s` | 12: `V` facing `0x800`, z `-0x300`, into its knockdown, `+ 12 s`; 13: `+ (s << 4)` |
/// | 14 (`0x801F7E74`) | - | cut pitch `-0x40`, yaw `0x700 - V`, TR `(0, 0x300, 0x600)` on `V`'s body; `C` to z `0x140` facing `0x800`, its cursor `0`; `+ 12 s` |
/// | 15, 16 (`0x801F7F40`, `0x801F8170`) | focus z `-2s`, `C` z `-7s`, yaw `+3s`, TR z `+s/8`, TR y `+s` | 15: `V` facing `0`, z `+0x300`, into its knockdown, `+ 12 s`; 16: `+ 56 s` |
/// | 17 (`0x801F82CC`) | - | `+ (s << 3)` |
/// | 18 (`0x801F832C`) | - | `V` z `-0x100`; `C` to `(-8, -0x400)`, staged `0x0A`; cut pitch `0`, yaw `0x800 - C`, TR `(0, 0x600, 0x300)` on `C`; `+ (s << 6)` |
/// | 19 (`0x801F840C`) | TR z `+s/8`, TR y `+3s/4`, pitch `+s/8` | `+ (s << 6)` |
/// | 20 (`0x801F84F4`) | - | `C` staged `0x0B`; `+ (s << 5)` |
/// | 21 (`0x801F8558`) | pitch `+s/4`, TR y `-4s`, focus z `+s/8`; `C` z `+2s` on its hop ([`blazing_slash_hop_height`]) | `C` to z `-0x200`, height `0`; cue `0x56`; the pacing word `= 84 s`; `+ 96 s` |
/// | 22 (`0x801F8804`) | pitch `+s/4`, TR z `+4s`; the flame record on `C` each time the word falls to the pacing word, which then drops `12 s` | pool flush; `C` to `(0, 0x1C0)`, staged `0x0D`; `V` to z `-0x1C0` facing `0x800`, its cursor `0`; cut pitch `-0x80`, yaw `0x800 - C`, TR `(0, 0x500, 0x600)` on `C`; `+ (s << 7)` |
/// | 23 (`0x801F8A50`) | TR z `+s`; `V`'s cursor `0` | `C` `+0x21C = 0` |
/// | 24 (`0x801F8AD0`) | TR z `+s`; held while `V` plays its knockdown (`+0x1D9 == +0x1F1`) | `+ (s << 5)` |
/// | 25 (`0x801F8BD4`) | - | both staged `0`; `V` facing `0`, shown (`+0x04 = 0x20080200`); cut pitch `0x80`, yaw `0x800 - C`, TR `(0, 0x500, 0x800)` on `C` |
/// | `0xFF` (`0x801F8CBC`) | - | `C` staged `0`, its body radius back on its size class; `V` `+0x21C = 0` |
///
/// Every arm from 1 on but 24 opens on the drain of the word and holds while
/// it stays positive; the drifts and the every-pass stores sit ahead of that
/// gate. The anim-rate stores `+0x21D` ride the same passes: `V` at `2`
/// through arms 6..=17 but 8 (frozen, `0`), `C` frozen through 11, 13, 14,
/// 16..=18 and back at `8` as 11 and 14 pass, both frozen through 18..=20
/// (`V`) and 24, 25, `C` at `8` through 21 and `3` through 23, both `8` as 25
/// passes.
///
/// The six `FUN_801DD6B4` sites are named as their passes land them
/// ([`CaptureCamArm::hit_site`], in [`crate::cast_module_ticks::BLAZING_SLASH_POWERS`]'
/// order): arms 6, 9, 12, 15 and 21 as they pass, and arm 22's on each paced
/// call. Each is the wrapper's roll, the popup `FUN_801F44A0`, the unsigned
/// clamp to the live HP of `0x801C9370[0]` and the readout flag
/// `0x8007B64C` cleared; arm 23 stores `0x78` there as it passes
/// ([`BLAZING_SLASH_READOUT_ARM`]). Arm 7 clears `C`'s committed-clip ring
/// `+0x1FB..+0x21A` ([`CaptureSeatWrite::ghost_ring_clear`]).
///
/// Not carried here: arm 24's dead-victim branch (which forces battle state
/// 5); arm 25's `ctx[+0x6D0] = 0x800`.
///
/// PORT: FUN_801F6DD8 (PROT 0958; the camera arms, the countdown and clip
/// gates and the seat stores)
pub fn blazing_slash_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let s = SPEED_SCALAR as i16;
    let c = seats.caster;
    let v = seats.victim;
    let cb = st.caster_body.unwrap_or(c);
    let vb = st.victim_body.unwrap_or(v);
    let depth = seats.depth_raw;
    let shot = |pitch: i16, yaw: i16, tr: [i16; 3], on: ModuleSeat, frames: u16| ModuleShot {
        angles: [pitch, yaw, 0],
        tr,
        focus: focus_on(on),
        frames,
    };
    let drift = |pitch: i16, yaw: i16, tr_y: i16, tr_z: i16| CaptureDrift {
        pitch,
        yaw,
        tr_y,
        tr_z,
        tr_x: 0,
    };
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    match phase {
        0 => {
            st.countdown.0 = SPEED_SCALAR * 12;
            st.module_words = [0; 2];
            st.wait = (0, 0);
            let caster_at = ModuleSeat { x: 0, z: 0x80, ..c };
            arm.pose_set = Some(CapturePoseSet {
                yaw: Some(0),
                tr_z: Some(0x1000),
                focus_xz: Some([0, 0]),
                ..Default::default()
            });
            arm.caster_write = CaptureSeatWrite {
                x: Some(0),
                z: Some(0x80),
                facing: Some(0x800),
                rate: Some(1),
                stage: Some(0x0A),
                restage: true,
                ..Default::default()
            };
            arm.victim_write = CaptureSeatWrite {
                x: Some(0),
                z: Some(-0x80),
                facing: Some(0),
                ..Default::default()
            };
            // `depth / 2` toward zero (`srl 0x1f; addu; sra 1`).
            arm.shot = Some(shot(
                0,
                yaw_from(0x800, 0x800),
                [0, 0x300, (depth / 2) as i16],
                caster_at,
                0xC,
            ));
            return arm;
        }
        0xFF => {
            arm.caster_write.stage = Some(0);
            arm.caster_write.restage = true;
            arm.caster_radius_restore = true;
            arm.victim_write.render_flag = Some(0);
            return arm;
        }
        1..=25 => {}
        _ => return arm,
    }
    let [mut pace, _] = st.module_words;
    // The writes each arm makes on every pass, ahead of its gate.
    match phase {
        1..=4 => {
            let yaw = if phase == 3 { 4 * s } else { 0 };
            arm.drift = Some(drift(0, yaw, 0, s / 4));
        }
        5 => arm.drift = Some(drift(0, 0, 0, s / 8)),
        6 => arm.drift = Some(drift(-(s >> 1), 0, 0, -4 * s)),
        9 | 10 => {
            arm.drift = Some(drift(s / 8, -2 * s, -3 * s, 8 * s));
            arm.focus_z = -7 * s;
            arm.caster_write.dz = -8 * s;
        }
        12 | 13 => {
            arm.drift = Some(drift(0, 4 * s, s, 3 * s));
            arm.focus_z = s >> 1;
            arm.caster_write.dz = 7 * s;
        }
        15 | 16 => {
            arm.drift = Some(drift(0, 3 * s, s, s / 8));
            arm.focus_z = -2 * s;
            arm.caster_write.dz = -7 * s;
        }
        19 => arm.drift = Some(drift(s / 8, 0, (3 * s) / 4, s / 8)),
        21 => {
            arm.drift = Some(drift(s / 4, 0, -4 * s, 0));
            arm.focus_z = s / 8;
            arm.caster_write.dz = 2 * s;
            // The word as the pass finds it, ahead of the drain.
            arm.caster_write.y = Some(blazing_slash_hop_height(st.countdown.0));
        }
        22 => arm.drift = Some(drift(s / 4, 0, 0, 4 * s)),
        23 | 24 => arm.drift = Some(drift(0, 0, 0, s)),
        _ => {}
    }
    if matches!(phase, 10 | 13 | 16) {
        arm.victim_write.seek_floor = Some(VICTIM_CURSOR_FLOOR);
    }
    if phase == 23 {
        arm.victim_write.cursor_reset = true;
    }
    let hold = match phase {
        // `0x801F8B0C..0x801F8B18`: no drain, the victim's playing clip
        // against its knockdown slot.
        24 => {
            if st.wait.0 != 24 {
                st.wait = (24, 0);
            }
            st.wait.1 = st.wait.1.saturating_add(1);
            st.victim_on_knockdown_clip && st.wait.1 < ARM_24_WAIT_BOUND
        }
        22 => {
            // `0x801F8878..0x801F8954`: the drain, the paced call for every
            // pass the word stands at or under the pacing word, then the
            // hold.
            // The compare sits ahead of the gate (`slt` / `bne` at
            // `0x801F8888`, the `bgtz` at `0x801F8950`), so the pass that
            // leaves makes an eighth call on a spent word: its hit lands,
            // and its record goes with the flush behind the gate.
            st.countdown.drain();
            let held = st.countdown.0 > 0;
            if (st.opens_pass || !held) && pace >= st.countdown.0 {
                pace -= SPEED_SCALAR * FLAME_STEP;
                if held {
                    arm.pass_spawns[0] = Some(FLAME);
                }
                // `FUN_801DD6B4(0x30)` at `0x801F88D8`, behind the record.
                arm.hit_site = Some(5);
            }
            if !held {
                st.countdown.drain_rest(MODULE_DRAIN_PER_TICK);
            }
            held
        }
        _ => st.countdown.drain_above(0),
    };
    arm.hold = hold;
    if !hold {
        let rearm = match phase {
            1 | 3 | 5 | 18 | 19 => 1 << 6,
            2 | 22 => 1 << 7,
            4 | 6 | 20 | 24 => 1 << 5,
            7 | 8 | 9 | 11 | 12 | 14 | 15 => 12,
            10 | 13 => 1 << 4,
            16 => 56,
            17 => 1 << 3,
            21 => 96,
            _ => 0,
        };
        st.countdown.add(rearm);
        let knockdown = |facing: Option<u16>, dz: i16| CaptureSeatWrite {
            stage_knockdown: true,
            facing,
            dz,
            ..Default::default()
        };
        match phase {
            1 => {
                arm.shot = Some(shot(
                    0x80,
                    yaw_from(0x800, c.facing),
                    [0, 0x600, (depth / 2) as i16],
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
                // `depth / 8` toward zero (`bgez; addiu 7; sra 3`).
                arm.shot = Some(shot(
                    -0x80,
                    yaw_from(0x8C0, c.facing),
                    [0, 0x700, (depth / 8) as i16],
                    cb,
                    1,
                ));
                // `+0x1DA` and `+0x1DC` one up (`0x801F72B0..0x801F72C4`).
                arm.caster_write.stage = Some(0x0B);
                arm.caster_write.restage = true;
                arm.ctx_278 = Some(3);
            }
            5 => {
                arm.shot = Some(shot(
                    0x100,
                    yaw_from(0xC0, c.facing),
                    [0, 0x280, depth as i16],
                    cb,
                    1,
                ));
                arm.caster_write.render_flag = Some(7);
                arm.victim_write.render_flag = Some(7);
            }
            6 => {
                arm.shot = Some(shot(
                    -0x40,
                    yaw_from(0x6C0, v.facing),
                    [0, 0x300, 0x600],
                    v,
                    1,
                ));
                arm.victim_write = knockdown(None, 0);
                arm.caster_write.loop_release = true;
                arm.caster_write.render_flag = Some(0);
                // `FUN_801DD6B4(0x30)` at `0x801F74EC`.
                arm.hit_site = Some(0);
            }
            7 => {
                arm.caster_write.z = Some(0x180);
                arm.caster_write.stage = Some(0x0C);
                arm.caster_write.restage = true;
                arm.shot = Some(shot(
                    0,
                    yaw_from(0x800, c.facing),
                    [0, 0x600, 0x400],
                    ModuleSeat { z: 0x180, ..c },
                    1,
                ));
                // `sb zero,0x1fb(v0)` over `0x20` bytes (`0x801F7630..0x801F7644`).
                arm.caster_write.ghost_ring_clear = true;
            }
            // `sh zero,0x58(caster[+0x22C])` at `0x801F76A8`.
            8 => arm.caster_radius = Some(0),
            // `FUN_801DD6B4(0x38)` at `0x801F7854`, `0x801F7CA4` and
            // `0x801F80E4`, one a run.
            9 => {
                arm.victim_write = knockdown(None, 0);
                arm.hit_site = Some(1);
            }
            11 | 14 => {
                let (pitch, yaw_k, facing, z) = if phase == 11 {
                    (0x40, -0x100, 0, -0x580)
                } else {
                    (-0x40, 0x700, 0x800, 0x140)
                };
                arm.shot = Some(shot(
                    pitch,
                    yaw_from(yaw_k, v.facing),
                    [0, 0x300, 0x600],
                    vb,
                    1,
                ));
                arm.caster_write.facing = Some(facing);
                arm.caster_write.z = Some(z);
                arm.caster_write.cursor_reset = true;
            }
            12 => {
                arm.victim_write = knockdown(Some(0x800), -0x300);
                arm.hit_site = Some(2);
            }
            15 => {
                arm.victim_write = knockdown(Some(0), 0x300);
                arm.hit_site = Some(3);
            }
            18 => {
                arm.victim_write.z = Some(-0x100);
                arm.caster_write.x = Some(-8);
                arm.caster_write.z = Some(-0x400);
                // `+0x1DA` two down with the bump (`0x801F8390..0x801F83B0`).
                arm.caster_write.stage = Some(0x0A);
                arm.caster_write.restage = true;
                arm.shot = Some(shot(
                    0,
                    yaw_from(0x800, c.facing),
                    [0, 0x600, 0x300],
                    ModuleSeat {
                        x: -8,
                        z: -0x400,
                        ..c
                    },
                    1,
                ));
            }
            20 => {
                arm.caster_write.stage = Some(0x0B);
                arm.caster_write.restage = true;
            }
            21 => {
                arm.caster_write.dz = 0;
                arm.caster_write.z = Some(-0x200);
                arm.caster_write.y = Some(0);
                pace = SPEED_SCALAR * FLAME_SEED;
                // `FUN_801DD6B4(0x40)` at `0x801F8754`, then
                // `FUN_8004FCC8(0x56)` at `0x801F87BC`.
                arm.hit_site = Some(4);
                arm.cue = Some(0x56);
            }
            22 => {
                arm.caster_write.x = Some(0);
                arm.caster_write.z = Some(0x1C0);
                arm.caster_write.stage = Some(0x0D);
                arm.caster_write.restage = true;
                arm.victim_write.z = Some(-0x1C0);
                arm.victim_write.facing = Some(0x800);
                arm.victim_write.cursor_reset = true;
                arm.shot = Some(shot(
                    -0x80,
                    yaw_from(0x800, c.facing),
                    [0, 0x500, 0x600],
                    ModuleSeat {
                        x: 0,
                        z: 0x1C0,
                        ..c
                    },
                    1,
                ));
            }
            23 => arm.caster_write.render_flag = Some(0),
            25 => {
                arm.victim_write.stage = Some(0);
                arm.victim_write.restage = true;
                arm.victim_write.facing = Some(0);
                arm.victim_write.colour = Some(0x2008_0200);
                arm.caster_write.stage = Some(0);
                arm.caster_write.restage = true;
                // The depth is the `0x800` the arm has just stored into
                // `ctx[+0x6D0]` (`0x801F8C5C`).
                arm.shot = Some(shot(
                    0x80,
                    yaw_from(0x800, c.facing),
                    [0, 0x500, 0x800],
                    c,
                    1,
                ));
            }
            _ => {}
        }
    }
    // The anim-rate stores `+0x21D`: those ahead of the gate stand on a held
    // pass, and the arm's later stores land over them on the pass that
    // leaves.
    let (cr, vr): (Option<u8>, Option<u8>) = match (phase, hold) {
        (6, false) => (None, Some(2)),
        (7 | 9 | 10 | 12 | 15, _) => (None, Some(2)),
        (8 | 19 | 20, _) => (None, Some(0)),
        (11 | 14, true) => (Some(0), Some(2)),
        (11 | 14, false) => (Some(8), Some(2)),
        (13 | 16 | 17, _) => (Some(0), Some(2)),
        (18, _) => (Some(0), Some(0)),
        (21, _) => (Some(8), None),
        (22, false) => (None, Some(0)),
        (23, _) => (Some(3), Some(0)),
        (24, true) => (Some(0), Some(2)),
        (24, false) | (25, true) => (Some(0), Some(0)),
        (25, false) => (Some(8), Some(8)),
        _ => (None, None),
    };
    arm.caster_write.rate = cr;
    arm.victim_write.rate = vr;
    st.module_words[0] = pace;
    arm
}

// The per-arm part spawns, every one `FUN_80050ED4(caster + 0x34,
// caster + 0x44, record, 0x1000)`: the caster's live trio on its angle trio.
const fn spawn(record: u32) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor: SpawnAnchor::CasterShifted { dy: 0, dz: 0 },
    }
}
/// The record arm 21 seats last and arm 22 seats again on each paced pass
/// (`0x801F87F4`, `0x801F88A4`).
const FLAME: ModuleSpawn = spawn(0x801F_9348);
/// `0x801F6F9C`, arm 0.
pub(super) const E958_0: [ModuleSpawn; 1] = [spawn(0x801F_8EB8)];
/// `0x801F73B8`, arm 5's pass.
pub(super) const E958_5: [ModuleSpawn; 1] = [spawn(0x801F_9164)];
/// `0x801F78C4`, arm 9's pass.
pub(super) const E958_9: [ModuleSpawn; 1] = [spawn(0x801F_91EC)];
/// `0x801F7D14`, arm 12's pass.
pub(super) const E958_12: [ModuleSpawn; 1] = [spawn(0x801F_9260)];
/// `0x801F8154`, arm 15's pass.
pub(super) const E958_15: [ModuleSpawn; 1] = [spawn(0x801F_92D4)];
/// `0x801F86A0` and `0x801F87F4`, arm 21's pass. The first is seated at the
/// point the arm has just stored, `(x, 0, -0x200)`.
pub(super) const E958_21: [ModuleSpawn; 2] = [spawn(0x801F_9164), FLAME];

#[cfg(test)]
mod tests {
    use super::*;

    fn seats() -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 122,
                y: 0,
                z: -630,
                facing: 0x800,
            },
            victim: ModuleSeat {
                x: 40,
                y: 0,
                z: 310,
                facing: 0,
            },
            depth_raw: 0x1000,
            ..Default::default()
        }
    }

    /// Run `phase` until it passes; the ticks it took and its passing arm.
    fn run(st: &mut ModuleCamState, phase: u8) -> (u32, CaptureCamArm) {
        let mut n = 0;
        loop {
            n += 1;
            st.opens_pass = true;
            let a = blazing_slash_camera(st, phase, seats());
            if !a.hold {
                return (n, a);
            }
        }
    }

    #[test]
    fn arm_0_stands_the_pair_on_the_axis_and_cuts_to_the_caster() {
        let mut st = ModuleCamState::default();
        let a = blazing_slash_camera(&mut st, 0, seats());
        assert_eq!(st.countdown.0, SPEED_SCALAR * 12);
        assert_eq!(a.caster_write.z, Some(0x80));
        assert_eq!(a.caster_write.stage, Some(0x0A));
        assert_eq!(a.caster_write.rate, Some(1));
        assert_eq!(a.victim_write.z, Some(-0x80));
        let s = a.shot.unwrap();
        assert_eq!(s.angles, [0, 0, 0]);
        assert_eq!(s.tr, [0, 0x300, 0x800]);
        assert_eq!(s.focus, [0, 0, -0x80]);
        assert_eq!(s.frames, 0xC);
        assert!(!a.hold);
    }

    #[test]
    fn the_gates_run_the_re_arms() {
        // Each arm's dwell in ticks is its predecessor's re-arm over the
        // per-tick drain; arm 24 waits on a clip no test host plays.
        let mut st = ModuleCamState::default();
        blazing_slash_camera(&mut st, 0, seats());
        let ticks: Vec<u32> = (1..=25u8).map(|p| run(&mut st, p).0).collect();
        assert_eq!(
            ticks,
            [
                12, 64, 128, 64, 32, 64, 32, 12, 12, 12, 16, 12, 12, 16, 12, 12, 56, 8, 64, 64, 32,
                96, 128, 1, 32
            ]
        );
    }

    #[test]
    fn eleven_arms_make_a_shot() {
        let mut st = ModuleCamState::default();
        let mut shots = Vec::new();
        for phase in 0..=25u8 {
            if run(&mut st, phase).1.shot.is_some() {
                shots.push(phase);
            }
        }
        assert_eq!(shots, [0, 1, 4, 5, 6, 7, 11, 14, 18, 22, 25]);
    }

    #[test]
    fn the_dashes_carry_the_caster_and_the_cuts_stand_it_back() {
        let mut st = ModuleCamState::default();
        for phase in 0..9u8 {
            run(&mut st, phase);
        }
        let held = blazing_slash_camera(&mut st, 9, seats());
        assert!(held.hold);
        assert_eq!(held.caster_write.dz, -64);
        assert_eq!(held.focus_z, -56);
        assert_eq!(held.drift.unwrap().yaw, -16);
        let (_, pass) = run(&mut st, 9);
        assert!(pass.victim_write.stage_knockdown);
        run(&mut st, 10);
        let (_, cut) = run(&mut st, 11);
        assert_eq!(cut.caster_write.z, Some(-0x580));
        assert_eq!(cut.caster_write.facing, Some(0));
        assert_eq!(cut.caster_write.rate, Some(8));
        let s = cut.shot.unwrap();
        assert_eq!(s.angles, [0x40, 0xF00, 0]);
        assert_eq!(s.tr, [0, 0x300, 0x600]);
    }

    #[test]
    fn arm_22_paces_its_flames_and_cuts_back_to_the_caster() {
        let mut st = ModuleCamState::default();
        for phase in 0..21u8 {
            run(&mut st, phase);
        }
        let (_, landing) = run(&mut st, 21);
        assert_eq!(landing.cue, Some(0x56));
        assert_eq!(landing.caster_write.z, Some(-0x200));
        assert_eq!(landing.caster_write.y, Some(0));
        assert_eq!(st.module_words[0], SPEED_SCALAR * 84);
        let (mut flames, mut hits) = (0, 0);
        let pass = loop {
            st.opens_pass = true;
            let a = blazing_slash_camera(&mut st, 22, seats());
            flames += usize::from(a.pass_spawns[0].is_some());
            hits += usize::from(a.hit_site == Some(5));
            if !a.hold {
                break a;
            }
        };
        // The word crosses `84 s`, `72 s` .. `12 s` held; the crossing at
        // zero is the pass that leaves, whose flush takes the record and
        // whose hit still lands.
        assert_eq!(flames, 7);
        assert_eq!(hits, 8);
        assert_eq!(pass.caster_write.stage, Some(0x0D));
        assert_eq!(pass.caster_write.z, Some(0x1C0));
        assert_eq!(pass.victim_write.z, Some(-0x1C0));
        assert_eq!(pass.shot.unwrap().focus, [0, 0, -0x1C0]);
    }

    #[test]
    fn five_arms_land_a_hit_as_they_pass_and_arm_7_clears_the_ring() {
        let mut st = ModuleCamState::default();
        let mut sites = Vec::new();
        for phase in 0..=21u8 {
            let (_, a) = run(&mut st, phase);
            if let Some(site) = a.hit_site {
                sites.push((phase, site));
            }
            assert_eq!(a.caster_write.ghost_ring_clear, phase == 7);
        }
        assert_eq!(sites, [(6, 0), (9, 1), (12, 2), (15, 3), (21, 4)]);
    }

    #[test]
    fn arm_24_waits_on_the_victims_knockdown() {
        let mut st = ModuleCamState {
            victim_on_knockdown_clip: true,
            ..Default::default()
        };
        assert!(blazing_slash_camera(&mut st, 24, seats()).hold);
        st.victim_on_knockdown_clip = false;
        let a = blazing_slash_camera(&mut st, 24, seats());
        assert!(!a.hold);
        assert_eq!(st.countdown.0, SPEED_SCALAR << 5);
        assert_eq!(a.victim_write.rate, Some(0));
    }

    #[test]
    fn the_hop_rises_and_lands() {
        assert_eq!(blazing_slash_hop_height(0), 0);
        let peak = blazing_slash_hop_height(SPEED_SCALAR * 16);
        assert!((-512..=-500).contains(&peak), "{peak}");
        assert!(blazing_slash_hop_height(SPEED_SCALAR * 32).abs() < 8);
    }

    #[test]
    fn the_last_arms_hand_the_seats_back() {
        let mut st = ModuleCamState::default();
        let (_, a) = run(&mut st, 25);
        assert_eq!(a.caster_write.rate, Some(8));
        assert_eq!(a.victim_write.colour, Some(0x2008_0200));
        assert_eq!(a.shot.unwrap().tr, [0, 0x500, 0x800]);
        let end = blazing_slash_camera(&mut st, 0xFF, seats());
        assert!(end.caster_radius_restore);
        assert_eq!(end.victim_write.render_flag, Some(0));
        assert!(!end.hold);
    }
}
