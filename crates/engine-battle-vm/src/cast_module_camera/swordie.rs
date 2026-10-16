//! PROT 0910 (Swordie)'s director, beside its ported tick body
//! (`crate::seru_magic_ticks::swordie_tick`).

use super::body_paced::*;
use super::*;

const fn swordie_spawn(va: u32, anchor: SpawnAnchor) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(va),
        anchor,
    }
}

const fn hand(side: i16, extra: i16, y: i16) -> SpawnAnchor {
    SpawnAnchor::CreatureFrame {
        div: 12,
        side,
        extra,
        y,
        yaw: 0x400,
        roll: 0,
    }
}

/// PROT 0910 arm 5's five spawns on its pass (`jal 0x80021B04` at
/// `0x801F7298`, `73F4`, `7478`, `75A0`, `7624`): the ring emitter
/// `0x801F8A80` a sixty-fourth of a unit behind the creature, whose
/// op-`0x20` loop calls stager arms `0` and `1` fifteen times, then for
/// each hand a glow `0x801F8B44` at `y = -0xE0` and the ray emitter
/// `0x801F8B8C` (stager arms `2` / `3`, eleven times) at `y = -0x118`.
/// The retail capture of the cast holds the rings at `(29, 0, -2370)` and
/// the hands at `x = -78` / `126`, `z = -2646` round the creature's
/// `(30, 0, -2307)`.
pub const SWORDIE_RING_SPAWNS: [ModuleSpawn; 5] = [
    swordie_spawn(
        0x801F_8A80,
        SpawnAnchor::CreatureFrame {
            div: 64,
            side: 0,
            extra: 0,
            y: 0,
            yaw: 0,
            roll: 0,
        },
    ),
    swordie_spawn(0x801F_8B44, hand(40, 0, -0xE0)),
    swordie_spawn(0x801F_8B8C, hand(40, 32, -0x118)),
    swordie_spawn(0x801F_8B44, hand(-40, 0, -0xE0)),
    swordie_spawn(0x801F_8B8C, hand(-40, 32, -0x118)),
];

/// PROT 0910 arm 8's spawn on its pass, on the creature (`0x801F7898`): the
/// glow whose op-`0x20` loop calls stager arm `4` three times.
pub const SWORDIE_GLOW_SPAWNS: [ModuleSpawn; 1] =
    [swordie_spawn(0x801F_8CB8, SpawnAnchor::Creature)];

/// The yaw offsets PROT 0910's arm 10 adds to `-f` per landed slash, read
/// at `0x801F8D60 + hits * 2` (`0x801F7B00..0x801F7B1C`): the sway that
/// swings the camera a different way on each of the four hits.
pub const SWORDIE_SWAY_YAW: [i16; 5] = [0, 0x300, -0x100, 0x100, -0x200];

/// The fourth slash's progress word reads at least `speed << 6` once it has
/// landed (`sll v1,v1,0x6` / `sltu` at `0x801F7A24..0x801F7A30`); arm 10
/// then pulls the sway in to the victim.
pub const SWORDIE_SWAY_LANDED: i32 = SPEED_SCALAR << 6;

/// One `FUN_801D829C` call of PROT 0910 framing its own creature: focus on
/// the creature's seat, yaw `k - creature[+0x46]`.
fn on_creature(c: ModuleSeat, pitch: i16, k: i32, tr: [i16; 3], frames: u16) -> ArmDirection {
    shot([pitch, yaw_from(k, c.facing)], tr, focus_on(c), frames)
}

/// PROT 0910 (Swordie).
///
/// With `h` the heading from the victim to the caster turned half round
/// (`FUN_80019B28(victim, caster) + 0x800`): a one-frame cut at arm 0 -
/// pitch `0x40`, yaw `0x800 - h`, TR `(0, 0x300, 0xC00)`, on the point half
/// a unit from the victim along `h` (`0x801F6B38..0x801F6C08`). Arm 2, past
/// its CD poll, seats the creature on that same point facing `h`
/// (`jal 0x801F19EC`, then `+0x46 = h`, `+0x34 = victim.x - sin(h) / 2`,
/// `+0x38 = victim.z - cos(h) / 2` at `0x801F6D10..0x801F6DD4`): Swordie
/// stands between the victim and the caster, which is what the split panels
/// open onto.
///
/// The later framings, each on its arm's pass, are on the creature (`f` its
/// facing):
///
/// | Call | Arm | Pitch | Yaw | TR | Frames |
/// |---|---|---|---|---|---|
/// | `0x801F7194` | 5 | `0` | `0x900 - f` | `(0, 0x300, 0x400)` | 1 |
/// | `0x801F768C` | 6 | `-0x40` | `0xC00 - f` | `(-0x300, 0x300, 0x800)` | `0x100` |
/// | `0x801F7750` | 7 | `0x80` | `0x700 - f` | `(0, 0x400, 0x400)` | 1 |
/// | `0x801F77BC` | 8 | `0x60` | `0x820 - f` | `(0, 0x300, 0x2400)` | `0x80` |
///
/// and arm 9's pass (`0x801F79D8`) cuts onto the victim's body pair -
/// pitch `0x40`, yaw `-f`, TR `(0, 0x500, ctx[+0x6D0] / 2)`. Arm 4's pass
/// puts up the spell name (`0x801F70E0`) and arm 5's the attack name
/// (`0x801F71EC`).
///
/// Arm 10 re-arms a 12-frame sway on every pass (`0x801F7A20..0x801F7B20`):
/// yaw `-f` plus [`SWORDIE_SWAY_YAW`]`[hits]`, on the victim's body pair,
/// pitch `0x40` and TR `(0, 0x500, d / 2)` while the fourth slash is still
/// travelling, pitch `0x100` and TR `(0, 0x300, d)` once it has landed
/// ([`SWORDIE_SWAY_LANDED`]). The host feeds the two module words through
/// [`ModuleCamState::module_words`] (`[hits, fourth slash progress]`).
///
/// PORT: overlay_summon_swordie_0910_801f69ec (PROT 0910; camera arms 0, 5..10, the arm 2 creature placement, the arm 4/5 captions, the settle arm's caption clear and the spawn calls of arms 5 and 8)
pub fn swordie_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let v = seats.victim;
    let h = heading(v, seats.caster).wrapping_add(0x800) & 0xFFF;
    let (sin, cos) = trig12(h);
    let point = ModuleSeat {
        x: (i32::from(v.x) - half(sin)) as i16,
        y: 0,
        z: (i32::from(v.z) - half(cos)) as i16,
        facing: h,
    };
    let d = seats.depth_raw as i16;
    run_body_paced(
        st,
        phase,
        BodyPacedArms {
            open: |_| {
                shot(
                    [0x40, yaw_from(0x800, h)],
                    [0, 0x300, 0xC00],
                    focus_on(point),
                    1,
                )
            },
            pass: |st, arm| {
                let c = st.creature_live.or(st.creature).unwrap_or(point);
                match arm {
                    4 => captioned(ArmDirection::PASS, ModuleCaption::SpellName),
                    5 => ArmDirection {
                        spawns: &SWORDIE_RING_SPAWNS,
                        ..captioned(
                            on_creature(c, 0, 0x900, [0, 0x300, 0x400], 1),
                            ModuleCaption::AttackName,
                        )
                    },
                    6 => on_creature(c, -0x40, 0xC00, [-0x300, 0x300, 0x800], 0x100),
                    7 => on_creature(c, 0x80, 0x700, [0, 0x400, 0x400], 1),
                    8 => ArmDirection {
                        spawns: &SWORDIE_GLOW_SPAWNS,
                        ..on_creature(c, 0x60, 0x820, [0, 0x300, 0x2400], 0x80)
                    },
                    9 => shot(
                        [0x40, yaw_from(0, c.facing)],
                        [0, 0x500, d / 2],
                        focus_on(v),
                        1,
                    ),
                    // The settle arm's pass, ahead of the `0xFF` latch
                    // (`0x801F7BC4`).
                    SWORDIE_SETTLE_PHASE => captioned(ArmDirection::PASS, ModuleCaption::Cleared),
                    _ => ArmDirection::PASS,
                }
            },
            each: |st, arm, _| match arm {
                2 => {
                    st.creature = Some(point);
                    ArmDirection::PASS
                }
                SWORDIE_SETTLE_PHASE => {
                    let c = st.creature_live.or(st.creature).unwrap_or(point);
                    let [hits, fourth] = st.module_words;
                    let sway = SWORDIE_SWAY_YAW[hits.clamp(0, 4) as usize];
                    let yaw = (i32::from(yaw_from(0, c.facing)) + i32::from(sway)) & 0xFFF;
                    let (pitch, tr) = if fourth >= SWORDIE_SWAY_LANDED {
                        (0x100, [0, 0x300, d])
                    } else {
                        (0x40, [0, 0x500, d / 2])
                    };
                    shot([pitch, yaw as i16], tr, focus_on(v), 0xC)
                }
                _ => ArmDirection::PASS,
            },
        },
    )
}

use crate::seru_magic_ticks::SWORDIE_SETTLE_PHASE;

#[cfg(test)]
mod tests {
    use super::*;

    /// Arm 2 seats Swordie half a unit from the victim toward the caster,
    /// and each later framing lands once, on the next arm's first tick.
    #[test]
    fn swordie_seats_between_victim_and_caster_and_frames_it_once() {
        let seats = ModuleCamSeats {
            caster: ModuleSeat {
                z: -2000,
                ..Default::default()
            },
            victim: ModuleSeat {
                z: 1000,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut st = ModuleCamState::default();
        swordie_direct(&mut st, 2, seats);
        let c = st.creature.expect("arm 2 seats the creature");
        assert_eq!(c.x, 0);
        assert!(c.z < 1000 && c.z > -2000, "between the two: {}", c.z);
        swordie_direct(&mut st, 5, seats);
        assert!(swordie_direct(&mut st, 6, seats).shot.is_some());
        assert!(swordie_direct(&mut st, 6, seats).shot.is_none());
        assert_eq!(
            swordie_direct(&mut st, 7, seats).shot.map(|s| s.frames),
            Some(0x100)
        );
    }

    /// Arm 10 sways by the hit count and closes in once the fourth lands.
    #[test]
    fn swordie_sway_follows_the_hits() {
        let seats = ModuleCamSeats {
            depth_raw: 0x800,
            ..Default::default()
        };
        let mut st = ModuleCamState {
            creature: Some(ModuleSeat::default()),
            module_words: [1, 0],
            ..Default::default()
        };
        let s = swordie_direct(&mut st, 10, seats).shot.expect("sway");
        assert_eq!(
            (s.angles, s.tr, s.frames),
            ([0x40, 0x300, 0], [0, 0x500, 0x400], 0xC)
        );
        st.module_words = [4, SWORDIE_SWAY_LANDED];
        let s = swordie_direct(&mut st, 10, seats).shot.expect("sway");
        assert_eq!((s.angles, s.tr), ([0x100, 0xE00, 0], [0, 0x300, 0x800]));
    }

    /// Arm 5's points land where the retail capture holds them round the
    /// creature at `(30, 0, -2307)` facing `13`: the ring at
    /// `(29, 0, -2370)`, the first hand's glow at `(-78, -224, -2646)` and
    /// its ray emitter `(-80, -280, -2773)`, the second hand mirrored.
    #[test]
    fn swordie_arm5_points_match_the_capture() {
        let c = ModuleSeat {
            x: 30,
            y: 0,
            z: -2307,
            facing: 13,
        };
        let at = |i: usize| {
            spawn_anchor_point(SWORDIE_RING_SPAWNS[i].anchor, Some(c), None, None)
                .unwrap()
                .0
        };
        assert_eq!(at(0), [29, 0, -2370]);
        assert_eq!(at(1), [-78, -0xE0, -2646]);
        assert_eq!(at(2), [-80, -0x118, -2773]);
        assert_eq!(at(3)[1], -0xE0);
        assert!((at(3)[0] - 126).abs() <= 1 && (at(3)[2] + 2650).abs() <= 1);
    }
}
