//! PROT 0960 (**Plasma Strike**, `0x7B`, Lu Delilas): the tick body
//! `FUN_801F74E4`'s camera arms, phase holds, seat stores and part spawns -
//! the half the phase port ([`crate::cast_module_ticks::plasma_strike_tick`])
//! does not carry.
//!
//! The body is Megaton Press's sibling ([`super::megaton_press`]): the same
//! countdown word shape (`0x801F8E50`, `scalar * delta` a pass), the same
//! arm-0 staging on the seat axis, but its middle arms hold on the caster's
//! clip (arm 5's cursor, arm 6's loop counter, arm `0x0D`'s cursor) rather
//! than on the word, and arms 7..`0x0E` re-arm it by **storing** rather than
//! adding.
//!
//! See `ghidra/scripts/funcs/overlay_cast_plasma_strike_0960_801f74e4.txt`.

use super::capture::{CaptureCamArm, CaptureDrift, CapturePoseSet, CaptureSeatWrite};
use super::*;

/// PROT 0960's `0x7B` tick body.
pub const PLASMA_STRIKE_BODY: u32 = 0x801F_74E4;
/// PROT 0960's countdown word (`lui 0x8020` / `-0x71B0`).
pub const PLASMA_STRIKE_COUNTDOWN: u32 = 0x801F_8E50;

/// The node cursor (12.4) arm 5 waits for on the caster's clip `0x0D`
/// (`slti v0,v0,0x90` at `0x801F7B80`).
pub const PLASMA_STRIKE_ARM5_CURSOR: u16 = 0x90;
/// The node cursor arm `0x0D` waits for before its burst (`slti v0,v0,0x160`
/// at `0x801F8150`).
pub const PLASMA_STRIKE_BURST_CURSOR: u16 = 0x160;

/// The camera, holds and seat half of **Plasma Strike** (PROT 0960, body
/// `0x801F74E4`, a `beq` / `slti` tree over phases `0..=0x10` and `0xFF`), by
/// the module phase the body is about to run. With `s` the speed scalar (one
/// engine tick a frame step of one):
///
/// | arm | every pass | gate | as it passes |
/// |---|---|---|---|
/// | 0 (`0x801F7658`) | - | - | caster to `(0, 0x80)` facing `0x800` at rate `1`, staged `0x0E`; victim to `(0, -0x80)` facing `0`; yaw `0`, TR z `0x1000`, focus `(0, 0)`; shot pitch `0`, TR `(0, 0x300, depth / 2)` on the caster, `0xC` frames; countdown `= 12 s` |
/// | 1 (`0x801F7760`) | TR z `+s/4` | word | shot pitch `0x40`, yaw `0x800 - caster`, TR `(0, 0x500, depth / 2)` on its body, `0x40` frames; `+ (s << 6)` |
/// | 2 (`0x801F7844`) | TR z `+s/4` | word | `+ (s << 7)` |
/// | 3 (`0x801F78BC`) | TR z `+s/4`, yaw `+4s` | word | yaw `0x800 - caster`; `+ (s << 6)` |
/// | 4 (`0x801F7970`) | TR z `+s/4` | word | pool flush; cut pitch `-0x40`, yaw `0x400 - caster`, TR `(0, 0x400, 0x400)` on the pair's midpoint; caster staged `0x0C`; the backdrop off; `+ (s << 6)` |
/// | 5 (`0x801F7AD0`) | both `x = 0`; arm 4's cut | caster clip `0x0D` at cursor `0x90` | cut pitch `0`, yaw `0x300 - caster`, TR `(0, -5 h, 0x300)` on its body (`h` its display height); rates `2` / `0`; victim shown |
/// | 6 (`0x801F7C1C`) | rates `2` / `0`; arm 5's cut | caster loop counter off `0xFF` | `= s << 5` |
/// | 7, 9, `0x0B` | TR z `+16s`, yaw `-s` | word | cuts on the caster's body: yaw `0xA00` / `0` / `0x300` less its heading, TR z `0x200` / `0x200` / `0x300`; 7 and 9 `= s << 5`; `0x0B` releases the caster's loop |
/// | 8, `0x0A` | drain | - | - |
/// | `0x0C` | TR x `-s`, yaw `-s/2`; drain | - | (the flurry lands, the phase port) |
/// | `0x0D` (`0x801F80CC`) | TR x `-s`, yaw `-s/2`; drain | caster cursor `0x160` | (the `0x1C0` burst) four records on the victim's floor point; victim `+0x04 = 0x3FF00000`, rate `2`; `= s << 7` |
/// | `0x0E` (`0x801F82FC`) | TR x `-s`, yaw `-s/2`; victim rate `2` | word | cut yaw `0x780 - caster`, TR `(-0x80, 0x200, 0x340)` on its live pair; victim hidden; `= s << 7` |
/// | `0x0F` (`0x801F8450`) | TR y `+s`; caster rate `2` | word | `= 0x60 s` |
/// | `0x10` (`0x801F84E4`) | - | word | both shown, the caster's loop released and its stage cleared |
///
/// A clip gate whose input the host cannot answer (`None`) passes.
///
/// PORT: FUN_801F74E4 (PROT 0960; the camera arms, the countdown and clip
/// gates and the seat stores)
pub fn plasma_strike_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let s = SPEED_SCALAR as i16;
    let c = seats.caster;
    let cb = st.caster_body.unwrap_or(c);
    let vb = st.victim_body.unwrap_or(seats.victim);
    // `depth / 2`, truncated toward zero (`srl 0x1F; addu; sra 1`).
    let half_depth = (seats.depth_raw / 2) as i16;
    let shot = |pitch: i16, yaw: i16, tr: [i16; 3], focus: [i16; 3], frames: u16| ModuleShot {
        angles: [pitch, yaw, 0],
        tr,
        focus,
        frames,
    };
    // The pair's negated midpoint, `-(a + b) / 2` toward zero.
    let mid = |a: i16, b: i16| (-(i32::from(a) + i32::from(b)) / 2) as i16;
    let midpoint = [mid(cb.x, vb.x), 0, mid(cb.z, vb.z)];
    // `TR y = -5 * caster[+0x3E]` (`lh v1,0x3e(s2)`, `subu`, `* 5`).
    let lift = (-5 * i32::from(cb.y)) as i16;
    let body_cut = |yaw_k: i32, tr_z: i16| {
        shot(
            0,
            yaw_from(yaw_k, c.facing),
            [0, lift, tr_z],
            focus_on(cb),
            1,
        )
    };
    let arm4_cut = shot(
        -0x40,
        yaw_from(0x400, c.facing),
        [0, 0x400, 0x400],
        midpoint,
        1,
    );
    let drift = |pitch: i16, yaw: i16, tr_y: i16, tr_z: i16, tr_x: i16| CaptureDrift {
        pitch,
        yaw,
        tr_y,
        tr_z,
        tr_x,
    };
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    match phase {
        0 => {
            st.countdown.0 = SPEED_SCALAR * 12;
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
                stage: Some(0x0E),
                restage: true,
                ..Default::default()
            };
            arm.victim_write = CaptureSeatWrite {
                x: Some(0),
                z: Some(-0x80),
                facing: Some(0),
                ..Default::default()
            };
            arm.shot = Some(shot(
                0,
                yaw_from(0x800, 0x800),
                [0, 0x300, half_depth],
                focus_on(caster_at),
                0xC,
            ));
        }
        1..=4 => {
            let yaw = if phase == 3 { 4 * s } else { 0 };
            arm.drift = Some(drift(0, yaw, 0, s / 4, 0));
            arm.hold = st.countdown.drain_above(0);
            if !arm.hold {
                st.countdown.add(if phase == 2 { 1 << 7 } else { 1 << 6 });
                match phase {
                    1 => {
                        arm.shot = Some(shot(
                            0x40,
                            yaw_from(0x800, c.facing),
                            [0, 0x500, half_depth],
                            focus_on(cb),
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
                        arm.shot = Some(arm4_cut);
                        arm.caster_write.stage = Some(0x0C);
                        arm.caster_write.restage = true;
                        // `sh 0x1000,0x78` on both backdrop records
                        // (`0x801F7A84..0x801F7AA0`).
                        arm.backdrop_off = true;
                        // `sb s6,0x278(v0)` with `s6 = 3` (`0x801F7A80`).
                        arm.ctx_278 = Some(3);
                    }
                    _ => {}
                }
            }
        }
        5 => {
            arm.shot = Some(arm4_cut);
            // `li v0,0xd; sb v0,0x1da(s2)` every pass, with no bump
            // (`0x801F7ADC`): the byte waits for clip `0x0C`'s end.
            arm.caster_write.stage = Some(0x0D);
            arm.caster_write.x = Some(0);
            arm.victim_write.x = Some(0);
            arm.hold = !(st.caster_clip.is_none_or(|k| k == 0x0D)
                && st
                    .caster_frame
                    .is_none_or(|f| f >= PLASMA_STRIKE_ARM5_CURSOR));
            if !arm.hold {
                arm.shot = Some(body_cut(0x300, 0x300));
                arm.caster_write.rate = Some(2);
                arm.victim_write.rate = Some(0);
                arm.victim_write.colour = Some(0x2008_0200);
                arm.victim_write.render_flag = Some(0);
            }
        }
        6 => {
            arm.shot = Some(body_cut(0x300, 0x300));
            arm.caster_write.rate = Some(2);
            arm.victim_write.rate = Some(0);
            arm.hold = st.caster_loops == Some(0xFF);
            if !arm.hold {
                st.countdown.0 = SPEED_SCALAR << 5;
            }
        }
        7 | 9 | 0x0B => {
            arm.drift = Some(drift(0, -s, 0, 16 * s, 0));
            arm.hold = st.countdown.drain_above(0);
            if !arm.hold {
                arm.shot = Some(match phase {
                    7 => body_cut(0xA00, 0x200),
                    9 => body_cut(0, 0x200),
                    _ => body_cut(0x300, 0x300),
                });
                if phase == 0x0B {
                    arm.caster_write.loop_release = true;
                } else {
                    st.countdown.0 = SPEED_SCALAR << 5;
                }
            }
        }
        8 | 0x0A => st.countdown.drain(),
        0x0C => {
            arm.drift = Some(drift(0, -(s >> 1), 0, 0, -s));
            st.countdown.drain();
        }
        0x0D => {
            arm.drift = Some(drift(0, -(s >> 1), 0, 0, -s));
            st.countdown.drain();
            arm.hold = st
                .caster_frame
                .is_some_and(|f| f < PLASMA_STRIKE_BURST_CURSOR);
            if !arm.hold {
                st.countdown.0 = SPEED_SCALAR << 7;
                arm.victim_write.colour = Some(0x3FF0_0000);
                arm.victim_write.render_flag = Some(0);
                arm.victim_write.rate = Some(2);
            }
        }
        0x0E => {
            arm.drift = Some(drift(0, -(s >> 1), 0, 0, -s));
            arm.victim_write.rate = Some(2);
            arm.hold = st.countdown.drain_above(0);
            if !arm.hold {
                st.countdown.0 = SPEED_SCALAR << 7;
                arm.shot = Some(shot(
                    0,
                    yaw_from(0x780, c.facing),
                    [-0x80, 0x200, 0x340],
                    focus_on(c),
                    1,
                ));
                arm.victim_write.colour = Some(0);
                arm.victim_write.render_flag = Some(0xFF);
                arm.caster_write.rate = Some(2);
            }
        }
        0x0F => {
            arm.drift = Some(drift(0, 0, s, 0, 0));
            arm.caster_write.rate = Some(2);
            arm.hold = st.countdown.drain_above(0);
            if !arm.hold {
                st.countdown.0 = SPEED_SCALAR * 0x60;
            }
        }
        0x10 => {
            arm.hold = st.countdown.drain_above(0);
            if !arm.hold {
                arm.caster_write.loop_release = true;
                arm.caster_write.colour = Some(0x2008_0200);
                arm.caster_write.render_flag = Some(0);
                arm.victim_write.colour = Some(0x2008_0200);
                arm.victim_write.render_flag = Some(0);
            }
        }
        _ => {}
    }
    arm
}

/// PROT 0960's `0xA6` tick body (Neo Star Slash, the Sim-Seru Gaza).
pub const NEO_STAR_SLASH_BODY: u32 = 0x801F_69D8;

/// The camera, countdown and seat half of **Neo Star Slash** (PROT 0960,
/// body `0x801F69D8`, a `beq` / `slti` chain over `0..=6` and `0xFF`; its
/// phase chain is [`crate::cast_module_ticks::NEO_STAR_SLASH_CHAIN`]). The
/// countdown is the word `0x801F8E50`, drained `s * d` a pass and re-armed by
/// each passing arm; `depth` is `ctx[+0x6D0]`. Every shot is on the caster:
///
/// | arm | every pass | gate | as it passes |
/// |---|---|---|---|
/// | 0 (`0x801F6AC4`) | - | - | cue `0x15D`; shot pitch `0`, yaw `0x800 - facing`, TR `(0, 0x800, depth / 2)`, `0xC` frames; `= s << 5` |
/// | 1 (`0x801F6B48`) | TR z `+s/2` | word | (clip `0x0A`, rate `4`: the chain); `= 0x60 s` |
/// | 2 (`0x801F6BCC`) | caster rate `4`; TR z `+s/2` | word | cut pitch `0xC0`, yaw `0x800 - facing`, TR `(0, 0, 3 depth)`; seven records on the caster; `+ 0xC0 s` |
/// | 3 (`0x801F6D5C`) | caster rate `4`; TR z `-s/2` | word | the caster's loop released, rate `8`; cut pitch `0x60`, yaw `0x800 - facing`, TR `(0, 0x300, 3 depth / 4)`; the party seats hidden (`+0x04 = 0`, `+0x21C = 0xFF`); `+ 0x1C s` |
/// | 4 (`0x801F6EB4`) | caster rate `4`; TR z `+8s` | word | pool flush; the white and red fades; the party shown; two records on the caster; `+ (s << 7)` (its caster rate `0` is left out, as the chain leaves it) |
/// | 5 (`0x801F7048`) | TR z `+8s` | word | two records on the caster, three on each point of a ten-point ring about it and three on each live party seat; (the `0x2C6` party sweep: the chain); `+ 0xA0 s` |
/// | 6 (`0x801F73A8`) | TR z `+4s` | word | (the party row's settle: the chain) |
///
/// PORT: FUN_801F69D8 (PROT 0960 `0xA6`; the camera arms, the countdown gates
/// and the caster / party stores)
pub fn neo_star_slash_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let s = SPEED_SCALAR as i16;
    let c = seats.caster;
    let depth = seats.depth_raw;
    let shot = |pitch: i16, tr: [i16; 3], frames: u16| ModuleShot {
        angles: [pitch, yaw_from(0x800, c.facing), 0],
        tr,
        focus: focus_on(c),
        frames,
    };
    let tr_z = |k: i16| CaptureDrift {
        tr_z: k,
        ..Default::default()
    };
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    match phase {
        0 => {
            st.countdown.0 = SPEED_SCALAR << 5;
            // `depth / 2` toward zero (`srl 0x1f; addu; sra 1`).
            arm.shot = Some(shot(0, [0, 0x800, (depth / 2) as i16], 0xC));
        }
        1..=6 => {
            if (2..=4).contains(&phase) {
                arm.caster_write.rate = Some(4);
            }
            arm.drift = Some(tr_z(match phase {
                1 | 2 => s >> 1,
                3 => -(s >> 1),
                6 => 4 * s,
                _ => 8 * s,
            }));
            arm.hold = st.countdown.drain_above(0);
            if !arm.hold {
                match phase {
                    1 => st.countdown.0 = SPEED_SCALAR * 0x60,
                    2 => {
                        st.countdown.add(0xC0);
                        arm.shot = Some(shot(0xC0, [0, 0, (3 * depth) as i16], 1));
                    }
                    3 => {
                        st.countdown.add(0x1C);
                        arm.caster_write.loop_release = true;
                        arm.caster_write.rate = Some(8);
                        // `(3 * depth) / 4` toward zero (`bgez; addiu 3; sra 2`).
                        arm.shot = Some(shot(0x60, [0, 0x300, ((3 * depth) / 4) as i16], 1));
                        arm.party_write = CaptureSeatWrite {
                            colour: Some(0),
                            render_flag: Some(0xFF),
                            ..Default::default()
                        };
                    }
                    4 => {
                        st.countdown.add(1 << 7);
                        // Arm 4's `sb zero,0x21d(s4)` (`0x801F6F8C`) is left
                        // out under the module's rate rule: nothing in the
                        // body restores it, and a frozen caster would hold
                        // the band's close on a clip that never ends.
                        arm.party_write = CaptureSeatWrite {
                            colour: Some(0x2008_0200),
                            render_flag: Some(0),
                            ..Default::default()
                        };
                    }
                    5 => st.countdown.add(0xA0),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    arm
}

const fn spawn(record: u32, anchor: SpawnAnchor) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor,
    }
}
/// `0x801F7750`, arm 0: on the caster's `+0x34` / `+0x44`.
pub(super) const E960_0: [ModuleSpawn; 1] = [spawn(
    0x801F_8768,
    SpawnAnchor::CasterShifted { dy: 0, dz: 0 },
)];
/// `0x801F82A4..0x801F82EC`, arm `0x0D`'s burst: the victim's
/// `+0x3C..+0x43` copied with the height zeroed, on the caster's angle trio.
pub(super) const E960_D: [ModuleSpawn; 4] = [
    spawn(0x801F_883C, SpawnAnchor::VictimBodyGroundCasterRot),
    spawn(0x801F_88A4, SpawnAnchor::VictimBodyGroundCasterRot),
    spawn(0x801F_88E4, SpawnAnchor::VictimBodyGroundCasterRot),
    spawn(0x801F_8924, SpawnAnchor::VictimBodyGroundCasterRot),
];

/// `0x801F6C78..0x801F6D24`, `0xA6`'s arm 2: seven records on the caster.
pub(super) const E960_A6_2: [ModuleSpawn; 7] = [
    spawn(0x801F_8980, SpawnAnchor::CasterShifted { dy: 0, dz: 0 }),
    spawn(0x801F_89CC, SpawnAnchor::CasterShifted { dy: 0, dz: 0 }),
    spawn(0x801F_8A18, SpawnAnchor::CasterShifted { dy: 0, dz: 0 }),
    spawn(0x801F_8A64, SpawnAnchor::CasterShifted { dy: 0, dz: 0 }),
    spawn(0x801F_8AB0, SpawnAnchor::CasterShifted { dy: 0, dz: 0 }),
    spawn(0x801F_8B04, SpawnAnchor::CasterShifted { dy: 0, dz: 0 }),
    spawn(0x801F_8B90, SpawnAnchor::CasterShifted { dy: 0, dz: 0 }),
];
/// `0x801F7000..0x801F7034`, `0xA6`'s arm 4: two records on the caster.
pub(super) const E960_A6_4: [ModuleSpawn; 2] = [
    spawn(0x801F_8BF4, SpawnAnchor::CasterShifted { dy: 0, dz: 0 }),
    spawn(0x801F_8C58, SpawnAnchor::CasterShifted { dy: 0, dz: 0 }),
];
/// `0x801F7094..0x801F7268`, `0xA6`'s arm 5: two records on the caster,
/// then three on each point of a ten-point ring about it (radius `0x1000 /
/// 3`), then the same three on every live party seat ahead of its hit.
pub(super) const E960_A6_5: [ModuleSpawn; 41] = [
    spawn(0x801F_8D44, SpawnAnchor::CasterShifted { dy: 0, dz: 0 }),
    spawn(0x801F_8DA8, SpawnAnchor::CasterShifted { dy: 0, dz: 0 }),
    spawn(
        0x801F_8CB8,
        SpawnAnchor::CasterRing {
            index: 0,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CF0,
        SpawnAnchor::CasterRing {
            index: 0,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8E0C,
        SpawnAnchor::CasterRing {
            index: 0,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CB8,
        SpawnAnchor::CasterRing {
            index: 1,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CF0,
        SpawnAnchor::CasterRing {
            index: 1,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8E0C,
        SpawnAnchor::CasterRing {
            index: 1,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CB8,
        SpawnAnchor::CasterRing {
            index: 2,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CF0,
        SpawnAnchor::CasterRing {
            index: 2,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8E0C,
        SpawnAnchor::CasterRing {
            index: 2,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CB8,
        SpawnAnchor::CasterRing {
            index: 3,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CF0,
        SpawnAnchor::CasterRing {
            index: 3,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8E0C,
        SpawnAnchor::CasterRing {
            index: 3,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CB8,
        SpawnAnchor::CasterRing {
            index: 4,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CF0,
        SpawnAnchor::CasterRing {
            index: 4,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8E0C,
        SpawnAnchor::CasterRing {
            index: 4,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CB8,
        SpawnAnchor::CasterRing {
            index: 5,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CF0,
        SpawnAnchor::CasterRing {
            index: 5,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8E0C,
        SpawnAnchor::CasterRing {
            index: 5,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CB8,
        SpawnAnchor::CasterRing {
            index: 6,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CF0,
        SpawnAnchor::CasterRing {
            index: 6,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8E0C,
        SpawnAnchor::CasterRing {
            index: 6,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CB8,
        SpawnAnchor::CasterRing {
            index: 7,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CF0,
        SpawnAnchor::CasterRing {
            index: 7,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8E0C,
        SpawnAnchor::CasterRing {
            index: 7,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CB8,
        SpawnAnchor::CasterRing {
            index: 8,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CF0,
        SpawnAnchor::CasterRing {
            index: 8,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8E0C,
        SpawnAnchor::CasterRing {
            index: 8,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CB8,
        SpawnAnchor::CasterRing {
            index: 9,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8CF0,
        SpawnAnchor::CasterRing {
            index: 9,
            count: 10,
            div: 3,
        },
    ),
    spawn(
        0x801F_8E0C,
        SpawnAnchor::CasterRing {
            index: 9,
            count: 10,
            div: 3,
        },
    ),
    spawn(0x801F_8CB8, SpawnAnchor::PartySeat(0)),
    spawn(0x801F_8CF0, SpawnAnchor::PartySeat(0)),
    spawn(0x801F_8E0C, SpawnAnchor::PartySeat(0)),
    spawn(0x801F_8CB8, SpawnAnchor::PartySeat(1)),
    spawn(0x801F_8CF0, SpawnAnchor::PartySeat(1)),
    spawn(0x801F_8E0C, SpawnAnchor::PartySeat(1)),
    spawn(0x801F_8CB8, SpawnAnchor::PartySeat(2)),
    spawn(0x801F_8CF0, SpawnAnchor::PartySeat(2)),
    spawn(0x801F_8E0C, SpawnAnchor::PartySeat(2)),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_gates_and_clip_gates_pace_the_body() {
        let seats = ModuleCamSeats {
            depth_raw: 0x1000,
            ..Default::default()
        };
        let mut st = ModuleCamState::default();
        let a = plasma_strike_camera(&mut st, 0, seats);
        assert_eq!(a.shot.unwrap().tr, [0, 0x300, 0x800]);
        let dwell = |st: &mut ModuleCamState, phase: u8| {
            (1..)
                .find(|_| !plasma_strike_camera(st, phase, seats).hold)
                .unwrap()
        };
        assert_eq!([1, 2, 3, 4].map(|p| dwell(&mut st, p)), [12, 64, 128, 64]);
        // Arm 5 holds until the caster's clip `0x0D` reaches cursor `0x90`.
        st.caster_clip = Some(0x0D);
        st.caster_frame = Some(0x80);
        assert!(plasma_strike_camera(&mut st, 5, seats).hold);
        st.caster_frame = Some(0x90);
        assert!(!plasma_strike_camera(&mut st, 5, seats).hold);
        // Arm 6 holds on the loop counter, then stores `s << 5`.
        st.caster_loops = Some(0xFF);
        assert!(plasma_strike_camera(&mut st, 6, seats).hold);
        st.caster_loops = Some(0);
        assert!(!plasma_strike_camera(&mut st, 6, seats).hold);
        assert_eq!(dwell(&mut st, 7), 32);
    }

    #[test]
    fn neo_star_slash_paces_on_its_word_and_hides_the_party() {
        let seats = ModuleCamSeats {
            depth_raw: 0x1000,
            ..Default::default()
        };
        let mut st = ModuleCamState::default();
        let a = neo_star_slash_camera(&mut st, 0, seats);
        assert_eq!(a.shot.unwrap().tr, [0, 0x800, 0x800]);
        let dwell = |st: &mut ModuleCamState, phase: u8| {
            (1..)
                .find(|_| !neo_star_slash_camera(st, phase, seats).hold)
                .unwrap()
        };
        assert_eq!(dwell(&mut st, 1), 32);
        assert_eq!(dwell(&mut st, 2), 96);
        assert_eq!(dwell(&mut st, 3), 192);
        let a = (0..)
            .map(|_| neo_star_slash_camera(&mut st, 4, seats))
            .find(|a| !a.hold)
            .unwrap();
        assert_eq!(a.party_write.render_flag, Some(0));
    }
}
