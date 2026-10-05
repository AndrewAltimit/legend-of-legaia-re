//! The summon-creature modules' opening framings (PROT 0914..0934) - the
//! arms a Seru summon runs through the band's sustain `0x35`.
//!
//! These modules' tick bodies are unported, so their directors are
//! **camera-only** ([`ModuleProfile::camera_only`]): they own the module
//! phase for the arms they cover - a pass advances it - and **park** on the
//! first arm they do not, where the camera holds the last framing. Nothing in
//! the band waits on them; the engine's stager choreography keeps deciding
//! how long `0x36` lasts.
//!
//! Every arm below opens the cast the same way: a cut at arm 0 (or a cut and
//! then a pan at arm 1), then the creature stream load and its CD poll, which
//! the engine reads as ready. Several drift the camera globals while a
//! countdown runs ([`ModuleNudge`]); PROT 0923 counts on the frame delta
//! alone rather than on the scalar-times-delta product the player Seru use.

use super::*;

/// Vsyncs per engine tick: the unit of the delta-only drains
/// (`*(0x1F800393)` per battle frame, a battle frame being that many vsyncs).
pub const DELTA_PER_TICK: i32 = 1;

/// PROT 0914 (Gola Gola): a cut at arm 0, a second cut at arm 1, a TR z drift
/// of `4 * delta` a pass through the load arms 2 and 3, and at arm 3 the
/// creature's first framing - pitch `-0x180`, yaw `0x400`, TR
/// `(0, 0x600, 0x100)` over `0x40` frames (`0x801F6D78`). Every framing aims
/// at the caster offset by `(0x400, -0x267)` (arm 1: `(0x18E, -0x267)`). Parks
/// at arm 4, which waits on the creature's clip frame.
///
/// PORT: FUN_801F69F0 (PROT 0914; camera arms 0/1/3 and the arm 2/3 drift)
pub fn gola_gola_direct(
    _st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> ArmDirection {
    let c = seats.caster;
    let at = |dx: i32, dz: i32| {
        [
            (-i32::from(c.x) - dx) as i16,
            0,
            (-i32::from(c.z) - dz) as i16,
        ]
    };
    let drift = ModuleNudge {
        tr_z: (4 * DELTA_PER_TICK) as i16,
        ..Default::default()
    };
    match phase {
        0 => ArmDirection::shot(ModuleShot {
            angles: [0, 0x800, 0],
            tr: [0, 0x600, 0x600],
            focus: at(0x400, -0x267),
            frames: 1,
        }),
        1 => ArmDirection::shot(ModuleShot {
            angles: [0x200, 0x400, 0],
            tr: [0, 0, 0x600],
            focus: at(0x18E, -0x267),
            frames: 1,
        }),
        2 => ArmDirection::PASS.nudged(drift),
        3 => ArmDirection::shot(ModuleShot {
            angles: [-0x180, 0x400, 0],
            tr: [0, 0x600, 0x100],
            focus: at(0x400, -0x267),
            frames: 0x40,
        })
        .nudged(drift),
        _ => ArmDirection::PARK,
    }
}

/// PROT 0915 (Mushura): a cut on the victim at arm 0 (pitch `0`, yaw `0x800`,
/// TR `(0, 0x400, 0x800)`), the same framing pitched to `0x200` over `0x80`
/// frames at arm 1 (`0x801F6CA8`), the load arms 2 and 3 (arm 3 arms the
/// countdown `0x801F814C = scalar << 6`), and at arm 4, once the countdown is
/// spent, a cut to pitch `-0x40`, yaw `0xA00`, TR `(0, 0x500, 0x1000)` on the
/// point half a turn of `0x800` past the victim (`0x801F6E0C`). Parks at arm 5.
///
/// Arm 0 also scatters the monster seats (`FUN_80056798` draws into their
/// `+0x34` / `+0x38`); that is the module's staging, not its camera, and is
/// not here.
///
/// PORT: FUN_801F69D8 (PROT 0915; camera arms 0/1/4 and arm 4's countdown gate)
pub fn mushura_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let v = seats.victim;
    match phase {
        0 => ArmDirection::shot(ModuleShot {
            angles: [0, 0x800, 0],
            tr: [0, 0x400, 0x800],
            focus: focus_on(v),
            frames: 1,
        }),
        1 => ArmDirection::shot(ModuleShot {
            angles: [0x200, 0x800, 0],
            tr: [0, 0x400, 0x800],
            focus: focus_on(v),
            frames: 0x80,
        }),
        2 => ArmDirection::PASS,
        3 => {
            st.countdown.arm(6);
            ArmDirection::PASS
        }
        4 => {
            if st.countdown.drain_above(0) {
                return ArmDirection::HOLD;
            }
            st.countdown.add(1 << 6);
            ArmDirection::shot(ModuleShot {
                angles: [-0x40, 0xA00, 0],
                tr: [0, 0x500, 0x1000],
                focus: [v.x.wrapping_neg(), 0, (0x800 - i32::from(v.z)) as i16],
                frames: 1,
            })
        }
        _ => ArmDirection::PARK,
    }
}

/// PROT 0917 (Barra): a cut at arm 0 - pitch `0`, yaw `0x800 - h`, TR
/// `(0, 0x400, 0x400)`, on the point a quarter unit from the victim back
/// along `h`, the heading from the caster to the victim (`0x801F6C5C`) - then
/// at arm 1 a climb written straight into the globals, TR y `+= p` and pitch
/// `-= p / 2` a pass for as long as TR y is below `0x800` (`0x801F6CF8`),
/// before the load. Parks at arm 2.
///
/// PORT: FUN_801F6A30, overlay_summon_barra_0917_801f6a30 (PROT 0917; camera arm 0 and the arm 1 climb)
pub fn barra_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let v = seats.victim;
    let h = heading(v, seats.caster).wrapping_add(0x800) & 0xFFF;
    let p = MODULE_DRAIN_PER_TICK;
    match phase {
        0 => {
            let (sin, cos) = trig12(h);
            st.tr_y = 0x400;
            ArmDirection::shot(ModuleShot {
                angles: [0, yaw_from(0x800, h), 0],
                tr: [0, 0x400, 0x400],
                focus: [
                    (-i32::from(v.x) + i32::from(sin) / 4) as i16,
                    0,
                    (-i32::from(v.z) + i32::from(cos) / 4) as i16,
                ],
                frames: 1,
            })
        }
        1 => {
            if st.tr_y < 0x800 {
                st.tr_y += p;
                return ArmDirection::HOLD.nudged(ModuleNudge {
                    pitch: -(p >> 1) as i16,
                    tr_y: p as i16,
                    tr_z: 0,
                });
            }
            ArmDirection::PASS
        }
        _ => ArmDirection::PARK,
    }
}

/// PROT 0920 (Slippery): a cut on the caster at arm 0 - pitch `0`, yaw
/// `0x800 - facing`, TR `(0, 0x580, 0x600)` (`0x801F6B90`) - with the
/// countdown `0x801F8440 = scalar << 7`; arms 1 and 2 push TR z out by `p` a
/// pass while it runs, arm 2 holding until it is spent, then seating the
/// creature and re-arming it by `scalar << 6`. Parks at arm 3.
///
/// PORT: FUN_801F69D8 (PROT 0920; camera arm 0 and the arm 1/2 drift and gate)
pub fn slippery_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let c = seats.caster;
    let p = MODULE_DRAIN_PER_TICK;
    let push = ModuleNudge {
        tr_z: p as i16,
        ..Default::default()
    };
    match phase {
        0 => {
            st.countdown.arm(7);
            ArmDirection::shot(ModuleShot {
                angles: [0, yaw_from(0x800, c.facing), 0],
                tr: [0, 0x580, 0x600],
                focus: focus_on(c),
                frames: 1,
            })
        }
        1 => {
            if st.countdown.0 > 0 {
                st.countdown.drain();
                return ArmDirection::PASS.nudged(push);
            }
            ArmDirection::PASS
        }
        2 => {
            if st.countdown.0 > 0 {
                st.countdown.drain();
                return ArmDirection::HOLD.nudged(push);
            }
            st.countdown.add(1 << 6);
            ArmDirection::PASS.nudged(push)
        }
        _ => ArmDirection::PARK,
    }
}

/// PROT 0923 (Gilium): a cut at arm 0 - pitch `0`, yaw `0x800`, TR
/// `(0, 0x600, 0x1000)`, focus the stage origin (`0x801F6BCC`) - with the
/// countdown `0x801FA4F8 = 0x200`, which arms 1..3 drain by `8 * delta` a pass
/// while pulling TR z in by `4 * delta`; arms 2 and 3 hold until it is spent,
/// then re-arm it by `0x200` and `0x400`, arm 3 seating the creature at
/// `(0, -0x100, -0x800)`. Parks at arm 4.
///
/// PORT: FUN_801F69D8 (PROT 0923; camera arm 0 and the arm 1..3 drift and gates)
pub fn gilium_direct(st: &mut ModuleCamState, phase: u8, _seats: ModuleCamSeats) -> ArmDirection {
    let pull = ModuleNudge {
        tr_z: -(4 * DELTA_PER_TICK) as i16,
        ..Default::default()
    };
    // `if (cd > 0) { cd -= 8 * delta; TR z -= 4 * delta; }`, and whether the
    // word is still positive after it.
    let drift = |st: &mut ModuleCamState| -> (bool, Option<ModuleNudge>) {
        if st.countdown.0 > 0 {
            st.countdown.0 -= 8 * DELTA_PER_TICK;
            (st.countdown.0 > 0, Some(pull))
        } else {
            (false, None)
        }
    };
    match phase {
        0 => {
            st.countdown.0 = 0x200;
            ArmDirection::shot(ModuleShot {
                angles: [0, 0x800, 0],
                tr: [0, 0x600, 0x1000],
                focus: [0; 3],
                frames: 1,
            })
        }
        1 => {
            let (_, nudge) = drift(st);
            ArmDirection {
                nudge,
                ..ArmDirection::PASS
            }
        }
        2 | 3 => {
            let (still, nudge) = drift(st);
            if still {
                return ArmDirection {
                    nudge,
                    ..ArmDirection::HOLD
                };
            }
            st.countdown.0 += if phase == 2 { 0x200 } else { 0x400 };
            if phase == 3 {
                st.creature = Some(ModuleSeat {
                    x: 0,
                    y: -0x100,
                    z: -0x800,
                    facing: 0,
                });
            }
            ArmDirection {
                nudge,
                ..ArmDirection::PASS
            }
        }
        _ => ArmDirection::PARK,
    }
}

/// PROT 0928 (Palma): a cut at arm 0 - pitch `0x80`, yaw `0`, TR
/// `(0, 0x200, 0x2000)`, focus the stage origin (`0x801F6C18`) - then the
/// load. Parks at arm 2, the CD poll.
///
/// Arm 0 also pulls every monster seat to an eighth of its position and
/// re-lights the stage (`0x8007BE6C..0x8007BE74`); neither is camera.
///
/// PORT: FUN_801F69F4 (PROT 0928; camera arm 0)
pub fn palma_direct(_st: &mut ModuleCamState, phase: u8, _seats: ModuleCamSeats) -> ArmDirection {
    match phase {
        0 => ArmDirection::shot(ModuleShot {
            angles: [0x80, 0, 0],
            tr: [0, 0x200, 0x2000],
            focus: [0; 3],
            frames: 1,
        }),
        1 => ArmDirection::PASS,
        _ => ArmDirection::PARK,
    }
}

/// PROT 0930 (Horn): a cut at arm 0 - pitch / yaw `0`, TR `(0, 0x400, 0x400)`,
/// focus the stage origin (`0x801F6B40`) - and at arm 1 a rise to TR
/// `(0, 0x1500, 0)` on `(0, 0, 0x200)` over `0x100` frames (`0x801F6D44`),
/// then the load arms 2 and 3. Parks at arm 3.
///
/// PORT: FUN_801F6A74, overlay_summon_horn_0930_801f6a74 (PROT 0930; camera arms 0/1)
pub fn horn_direct(_st: &mut ModuleCamState, phase: u8, _seats: ModuleCamSeats) -> ArmDirection {
    match phase {
        0 => ArmDirection::shot(ModuleShot {
            angles: [0; 3],
            tr: [0, 0x400, 0x400],
            focus: [0; 3],
            frames: 1,
        }),
        1 => ArmDirection::shot(ModuleShot {
            angles: [0; 3],
            tr: [0, 0x1500, 0],
            focus: [0, 0, -0x200],
            frames: 0x100,
        }),
        2 => ArmDirection::PASS,
        _ => ArmDirection::PARK,
    }
}

/// PROT 0931 (Jedo): a cut at arm 0 - pitch `-0x80`, yaw `0xA00`, TR
/// `(0, 0x800, 0x1000)` on `(0, 0, -0x600)` (`0x801F6B20`) - and at arm 1 a
/// pull-back to TR `(0, 0xC00, 0x1800)` over `0xAA` frames (`0x801F6CFC`).
/// Parks at arm 2, the load. (Its dispatch is a jump table at the image head,
/// `sltiu 0x20`.)
///
/// Arm 0 also re-seats the monster row from the formation table at
/// `0x80077628`; that is staging, not camera.
///
/// PORT: FUN_801F6A58, overlay_summon_jedo_0931_801f6a58 (PROT 0931; camera arms 0/1)
pub fn jedo_direct(_st: &mut ModuleCamState, phase: u8, _seats: ModuleCamSeats) -> ArmDirection {
    match phase {
        0 => ArmDirection::shot(ModuleShot {
            angles: [-0x80, 0xA00, 0],
            tr: [0, 0x800, 0x1000],
            focus: [0, 0, 0x600],
            frames: 1,
        }),
        1 => ArmDirection::shot(ModuleShot {
            angles: [-0x80, 0xA00, 0],
            tr: [0, 0xC00, 0x1800],
            focus: [0, 0, 0x600],
            frames: 0xAA,
        }),
        _ => ArmDirection::PARK,
    }
}

/// PROT 0913 (Nova, the player Seru whose module opens with a creature
/// stream read): a cut at arm 0 behind the caster - pitch `0x400`, yaw
/// `0x900 - caster[+0x46]`, TR `(0, 0, 0x400)`, focus the caster, one frame
/// (`0x801F6BCC..0x801F6C54`) - then a TR z drift of `(scalar * delta) / 4`
/// a pass through arm 1 (the stream request, `0x801F6C64`) and arm 2 (the CD
/// poll `FUN_8003F2B8(1)`, `0x801F6CD0`). Retail sits in arm 2 for as long
/// as the read takes, drifting. Nova's tick body is ported
/// (`cast_seru_ticks_b`), so this director runs beside it
/// ([`ModuleProfile::camera_beside`]) and holds nothing; the engine's read is
/// always ready, and the later framings (arm 2's `0x40`-frame shot on the
/// creature once the read lands, and on) are not directed. The
/// `nova_summon_mid_cast` capture is in arm 2 on arm 0's cut, `48` units of
/// drift out.
///
/// PORT: FUN_801F69F0 (PROT 0913; camera arm 0 and the arm 1/2 drift)
pub fn nova_direct(_st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let c = seats.caster;
    let drift = ModuleNudge {
        tr_z: (MODULE_DRAIN_PER_TICK / 4) as i16,
        ..Default::default()
    };
    match phase {
        0 => ArmDirection::shot(ModuleShot {
            angles: [0x400, yaw_from(0x900, c.facing), 0],
            tr: [0, 0, 0x400],
            focus: focus_on(c),
            frames: 1,
        }),
        1 | 2 => ArmDirection::PASS.nudged(drift),
        _ => ArmDirection::PASS,
    }
}

/// The cut every one of the six below opens or closes on: angles, TR and
/// focus straight from immediates.
fn cut(angles: [i16; 3], tr: [i16; 3], focus: [i16; 3], frames: u16) -> ArmDirection {
    ArmDirection::shot(ModuleShot {
        angles,
        tr,
        focus,
        frames,
    })
}

/// A delta-only countdown gate (`lbu 0x393; subu; bgtz`): drain one frame
/// delta, hold while the word is positive, otherwise re-arm it by `rearm`
/// (added to what is left, as the `addiu v0, v0, N` after the branch does).
fn delta_gate(st: &mut ModuleCamState, rearm: i32) -> bool {
    st.countdown.0 -= DELTA_PER_TICK;
    if st.countdown.0 > 0 {
        return true;
    }
    st.countdown.0 += rearm;
    false
}

/// PROT 0916 (Aluru): with `h` the heading from the victim to the caster
/// turned half round, a cut at arm 0 - pitch `-0x80`, yaw `0x800 - h`, TR
/// `(0, 0x400, 0x800)`, on the point half a unit from the victim along `h`
/// (`0x801F6C00..0x801F6CCC`). Arm 1 is the stream request; it skips arm 2
/// when the band timer is already spent, and arm 2 otherwise waits for it -
/// folded here into arm 2's gate, one tick later. Arm 3 (the CD poll) arms
/// the band timer `ctx[+0x6D8] = scalar * 20`, which arm 4 drains by
/// `scalar * delta` while it stays non-negative (`bgez`); this director keeps
/// that word in its own countdown. Arm 4 then seats the creature half a unit
/// from the victim along `h`, facing `h`, and frames it - pitch `0`, yaw
/// `0x800 - h`, TR `(0, 0x500, 0x400)` over `0x80` frames
/// (`0x801F7040..0x801F7190`). Parks at arm 5.
///
/// PORT: overlay_summon_aluru_0916_801f69f8 (PROT 0916; camera arms 0 and 4, the arm 2 wait and the arm 4 countdown)
pub fn aluru_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let v = seats.victim;
    let h = heading(v, seats.caster).wrapping_add(0x800) & 0xFFF;
    let (sin, cos) = trig12(h);
    match phase {
        0 => cut(
            [-0x80, yaw_from(0x800, h), 0],
            [0, 0x400, 0x800],
            [
                (half(sin) - i32::from(v.x)) as i16,
                0,
                (half(cos) - i32::from(v.z)) as i16,
            ],
            1,
        ),
        1 => ArmDirection::PASS,
        2 => gate(seats.band_timer != 0),
        3 => {
            st.countdown.0 = SPEED_SCALAR * 20;
            ArmDirection::PASS
        }
        4 => {
            if st.countdown.drain_non_negative() {
                return ArmDirection::HOLD;
            }
            let creature = ModuleSeat {
                x: (i32::from(v.x) - half(sin)) as i16,
                y: 0,
                z: (i32::from(v.z) - half(cos)) as i16,
                facing: h,
            };
            st.creature = Some(creature);
            cut(
                [0, yaw_from(0x800, h), 0],
                [0, 0x500, 0x400],
                focus_on(creature),
                0x80,
            )
        }
        _ => ArmDirection::PARK,
    }
}

/// PROT 0921 (Iota): a cut at arm 0 - pitch `0`, yaw `0x800`, TR
/// `(0, 0x400, 0x800)` on the stage origin (`0x801F6C38`) - arming the
/// countdown `0x801F8CB0 = scalar << 6`. While the word is positive arms 1..3
/// drain it by `scalar * delta` and pull TR z in by the same amount
/// (`0x801F6C84`); arm 1 passes on its stream request regardless, arm 2 holds
/// until it is spent and re-arms `scalar << 6` past the CD poll, and arm 3
/// holds the same way, then seats the creature on the origin and cuts to
/// pitch `0`, yaw `0x800`, TR `(0, 0x400, 0x4E20)`, re-arming `scalar *
/// 0x118` (`0x801F6FAC..0x801F7014`). Arm 4 is a `0x118`-frame push in to
/// pitch `-0x118`, TR `(0, 0x400, 0x200)` that drains the word once
/// (`0x801F7018..0x801F706C`). Parks at arm 5.
///
/// PORT: overlay_summon_iota_0921_801f6a08 (PROT 0921; camera arms 0, 3 and 4 and the arm 1..3 pull)
pub fn iota_direct(st: &mut ModuleCamState, phase: u8, _seats: ModuleCamSeats) -> ArmDirection {
    let pull = ModuleNudge {
        tr_z: -MODULE_DRAIN_PER_TICK as i16,
        ..Default::default()
    };
    // `if (cd > 0) { cd -= scalar * delta; TR z -= scalar * delta; }`
    let drift = |st: &mut ModuleCamState| -> Option<ModuleNudge> {
        (st.countdown.0 > 0).then(|| {
            st.countdown.drain();
            pull
        })
    };
    match phase {
        0 => {
            st.countdown.arm(6);
            cut([0, 0x800, 0], [0, 0x400, 0x800], [0; 3], 1)
        }
        1 => ArmDirection {
            nudge: drift(st),
            ..ArmDirection::PASS
        },
        2 => match drift(st) {
            Some(n) => ArmDirection::HOLD.nudged(n),
            None => {
                st.countdown.arm(6);
                ArmDirection::PASS
            }
        },
        3 => match drift(st) {
            Some(n) => ArmDirection::HOLD.nudged(n),
            None => {
                st.creature = Some(ModuleSeat::default());
                st.countdown.add(0x118);
                cut([0, 0x800, 0], [0, 0x400, 0x4E20], [0; 3], 1)
            }
        },
        4 => {
            st.countdown.drain();
            cut([-0x118, 0x800, 0], [0, 0x400, 0x200], [0; 3], 0x118)
        }
        _ => ArmDirection::PARK,
    }
}

/// PROT 0929 (Mule): a cut at arm 0 - pitch / yaw `0`, TR `(0, 0x500, 0x800)`
/// on the stage origin (`0x801F6BB4`) - then the stream request at arm 1 and
/// the CD poll at arm 2, which holds until the band timer `ctx[+0x6D8]` is
/// spent. While that timer runs, arms 1 and 2 also add `(scalar * delta) / 2`
/// a pass to `0x80089120`, a camera word outside the three globals a module
/// nudge models, so that drift is not carried. Parks at arm 3.
///
/// PORT: overlay_summon_mule_0929_801f69fc (PROT 0929; camera arm 0 and the arm 2 wait)
pub fn mule_direct(_st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    match phase {
        0 => cut([0; 3], [0, 0x500, 0x800], [0; 3], 1),
        1 => ArmDirection::PASS,
        2 => gate(seats.band_timer != 0),
        _ => ArmDirection::PARK,
    }
}

/// PROT 0932 (Meta): a cut at arm 0 - pitch `0`, yaw `0x800`, TR
/// `(0, 0x400, 0x800)` on the stage origin (`0x801F6AD8`) - arming the
/// countdown `0x801F8F18 = 0xC0`. Arm 1 drains it by the frame delta and,
/// every pass, swings the camera straight through the globals: pitch
/// `-4 * delta`, TR y `+12 * delta`, TR z `-12 * delta`; it passes once the
/// word falls below `0x41` (`0x801F6CF0..0x801F6D98`). Arm 2 is the stream
/// request and arm 3 the CD poll, which re-arms `0x60`. Arm 4 holds until
/// that is spent, re-arms `0x2E`, seats the creature at `(-0x96, -0x1C0)`
/// facing `0`, and cuts to pitch `-0x3C0`, yaw `0x80`, TR
/// `(-0x5C8, 0x600, -0x1000)` (`0x801F6E8C..0x801F6F54`); arm 5 holds the
/// same way, re-arms `8` and cuts to pitch `0xA0`, yaw `0xCD4`, TR
/// `(-0x200, 0x800, 0x6F4)` (`0x801F6F98..0x801F7018`). Parks at arm 6.
///
/// Arm 4 also writes five halfwords of the battle camera block at
/// `0x80083FF8` (`+0x14` / `+0x16` / `+0x18` / `+0x24` / `+0x26`), which no
/// module director models.
///
/// PORT: overlay_summon_meta_0932_801f6a34 (PROT 0932; camera arms 0, 1, 4 and 5 and their countdown)
pub fn meta_direct(st: &mut ModuleCamState, phase: u8, _seats: ModuleCamSeats) -> ArmDirection {
    match phase {
        0 => {
            st.countdown.0 = 0xC0;
            cut([0, 0x800, 0], [0, 0x400, 0x800], [0; 3], 1)
        }
        1 => {
            st.countdown.0 -= DELTA_PER_TICK;
            let swing = ModuleNudge {
                pitch: -(4 * DELTA_PER_TICK) as i16,
                tr_y: (12 * DELTA_PER_TICK) as i16,
                tr_z: -(12 * DELTA_PER_TICK) as i16,
            };
            let base = if st.countdown.0 < 0x41 {
                ArmDirection::PASS
            } else {
                ArmDirection::HOLD
            };
            base.nudged(swing)
        }
        2 => ArmDirection::PASS,
        3 => {
            st.countdown.0 = 0x60;
            ArmDirection::PASS
        }
        4 => {
            if delta_gate(st, 0x2E) {
                return ArmDirection::HOLD;
            }
            st.creature = Some(ModuleSeat {
                x: -0x96,
                y: 0,
                z: -0x1C0,
                facing: 0,
            });
            cut([-0x3C0, 0x80, 0], [-0x5C8, 0x600, -0x1000], [0; 3], 1)
        }
        5 => {
            if delta_gate(st, 8) {
                return ArmDirection::HOLD;
            }
            cut([0xA0, 0xCD4, 0], [-0x200, 0x800, 0x6F4], [0; 3], 1)
        }
        _ => ArmDirection::PARK,
    }
}

/// PROT 0933 (Terra): a cut at arm 0 - pitch `-0xA0`, yaw `0x800`, TR
/// `(0, 0x700, 0x1800)` on the stage origin (`0x801F6ACC`) - then the stream
/// request at arm 1. Arm 2, past the CD poll, arms the countdown
/// `0x801F93BC = 0x100` and rises to pitch `0xA0`, TR `(0, 0x700, 0)` over
/// `0x140` frames (`0x801F6D50..0x801F6DEC`). Arm 3 holds on the countdown
/// (drained by the frame delta) and re-arms `0x20`; arm 4 holds the same way,
/// re-arms `0x60` and cuts to pitch `0`, yaw `0x800`, TR `(0, 0x700, 0)`
/// (`0x801F6ED4..0x801F6FD0`). Parks at arm 5.
///
/// PORT: overlay_summon_terra_0933_801f6a30 (PROT 0933; camera arms 0, 2 and 4 and the arm 3/4 countdown)
pub fn terra_direct(st: &mut ModuleCamState, phase: u8, _seats: ModuleCamSeats) -> ArmDirection {
    match phase {
        0 => cut([-0xA0, 0x800, 0], [0, 0x700, 0x1800], [0; 3], 1),
        1 => ArmDirection::PASS,
        2 => {
            st.countdown.0 = 0x100;
            cut([0xA0, 0x800, 0], [0, 0x700, 0], [0; 3], 0x140)
        }
        3 => gate(delta_gate(st, 0x20)),
        4 => {
            if delta_gate(st, 0x60) {
                return ArmDirection::HOLD;
            }
            cut([0, 0x800, 0], [0, 0x700, 0], [0; 3], 1)
        }
        _ => ArmDirection::PARK,
    }
}

/// PROT 0934 (Ozma): a cut at arm 0 - pitch `0x180`, yaw `0x700`, TR
/// `(0, 0, 0x1000)`, focus `(0, 0, -0x400)` (`0x801F6AD8`) - and at arm 1 a
/// `0x80`-frame swing to yaw `0x6E8`, TR `(0, 0, 0x1800)` on the same point
/// (`0x801F6C94..0x801F6CC0`). Arm 2 is the stream request; it skips arm 3
/// when the band timer is already spent, and arm 3 otherwise waits for it -
/// folded here into arm 3's gate, one tick later. Arm 4 is the CD poll and
/// the creature's part spawns. Parks at arm 5.
///
/// PORT: overlay_summon_ozma_0934_801f6a40 (PROT 0934; camera arms 0 and 1 and the arm 3 wait)
pub fn ozma_direct(_st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    match phase {
        0 => cut([0x180, 0x700, 0], [0, 0, 0x1000], [0, 0, 0x400], 1),
        1 => cut([0x180, 0x6E8, 0], [0, 0, 0x1800], [0, 0, 0x400], 0x80),
        2 => ArmDirection::PASS,
        3 => gate(seats.band_timer != 0),
        4 => ArmDirection::PASS,
        _ => ArmDirection::PARK,
    }
}
