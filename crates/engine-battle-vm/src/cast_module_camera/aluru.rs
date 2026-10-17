//! PROT 0916 (Aluru, "Final Blast"): the whole phase chain of
//! `FUN_801F69F8` as a director ([`ModuleProfile::director_body`]).
//!
//! The module's countdown is the band's own word `ctx[+0x6D8]`, armed in
//! multiples of the speed scalar and drained by `scalar * delta` under the
//! `bgez` gate (hold while the word is non-negative). The director keeps it
//! in [`ModuleCamState::countdown`].
//!
//! The arm-0 kill roll (`0x801F6D44..0x801F6E8C`) is the host's
//! ([`ModuleCamState::kill`]): `rand() % (0x11 - level) < 8` kills, and a
//! scripted fight against a record with `+0x20` set never rolls - it takes
//! the damage path unless the victim carries `+0x16E` bit `4`. Word
//! `0x801F9010` holds the verdict (`0` kills); arm `0x11` reads it to zero
//! the victim's HP, arm `0x13` to break the killed victim up and leave for
//! `0xFF`, or to hand a damaged one to arm `0x14`.
//!
//! The arms' fades are the presentation hook's (`World::fire_cast_presentation`),
//! fired off the image's decoded call sites on the arm timing this director
//! paces.
//!
//! See `ghidra/scripts/funcs/overlay_summon_aluru_0916_801f69f8.txt`.

use super::body_paced::set_yaw;
use super::*;

/// The PROT entry of Aluru's module (`summon_aluru`).
pub const ALURU_ENTRY: u32 = 916;

/// The arm that draws the bands once at rest and, on the kill path, zeroes
/// the victim's HP (`0x801F7CB0`; the kill writes `0x801F7DCC..0x801F7E70`).
pub const ALURU_BLAST_ARM: u8 = 0x11;

/// The arm whose exit lands the damage path's hit (`jal 0x801DD0AC` at
/// `0x801F85A8`, on the pass the band word passes `scalar * 32`).
pub const ALURU_HIT_ARM: u8 = 0x12;

/// The arm that grows the bands to `scalar * 192` and, on the kill path,
/// breaks the victim up (`0x801F8184..0x801F81BC`).
pub const ALURU_SPREAD_ARM: u8 = 0x13;

/// The damage path's settle arm (`0x801F87D4`).
pub const ALURU_SETTLE_ARM: u8 = 0x14;

/// The band word's arm-`0x12` exit threshold, in scalars (`sll v0,v0,0x5`
/// at `0x801F8558`).
pub const ALURU_HIT_SPREAD: i32 = 32;

/// The band word's arm-`0x13` exit threshold, in scalars (`* 3 << 6` at
/// `0x801F869C..0x801F86A8`).
pub const ALURU_SPREAD_END: i32 = 192;

/// The banner element the kill path raises as it leaves arm `0x13`
/// (`FUN_801D8DE8(0x5B, 0)` at `0x801F87BC`, also stored to `ctx[+0x18]`).
pub const ALURU_KILL_BANNER: u8 = 0x5B;

/// Arm 3's two records (`0x801F7000`, `0x801F7018`), at the arm's point.
const SPAWNS_ARM_3: [ModuleSpawn; 2] = [
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8B3C),
        anchor: SpawnAnchor::ArmPoint,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8B88),
        anchor: SpawnAnchor::ArmPoint,
    },
];

/// Arm 4's four records (`0x801F71DC..0x801F7224`), off the shot trios.
const SPAWNS_ARM_4: [ModuleSpawn; 4] = [
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8F4C),
        anchor: SpawnAnchor::ShotFocus,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8F74),
        anchor: SpawnAnchor::ShotFocus,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8F9C),
        anchor: SpawnAnchor::ShotFocus,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8FC4),
        anchor: SpawnAnchor::ShotFocus,
    },
];

/// Arm 9's record (`0x801F76F8`), ahead of the creature.
const SPAWNS_ARM_9: [ModuleSpawn; 1] = [ModuleSpawn {
    record: SpawnRecord::Module(0x801F_8C6C),
    anchor: SpawnAnchor::ArmPoint,
}];

/// Arm 14's three records (`0x801F7AA0..0x801F7AE4`), at the same point.
const SPAWNS_ARM_14: [ModuleSpawn; 3] = [
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8E58),
        anchor: SpawnAnchor::ArmPoint,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8EA8),
        anchor: SpawnAnchor::ArmPoint,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8F0C),
        anchor: SpawnAnchor::ArmPoint,
    },
];

/// The point arms 9 and 14 seat their records at: `0x1CC` units ahead of
/// the creature along its facing `f`, at `y = -0x186`, turned to `f`
/// (`0x801F7628..0x801F76F4`). The sines are read at `0xFFF - f` and the
/// products rounded toward zero (`bgez; addu 0xFFF; sra 0xC`).
pub fn aluru_front_point(creature: ModuleSeat) -> ([i16; 3], [i16; 3]) {
    let f = creature.facing & 0xFFF;
    let (sin, cos) = trig12(0xFFF - f);
    let reach = |t: i16| (i32::from(t) * 460) / 4096;
    (
        [
            (i32::from(creature.x) - reach(sin)) as i16,
            -0x186,
            (i32::from(creature.z) + reach(cos)) as i16,
        ],
        [0, creature.facing as i16, 0],
    )
}

fn shot(angles: [i16; 2], tr: [i16; 3], focus: [i16; 3], frames: i32) -> ArmDirection {
    ArmDirection::shot(ModuleShot {
        angles: [angles[0], angles[1], 0],
        tr,
        focus,
        frames: frames as u16,
    })
}

/// PROT 0916's phase chain. `h` is the heading from the victim to the
/// caster turned half round; the creature stands half a unit from the
/// victim along it, facing it, from arm 4 on.
///
/// * `0` - a cut: pitch `-0x80`, yaw `0x800 - h`, TR `(0, 0x400, 0x800)`
///   on the creature's spot (`0x801F6C00..0x801F6CCC`).
/// * `1` - the stream request; skips arm 2 when the band timer is spent.
/// * `2` - waits for the band timer.
/// * `3` - the CD poll: two records at the creature's spot, the band word
///   `= scalar * 20`.
/// * `4` - once the word is spent, seats the creature (colour word `0x800`),
///   frames it (pitch `0`, yaw `0x800 - h`, TR `(0, 0x500, 0x400)`, `0x80`
///   frames) and spawns four records off the shot.
/// * `5` - yaw `+= 2 p` a pass while the creature's colour word drains by
///   `p` to `0x400`; then the yaw is stored `-f` and the camera pans to
///   pitch `0x140`, TR `(0, 0x600, 0x800)` over `0x80` frames.
/// * `6` - the same drift and drain to `0`; then yaw `= 0x800 - f`, the
///   spell name goes up and the word `= scalar * 180`.
/// * `7` - once spent, the attack name and a cut: pitch `0`, yaw
///   `0x900 - f`, TR `(0, 0x800, 0x300)`.
/// * `8` - a `scalar << 4`-frame tilt to pitch `-0x80`; word `= scalar << 7`.
/// * `9` - once spent, a `scalar << 3`-frame pull to pitch `-0x100`, TR
///   `(0, 0x780, 0x600)`; word `= scalar << 8`; the creature's next clip is
///   staged and a record spawned ahead of it.
/// * `10` - once the word falls below `scalar * 192`, a `scalar * 24`-frame
///   tilt to pitch `0x80`, TR `(0, 0x540, 0x800)`.
/// * `11` - once spent, a cut: pitch `0x80`, yaw `-f`, TR `(0, 0x600, 0x600)`.
/// * `12` - a `scalar * 12`-frame rise to pitch `0x100`, TR
///   `(0, 0xC00, 0x200)`; word `= scalar * 96`.
/// * `13` - once spent, a cut: pitch `-0x100`, yaw `-f - 0x700`, TR
///   `(0x200, 0x800, 0x800)`.
/// * `14` - a fade (`0xE0 -> 0x80` over `0x3C`), three records ahead of the
///   creature, word `= scalar * 55`.
/// * `15` - once spent, the creature leaves, a cut (TR `(0, 0x3E8, 0)`),
///   the fade replaced (`0x80 -> 0x100`), word `= scalar * 30`.
/// * `16` - a `0x1E`-frame pan onto the victim, TR `(0, 0x3E8, 0x1F4)`.
/// * `0x11` - once spent, the blast: a cut on the victim (yaw `-f - 0x400`,
///   TR z the framing depth), the fade replaced (`0x7F -> 0xC0` over `0xC0`),
///   the word zeroed and the bands drawn once at rest; the kill path zeroes
///   the victim's HP.
/// * `0x12` / `0x13` - the word grows by `p` a pass and the bands draw at
///   `word / 24`. Arm `0x12` leaves for `0x13` once the word passes
///   `scalar * 32` (the damage path's hit, the caption cleared); past
///   `scalar * 192` the fade goes out over `0x1E` and the damage path frames
///   the victim (pitch `0x40`, yaw `-f - 0x200`, TR `(0, 0x500, 2 depth /
///   3)`, `0x10` frames) and steps on, the kill path stores `0xFF`. Arm
///   `0x13` adds `2 p` to a killed victim's colour word every pass.
/// * `0x14` - re-arms the arm `0x13` framing every pass and stores `0xFF`
///   once the victim has settled.
/// * `0xFF` - the action SM's case 8 on the caster; the module is done.
///
/// Not carried: the battle lighting block writes of arms 4 and `0x11`
/// (`0x8007BE60`), the creature's and victim's `+0x21C` / `+0x04` stores,
/// the killing of the arm-14 records in arm 15 (they run until they halt),
/// arm `0x11`'s turn of the victim onto the creature, the kill tallies in
/// the character record (`+0x5D0`, `+0x664`) and arm `0x13`'s banner
/// [`ALURU_KILL_BANNER`]. The arm `0x14` framing focuses the victim's seat
/// rather than its `+0x3C` / `+0x40` pair.
///
/// PORT: overlay_summon_aluru_0916_801f69f8 (PROT 0916; the whole phase chain's gates, camera writes, captions, spawns and the band / colour words; arm 0's kill roll is `aluru_kill_verdict`)
pub fn aluru_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let v = seats.victim;
    let h = heading(v, seats.caster).wrapping_add(0x800) & 0xFFF;
    let (sin, cos) = trig12(h);
    let p = MODULE_DRAIN_PER_TICK;
    let s = SPEED_SCALAR;
    let creature = st.creature.unwrap_or_default();
    let f = i32::from(creature.facing);
    let on_creature = focus_on(creature);
    let on_victim = focus_on(v);
    let kill = st.kill == Some(true);
    st.band_spread = None;
    st.victim_shatter_step = 0;
    let yaw_drift = || {
        ArmDirection::HOLD.drifted(CaptureDrift {
            yaw: (2 * p) as i16,
            ..Default::default()
        })
    };
    match phase {
        0 => shot(
            [-0x80, yaw_from(0x800, h)],
            [0, 0x400, 0x800],
            [
                (half(sin) - i32::from(v.x)) as i16,
                0,
                (half(cos) - i32::from(v.z)) as i16,
            ],
            1,
        ),
        1 if seats.band_timer == 0 => ArmDirection {
            jump: Some(3),
            ..ArmDirection::PASS
        },
        1 => ArmDirection::PASS,
        2 => gate(seats.band_timer != 0),
        3 => {
            st.countdown.0 = s * 20;
            ArmDirection {
                spawns: &SPAWNS_ARM_3,
                arm_point: Some((
                    [
                        (i32::from(v.x) - half(sin)) as i16,
                        0,
                        (i32::from(v.z) - half(cos)) as i16,
                    ],
                    [0, h as i16, 0],
                )),
                ..ArmDirection::PASS
            }
        }
        4 => {
            if st.countdown.drain_non_negative() {
                return ArmDirection::HOLD;
            }
            let c = ModuleSeat {
                x: (i32::from(v.x) - half(sin)) as i16,
                y: 0,
                z: (i32::from(v.z) - half(cos)) as i16,
                facing: h,
            };
            st.creature = Some(c);
            st.creature_shatter = Some(0x800);
            ArmDirection {
                spawns: &SPAWNS_ARM_4,
                ..shot(
                    [0, yaw_from(0x800, h)],
                    [0, 0x500, 0x400],
                    focus_on(c),
                    0x80,
                )
            }
        }
        5 => {
            let w = st.creature_shatter.unwrap_or(0).wrapping_sub(p as u16);
            st.creature_shatter = Some(w);
            if w as i16 > 0x400 {
                return yaw_drift();
            }
            ArmDirection {
                drift: set_yaw(st, -f & 0xFFF).drift,
                ..shot([0x140, (-f) as i16], [0, 0x600, 0x800], on_creature, 0x80)
            }
        }
        6 => {
            let w = st.creature_shatter.unwrap_or(0).wrapping_sub(p as u16);
            st.creature_shatter = Some(w);
            if w as i16 > 0 {
                return yaw_drift();
            }
            st.creature_shatter = Some(0);
            st.countdown.0 = s * 180;
            ArmDirection {
                caption: Some(ModuleCaption::SpellName),
                ..set_yaw(st, (0x800 - f) & 0xFFF)
            }
        }
        7 => {
            if st.countdown.drain_non_negative() {
                return ArmDirection::HOLD;
            }
            ArmDirection {
                caption: Some(ModuleCaption::AttackName),
                ..shot([0, (0x900 - f) as i16], [0, 0x800, 0x300], on_creature, 1)
            }
        }
        8 => {
            st.countdown.0 = s << 7;
            shot(
                [-0x80, (0x800 - f) as i16],
                [0, 0x800, 0x300],
                on_creature,
                s << 4,
            )
        }
        9 => {
            if st.countdown.drain_non_negative() {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = s << 8;
            // `+0x1DA += 1` over the installer's idle clip `0`.
            st.creature_stage = Some(1);
            ArmDirection {
                spawns: &SPAWNS_ARM_9,
                arm_point: Some(aluru_front_point(creature)),
                ..shot(
                    [-0x100, (0x800 - f) as i16],
                    [0, 0x780, 0x600],
                    on_creature,
                    s << 3,
                )
            }
        }
        10 => {
            st.countdown.drain();
            if st.countdown.0 >= s * ALURU_SPREAD_END {
                return ArmDirection::HOLD;
            }
            shot(
                [0x80, (0x800 - f) as i16],
                [0, 0x540, 0x800],
                on_creature,
                s * 24,
            )
        }
        11 => {
            if st.countdown.drain_non_negative() {
                return ArmDirection::HOLD;
            }
            shot([0x80, (-f) as i16], [0, 0x600, 0x600], on_creature, 1)
        }
        12 => {
            st.countdown.0 = s * 96;
            shot([0x100, (-f) as i16], [0, 0xC00, 0x200], on_creature, s * 12)
        }
        13 => {
            if st.countdown.drain_non_negative() {
                return ArmDirection::HOLD;
            }
            shot(
                [-0x100, (-f - 0x700) as i16],
                [0x200, 0x800, 0x800],
                on_creature,
                1,
            )
        }
        14 => {
            st.countdown.0 = s * 55;
            ArmDirection {
                spawns: &SPAWNS_ARM_14,
                arm_point: Some(aluru_front_point(creature)),
                ..ArmDirection::PASS
            }
        }
        15 => {
            if st.countdown.drain_non_negative() {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = s * 30;
            shot([0, (-f) as i16], [0, 0x3E8, 0], on_creature, 1)
        }
        16 => shot([0, (-f) as i16], [0, 0x3E8, 0x1F4], on_victim, 0x1E),
        ALURU_BLAST_ARM => {
            if st.countdown.drain_non_negative() {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = 0;
            st.band_spread = Some(0);
            shot(
                [0, (-f - 0x400) as i16],
                [0, 0x3E8, seats.depth_raw as i16],
                on_victim,
                1,
            )
        }
        ALURU_HIT_ARM | ALURU_SPREAD_ARM => {
            if phase == ALURU_SPREAD_ARM && kill {
                st.victim_shatter_step = (2 * p) as u16;
            }
            st.countdown.0 = i32::from((st.countdown.0 + p) as i16);
            st.band_spread = Some(aluru_band_spread(st.countdown.0));
            let mut next = phase;
            let mut caption = None;
            if phase == ALURU_HIT_ARM && st.countdown.0 > s * ALURU_HIT_SPREAD {
                next = ALURU_SPREAD_ARM;
                caption = Some(ModuleCaption::Cleared);
            }
            let mut d = ArmDirection {
                caption,
                ..ArmDirection::HOLD
            };
            if st.countdown.0 > s * ALURU_SPREAD_END {
                if kill {
                    next = crate::seru_magic_ticks::SERU_B_DONE_PHASE;
                } else {
                    next += 1;
                    d.shot = Some(settle_shot(creature, v, seats.depth_raw));
                }
            }
            if next != phase {
                d.hold = false;
                d.jump = Some(next);
            }
            d
        }
        ALURU_SETTLE_ARM => {
            let mut d = ArmDirection {
                shot: Some(settle_shot(creature, v, seats.depth_raw)),
                ..gate(!st.victim_settled)
            };
            if st.victim_settled {
                d.jump = Some(crate::seru_magic_ticks::SERU_B_DONE_PHASE);
            }
            d
        }
        0xFF => ArmDirection {
            end_frame: true,
            ..ArmDirection::HOLD
        },
        _ => ArmDirection::HOLD,
    }
}

/// The damage path's framing of arms `0x13` and `0x14`: pitch `0x40`, yaw
/// `-f - 0x200`, TR `(0, 0x500, 2 depth / 3)` on the victim, over `0x10`
/// frames (`0x801F873C..0x801F87A8`, `0x801F87D4..0x801F8840`).
fn settle_shot(creature: ModuleSeat, victim: ModuleSeat, depth_raw: i32) -> ModuleShot {
    let f = i32::from(creature.facing);
    // `mult (2 * depth), 0x55555556; mfhi; - sign` - a third rounded toward
    // zero.
    let z = (2 * i32::from(depth_raw as i16)) / 3;
    ModuleShot {
        angles: [0x40, (-f - 0x200) as i16, 0],
        tr: [0, 0x500, z as i16],
        focus: focus_on(victim),
        frames: 0x10,
    }
}

/// The band spread arms `0x12` / `0x13` draw at for a band word: the signed
/// halfword over `24`, rounded toward zero (`mult 0x2AAAAAAB`, `sra 2`, sign
/// fix-up at `0x801F825C..0x801F82C0`).
pub fn aluru_band_spread(word: i32) -> i16 {
    (word as i16) / 24
}

/// The modulus base of the arm-0 kill roll: `rand() % (0x11 - level)`
/// (`li v1,0x11; subu v1,v1,a0` at `0x801F6DE4`).
pub const ALURU_KILL_BASE: u32 = 0x11;

/// The roll's kill threshold (`slti v1,v1,0x8` at `0x801F6E1C`): a remainder
/// below it kills.
pub const ALURU_KILL_THRESHOLD: u32 = 8;

/// PROT 0916's arm-0 verdict: `true` kills. `roll` is the `FUN_80056798`
/// draw, `None` where the arm skips it - a scripted fight
/// (`ctx[+0x287]`) against a record with `+0x20` set and a victim without
/// `+0x16E` bit `4` (`0x801F6D44..0x801F6D8C`), which never kills. The
/// divide is signed in retail and the draw non-negative, so an unsigned
/// remainder is the same value; the divisor is clamped to `1` where a level
/// of `0x11` would trap.
pub fn aluru_kill_verdict(roll: Option<u32>, level: u8) -> bool {
    roll.is_some_and(|r| {
        r % ALURU_KILL_BASE.saturating_sub(u32::from(level)).max(1) < ALURU_KILL_THRESHOLD
    })
}
