//! The player Seru-magic modules' directors (PROT 0903..0913).

use super::*;

// ---------------------------------------------------------------------------
// PROT 0903 - Gimard
// ---------------------------------------------------------------------------

/// PROT 0903's countdown word.
pub const GIMARD_COUNTDOWN_VA: u32 = 0x801F_7960;
/// The eye depth `ctx[+0x6D0]` PROT 0903's arm 10 stores for the walk-in's
/// case-6 framing.
pub const GIMARD_WALK_DEPTH: i32 = 0x800;
/// The yaw base `ctx[+0x6DA]` PROT 0903's arm 10 stores.
pub const GIMARD_WALK_YAW_BASE: i32 = 0x200;
/// The phase arm PROT 0903's creature walks in: clip 1, staged in arm 10,
/// played until the range poll clears.
pub const GIMARD_WALK_ARM: u8 = 11;

/// PROT 0903 arm 3's three spawns, on the creature it has just seated
/// (`a0 = creature + 0x34`, `a1 = creature + 0x44`; `jal 0x80021B04` at
/// `0x801F6E04` / `0x801F6E1C` / `0x801F6E34`).
pub const GIMARD_SEAT_SPAWNS: [ModuleSpawn; 3] = [
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_7820),
        anchor: SpawnAnchor::Creature,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_7870),
        anchor: SpawnAnchor::Creature,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_78C4),
        anchor: SpawnAnchor::Creature,
    },
];

/// PROT 0903 arm 6's three spawns, on the shot it has just armed
/// (`a0 = sp+0x30`, `a1 = sp+0x20`; `jal 0x80021B04` at `0x801F716C` /
/// `0x801F7184` / `0x801F719C`): the camera-relative fire tunnel the
/// creature's attack plays inside.
pub const GIMARD_TUNNEL_SPAWNS: [ModuleSpawn; 3] = [
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_7724),
        anchor: SpawnAnchor::ShotFocus,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_7794),
        anchor: SpawnAnchor::ShotFocus,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_7804),
        anchor: SpawnAnchor::ShotFocus,
    },
];

/// PROT 0903 arm 8's spawn: the battle overlay's effect prototype the pointer
/// word `0x801F63A8` names (`lw a2,0x63A8(0x801F)` at `0x801F72B4`), on the
/// creature - the breath.
pub const GIMARD_BREATH_SPAWNS: [ModuleSpawn; 1] = [ModuleSpawn {
    record: SpawnRecord::BattleProto(0x801F_63A8),
    anchor: SpawnAnchor::Creature,
}];

/// PROT 0903 arm 8's `MoveImage`: the 16x1 CLUT at `(0xD0, 0x1DC)` onto
/// `(0xE0, 0x1DC)` (`0x801F7270..0x801F72A0`), the palette the breath draws
/// with.
pub const GIMARD_BREATH_CLUT_MOVE: ModuleVramMove = ModuleVramMove {
    src: (0xD0, 0x1DC),
    size: (0x10, 1),
    dst: (0xE0, 0x1DC),
};

/// PROT 0903 (Gimard) - the camera and countdown half of the tick
/// `0x801F69D8`, one call per tick before the phase-chain body
/// ([`crate::cast_seru_ticks_a::gimard_tick`]) runs.
///
/// `b` below is `FUN_80019B28(victim, caster)`, the heading from the victim
/// to the caster, and `h = (b + 0x800) & 0xFFF` its reverse:
///
/// | arm | gate | camera / state |
/// |---:|---|---|
/// | 0 | - | snap: pitch `0x400`, yaw `-(b + 0x800)`, TR `(0, -0x40, 0x2000)`, focus origin (`0x801F6B7C`) |
/// | 1 | - | pitch `0x380`, yaw `0x800 - h`, TR `(0, 0x40, 0x400)`, focus the point half a unit past the victim along `h`, over `0x40` frames (`0x801F6C6C`); countdown `= scalar << 6` |
/// | 2 | drain while positive, hold above `0` | the creature stream load (`FUN_8003EAE4(0, 7)`), taken as ready |
/// | 3 | - | seats the creature half a unit from the victim toward the caster, facing `h`, `y = 0x200`; spawns [`GIMARD_SEAT_SPAWNS`] on it; countdown `= scalar << 6` |
/// | 4 | creature sinks; hold above `scalar << 5` | snap: pitch `-0x1C0`, yaw `0x700 - facing`, TR `(0, 0x380, 0x1E0)`, focus the creature (`0x801F6F18`) |
/// | 5 | creature sinks; hold above `0` | creature lands (`y = 0`), the spell-name caption; countdown `+= scalar * 180` |
/// | 6 | hold above `0` | snap: pitch `0x80`, yaw `0x880 - facing`, TR `(0, 0x400, 0x400)`, focus the creature (`0x801F712C`); spawns the fire tunnel [`GIMARD_TUNNEL_SPAWNS`]; the attack-name caption replaces the spell name; countdown `+= scalar * 192` |
/// | 7 | one drain | pitch `0x140`, yaw `0x940 - facing`, TR `(0, 0x340, 0xA00)`, focus the creature, over `0xC0` frames (`0x801F721C`) |
/// | 8 | hold above `scalar << 7` | the CLUT move [`GIMARD_BREATH_CLUT_MOVE`], then the breath [`GIMARD_BREATH_SPAWNS`] on the creature |
/// | 9 | hold above `scalar * 96` | - |
/// | 10 | hold above `0` | - |
/// | 12 | drain while positive, hold above `0` | case 8 on the caster every pass (`jal 0x801D5854` with `(ctx[+0x13], 8)` at `0x801F761C`); the settle poll is the body's |
///
/// Arm 10 also stores the walk-in's framing context: depth `ctx[+0x6D0] =
/// 0x800`, yaw base `ctx[+0x6DA] = 0x200`. Arm 11 is the walk-in: every pass
/// it turns the creature onto the victim, swings the yaw base by
/// `6 * scalar * delta`, frames the creature through `FUN_801D5854(7, 6)` -
/// the action SM's own case 6 on the creature seat ([`ModuleFollow`]) - and
/// holds on the range poll `FUN_8004E2F0(7, victim)`; once in range it
/// re-arms the countdown by `scalar * 192` and lands the hit.
///
/// Arm 2's CD poll (`0x8007BDB0 == 7`) and arm 3's `FUN_8003F2B8(1)` are the
/// streamed creature record's load; the engine has it resident, so both read
/// as ready.
///
/// PORT: FUN_801F69D8 (PROT 0903; the camera arms 0/1/4/6/7, the countdown gates of arms 2/4/5/6/7/8/9/10, arm 3's creature placement and the spawn / MoveImage calls of arms 3/6/8)
pub fn gimard_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let b = heading(seats.victim, seats.caster);
    let h = (b.wrapping_add(0x800)) & 0xFFF;
    let creature = st.creature.unwrap_or(seats.victim);
    match phase {
        0 => ArmDirection::shot(ModuleShot {
            angles: [0x400, yaw_from(-0x800, b), 0],
            tr: [0, -0x40, 0x2000],
            focus: [0; 3],
            frames: 1,
        }),
        1 => {
            let (sin, cos) = trig12(h);
            st.countdown.arm(6);
            ArmDirection::shot(ModuleShot {
                angles: [0x380, yaw_from(0x800, h), 0],
                tr: [0, 0x40, 0x400],
                focus: [
                    (half(sin) - i32::from(seats.victim.x)) as i16,
                    0,
                    (half(cos) - i32::from(seats.victim.z)) as i16,
                ],
                frames: 0x40,
            })
        }
        2 => {
            if st.countdown.0 > 0 {
                st.countdown.drain();
            }
            if st.countdown.0 > 0 {
                ArmDirection::HOLD
            } else {
                ArmDirection::PASS
            }
        }
        3 => {
            let (sin, cos) = trig12(h);
            st.creature = Some(ModuleSeat {
                x: (i32::from(seats.victim.x) - half(sin)) as i16,
                y: 0x200,
                z: (i32::from(seats.victim.z) - half(cos)) as i16,
                facing: h,
            });
            st.countdown.arm(6);
            ArmDirection {
                spawns: &GIMARD_SEAT_SPAWNS,
                ..ArmDirection::PASS
            }
        }
        4 => {
            if let Some(c) = st.creature.as_mut() {
                c.y = c.y.wrapping_sub(MODULE_DRAIN_PER_TICK as i16);
            }
            if st.countdown.drain_above(SPEED_SCALAR << 5) {
                return ArmDirection::HOLD;
            }
            ArmDirection::shot(ModuleShot {
                angles: [-0x1C0, yaw_from(0x700, creature.facing), 0],
                tr: [0, 0x380, 0x1E0],
                focus: focus_on(creature),
                frames: 1,
            })
        }
        5 => {
            if let Some(c) = st.creature.as_mut() {
                c.y = c.y.wrapping_sub(MODULE_DRAIN_PER_TICK as i16);
            }
            if st.countdown.drain_above(0) {
                return ArmDirection::HOLD;
            }
            if let Some(c) = st.creature.as_mut() {
                c.y = 0;
            }
            st.countdown.add(180);
            // `FUN_8003541C(0, 0, name, 0xAC - w / 2, 0x96, ..)` at
            // `0x801F700C`, the name off `DAT_800754C8[id * 12 + 8]`.
            ArmDirection {
                caption: Some(ModuleCaption::SpellName),
                ..ArmDirection::PASS
            }
        }
        6 => {
            if st.countdown.drain_above(0) {
                return ArmDirection::HOLD;
            }
            st.countdown.add(192);
            // `FUN_800319A8(0)` then `FUN_8003541C(0, 0, attack, 0xA0 - w /
            // 2, 0x96, ..)` (`0x801F707C..0x801F70CC`).
            ArmDirection {
                spawns: &GIMARD_TUNNEL_SPAWNS,
                caption: Some(ModuleCaption::AttackName),
                ..ArmDirection::shot(ModuleShot {
                    angles: [0x80, yaw_from(0x880, creature.facing), 0],
                    tr: [0, 0x400, 0x400],
                    focus: focus_on(creature),
                    frames: 1,
                })
            }
        }
        7 => {
            st.countdown.drain();
            ArmDirection::shot(ModuleShot {
                angles: [0x140, yaw_from(0x940, creature.facing), 0],
                tr: [0, 0x340, 0xA00],
                focus: focus_on(creature),
                frames: 0xC0,
            })
        }
        8 => {
            if st.countdown.drain_above(SPEED_SCALAR << 7) {
                return ArmDirection::HOLD;
            }
            ArmDirection {
                spawns: &GIMARD_BREATH_SPAWNS,
                vram_move: Some(GIMARD_BREATH_CLUT_MOVE),
                ..ArmDirection::PASS
            }
        }
        9 => gate(st.countdown.drain_above(SPEED_SCALAR * 96)),
        10 => {
            if st.countdown.drain_above(0) {
                return ArmDirection::HOLD;
            }
            // `sh 0x800, 0x6D0(ctx)` / `sh 0x200, 0x6DA(ctx)`
            // (`0x801F737C..0x801F738C`).
            st.yaw_base = GIMARD_WALK_YAW_BASE;
            ArmDirection::PASS
        }
        11 => {
            // The creature turns onto the victim, the yaw base swings by
            // `6 * scalar * delta` (`0x801F73C8..0x801F7400`), and case 6
            // frames the creature (`jal 0x801D5854` with `(7, 6)`), every
            // pass; the arm then holds on the range poll.
            let mut seat = st.creature_live.or(st.creature).unwrap_or(seats.victim);
            seat.facing = heading(seats.victim, seat).wrapping_add(0x800) & 0xFFF;
            st.yaw_base = (st.yaw_base + 6 * MODULE_DRAIN_PER_TICK) & 0xFFF;
            let follow = Some(ModuleFollow {
                seat,
                yaw_base: st.yaw_base,
                depth_raw: GIMARD_WALK_DEPTH,
            });
            // No creature seated (a headless host) has nothing to walk.
            if !st.creature_arrived && st.creature_live.is_some() {
                return ArmDirection {
                    hold: true,
                    follow,
                    ..ArmDirection::PASS
                };
            }
            st.countdown.add(192);
            ArmDirection {
                follow,
                ..ArmDirection::PASS
            }
        }
        // The settle arm frames the caster through the action SM's case 8
        // first, every pass (`lbu a0,0x13(s1)` / `jal 0x801D5854` with
        // `a1 = 8` at `0x801F7618..0x801F7620`), then drains the
        // `scalar * 192` arm 11 added before it polls the victim
        // (`lw a0,0x7960` / `blez` / `subu` / `bgtz` at
        // `0x801F7628..0x801F7654`). Case 8 over the breath's dead, still
        // fading victim is its dead-target arm, which zeroes the yaw ladder
        // `ctx[+0x6DA]` each pass (`sh zero,0x4(t0)` at `0x801D6B1C`) - so
        // the Done band's case 6 starts from a ladder the walk's swing no
        // longer holds (`shiny_refactor_gimard_levelup`).
        12 => ArmDirection {
            end_frame: true,
            ..gate(st.countdown.0 > 0 && st.countdown.drain_above(0))
        },
        _ => ArmDirection::PASS,
    }
}

// ---------------------------------------------------------------------------
// PROT 0905 - Vera
// ---------------------------------------------------------------------------

/// PROT 0905's countdown word.
pub const VERA_COUNTDOWN_VA: u32 = 0x801F_8818;

const fn module_spawns<const N: usize>(vas: [u32; N], anchor: SpawnAnchor) -> [ModuleSpawn; N] {
    let mut out = [ModuleSpawn {
        record: SpawnRecord::Module(0),
        anchor,
    }; N];
    let mut i = 0;
    while i < N {
        out[i].record = SpawnRecord::Module(vas[i]);
        i += 1;
    }
    out
}

/// PROT 0905 arm 0's four spawns, on the framed point at `y = -0x280`
/// (`jal 0x80021B04` at `0x801F6C18` / `6C3C` / `6C58` / `6C7C`). The
/// `vera_summon_mid_cast` capture holds exactly these four parts, risen to
/// `y = -418` over the target's hand.
pub const VERA_OPEN_SPAWNS: [ModuleSpawn; 4] = module_spawns(
    [0x801F_81E4, 0x801F_823C, 0x801F_8294, 0x801F_82FC],
    SpawnAnchor::ShotPoint { y: -0x280 },
);

/// PROT 0905 arm 4's three spawns, an eighth of a unit (`/ 32`) along the
/// target's heading at `y = -0x1C2` (`0x801F710C` / `7128` / `7144`).
pub const VERA_CUT_SPAWNS: [ModuleSpawn; 3] = module_spawns(
    [0x801F_8364, 0x801F_83CC, 0x801F_8434],
    SpawnAnchor::AlongVictimHeading {
        base: HeadingBase::Victim,
        div: 32,
        y: -0x1C2,
    },
);

/// PROT 0905 arm 5's two spawns, a sixth of a unit (`/ 24`) along the
/// target's heading from the creature it has just seated, at `y = -0x1C2`
/// (`0x801F7404` / `7420`).
pub const VERA_SEAT_SPAWNS: [ModuleSpawn; 2] = module_spawns(
    [0x801F_8494, 0x801F_8500],
    SpawnAnchor::AlongVictimHeading {
        base: HeadingBase::Creature,
        div: 24,
        y: -0x1C2,
    },
);

/// PROT 0905 arm 8's two spawns, on the creature (`a0 = creature + 0x34`,
/// `a1 = creature + 0x44`; `0x801F7A48` / `7A60`).
pub const VERA_CREATURE_SPAWNS: [ModuleSpawn; 2] =
    module_spawns([0x801F_85D4, 0x801F_862C], SpawnAnchor::Creature);

/// PROT 0905 arm 9's five spawns, on the restored target (`a0 = target +
/// 0x34`, `a1 = target + 0x44`; `0x801F7B34..0x801F7B94`).
pub const VERA_RESTORE_SPAWNS: [ModuleSpawn; 5] = module_spawns(
    [
        0x801F_868C,
        0x801F_86EC,
        0x801F_8730,
        0x801F_8774,
        0x801F_87D4,
    ],
    SpawnAnchor::Victim,
);
/// The phase arm PROT 0905 restores its target in.
pub const VERA_RESTORE_ARM: u8 = 9;

/// PROT 0905's arm-2 fade (`0x801F6DF4..0x801F6E5C`): once the countdown
/// has run out the arm requests the creature stream and spawns an additive
/// ramp black -> white over `0x20` vsyncs, held until killed, id `1`, and
/// keeps its actor at `ctx[+0x102C]`. The kind is the busy word the routine
/// returns (`sp+0x38`, `1` while it runs). `vera_summon_mid_cast` holds it
/// eight vsyncs in beside the band's flash-out.
pub const VERA_RISE_FADE: crate::battle_action::SummonFadeTemplate =
    crate::battle_action::SummonFadeTemplate {
        kind: 1,
        duration: 0x20,
        start_rgb: [0, 0, 0],
        end_rgb: [0xFF, 0xFF, 0xFF],
        delay: 0,
        hold: -1,
    };

/// PROT 0905's arm-4 pair (`0x801F6F30..0x801F6FC4`): the arm kills the
/// arm-2 fade and four effect actors, then spawns a warm additive flash
/// `(0xFF, 0xE0, 0x80)` -> black over `0x40` vsyncs and a blue one
/// `(0, 0x1F, 0x7F)` -> black over `0x20`, both id `1` and done when they
/// land.
pub const VERA_CUT_FADES: [(crate::battle_action::SummonFadeTemplate, i16); 2] = [
    (
        crate::battle_action::SummonFadeTemplate {
            kind: 1,
            duration: 0x40,
            start_rgb: [0xFF, 0xE0, 0x80],
            end_rgb: [0, 0, 0],
            delay: 0,
            hold: 0,
        },
        1,
    ),
    (
        crate::battle_action::SummonFadeTemplate {
            kind: 1,
            duration: 0x20,
            start_rgb: [0, 0x1F, 0x7F],
            end_rgb: [0, 0, 0],
            delay: 0,
            hold: 0,
        },
        1,
    ),
];

/// `trunc(v / d)` - the reciprocal-`mult` / `sra` / `subu sign` idiom and the
/// `bgez; addiu d-1; sra` idiom both round toward zero.
fn div0(v: i16, d: i32) -> i32 {
    i32::from(v) / d
}

/// PROT 0905 (Vera) - the camera and countdown half of the tick
/// `0x801F69D8`. The module's victim is the **ally** it restores; `f` below
/// is that seat's heading `+0x46`, and `g = (f + 0x800) & 0xFFF`.
///
/// | arm | gate | camera / state |
/// |---:|---|---|
/// | 0 | - | snap: pitch `0`, yaw `0x60C - f`, TR `(0, 0x5E0, 0x800)`, focus a 24th of a unit along `g` from the target (`0x801F6BDC`); countdown `= scalar << 6` |
/// | 1 | one drain | pitch `-0x60`, yaw `0x9F4 - f`, TR `(0, 0x580, 0x80)`, the same focus, over `0x60` frames (`0x801F6D88`) |
/// | 2 | while non-negative, drain; hold while still non-negative | the creature stream load, taken as ready; the white rise ([`VERA_RISE_FADE`]) |
/// | 3 | - | countdown `= scalar << 5` |
/// | 4 | drain, hold while non-negative | snap: pitch `0`, yaw `-f`, TR `(0, 0x600, 0x800)`, focus a 16th of a unit along `f` past the target (`0x801F7074`); kills the rise, spawns [`VERA_CUT_FADES`]; countdown `+= scalar * 60` |
/// | 5 | drain, hold while non-negative | seats the creature a 16th of a unit along `g` from the target, facing `g`; countdown `+= scalar * 180`; the phase jumps by **3**, so arms 6 and 7 are never reached |
/// | 8 | drift; drain, hold while non-negative | snap: pitch `0`, yaw `0x800 - facing`, TR `(0, 0xA00, 0x200)`, focus the creature (`0x801F7994`); countdown `+= scalar * 112` |
/// | 9 | drift; drain, hold while non-negative | the restore; countdown `+= scalar * 112` |
/// | 10 | drift; drain, hold while non-negative | the creature sinks; `0xFF` |
///
/// The drift of arms 8..10 is written straight into the camera globals every
/// pass ([`ModuleNudge`]), with `p` the per-pass product scalar * delta: arm
/// 8 raises TR y by `p / 8` and pulls TR z in by `p`; arm 9 drops TR y by
/// `3p / 2`, pushes TR z out by `3p` and tilts the pitch by `p / 8`; arm 10
/// drops TR y by `p / 8`, pushes TR z out by `4p` and tilts back by `p / 8`.
///
/// PORT: FUN_801F69D8 (PROT 0905; the camera arms 0/1/4/8, the drift of arms 8..10, the countdown gates of arms 2/4/5/8/9/10 and arm 5's creature placement)
pub fn vera_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let t = seats.victim;
    let f = t.facing & 0xFFF;
    let g = f.wrapping_add(0x800) & 0xFFF;
    let creature = st.creature.unwrap_or(t);
    let p = MODULE_DRAIN_PER_TICK;
    let near_focus = || {
        let (sin, cos) = trig12(g);
        [
            (div0(sin, 24) - i32::from(t.x)) as i16,
            0,
            (div0(cos, 24) - i32::from(t.z)) as i16,
        ]
    };
    match phase {
        0 => {
            st.countdown.arm(6);
            ArmDirection {
                spawns: &VERA_OPEN_SPAWNS,
                ..ArmDirection::shot(ModuleShot {
                    angles: [0, yaw_from(0x60C, f), 0],
                    tr: [0, 0x5E0, 0x800],
                    focus: near_focus(),
                    frames: 1,
                })
            }
        }
        1 => {
            st.countdown.drain();
            ArmDirection::shot(ModuleShot {
                angles: [-0x60, yaw_from(0x9F4, f), 0],
                tr: [0, 0x580, 0x80],
                focus: near_focus(),
                frames: 0x60,
            })
        }
        2 => {
            if st.countdown.0 >= 0 && st.countdown.drain_non_negative() {
                return ArmDirection::HOLD;
            }
            ArmDirection {
                fades: &[(VERA_RISE_FADE, 1)],
                ..ArmDirection::PASS
            }
        }
        3 => {
            st.countdown.arm(5);
            ArmDirection::PASS
        }
        4 => {
            if st.countdown.drain_non_negative() {
                return ArmDirection::HOLD;
            }
            st.countdown.add(60);
            let (sin, cos) = trig12(f);
            ArmDirection {
                spawns: &VERA_CUT_SPAWNS,
                fades: &VERA_CUT_FADES,
                kills_fades: true,
                ..ArmDirection::shot(ModuleShot {
                    angles: [0, yaw_from(0, f), 0],
                    tr: [0, 0x600, 0x800],
                    focus: [
                        (-(i32::from(t.x) + div0(sin, 16))) as i16,
                        0,
                        (-(i32::from(t.z) + div0(cos, 16))) as i16,
                    ],
                    frames: 1,
                })
            }
        }
        5 => {
            if st.countdown.drain_non_negative() {
                return ArmDirection::HOLD;
            }
            let (sin, cos) = trig12(g);
            st.creature = Some(ModuleSeat {
                x: (i32::from(t.x) - div0(sin, 16)) as i16,
                y: 0,
                z: (i32::from(t.z) - div0(cos, 16)) as i16,
                facing: g,
            });
            st.countdown.add(180);
            ArmDirection {
                spawns: &VERA_SEAT_SPAWNS,
                ..ArmDirection::PASS
            }
        }
        8 => {
            let drift = ModuleNudge {
                pitch: 0,
                tr_y: (p >> 3) as i16,
                tr_z: -p as i16,
            };
            if st.countdown.drain_non_negative() {
                return ArmDirection::HOLD.nudged(drift);
            }
            st.countdown.add(112);
            ArmDirection {
                spawns: &VERA_CREATURE_SPAWNS,
                ..ArmDirection::shot(ModuleShot {
                    angles: [0, yaw_from(0x800, creature.facing), 0],
                    tr: [0, 0xA00, 0x200],
                    focus: focus_on(creature),
                    frames: 1,
                })
            }
            .nudged(drift)
        }
        9 => {
            let drift = ModuleNudge {
                pitch: (p >> 3) as i16,
                tr_y: -((3 * p) >> 1) as i16,
                tr_z: (3 * p) as i16,
            };
            if st.countdown.drain_non_negative() {
                return ArmDirection::HOLD.nudged(drift);
            }
            st.countdown.add(112);
            ArmDirection {
                spawns: &VERA_RESTORE_SPAWNS,
                ..ArmDirection::PASS
            }
            .nudged(drift)
        }
        10 => {
            let drift = ModuleNudge {
                pitch: -(p >> 3) as i16,
                tr_y: -(p >> 3) as i16,
                tr_z: (4 * p) as i16,
            };
            if st.countdown.drain_non_negative() {
                return ArmDirection::HOLD.nudged(drift);
            }
            ArmDirection::PASS.nudged(drift)
        }
        _ => ArmDirection::PASS,
    }
}

// ---------------------------------------------------------------------------
// PROT 0908 - Zenoir
// ---------------------------------------------------------------------------

/// The phase arm PROT 0908 lands its last hit in (the finisher and the
/// splash).
pub const ZENOIR_FINISH_ARM: u8 = 11;

/// PROT 0908's grid point: half a unit from the victim along `h`, snapped to
/// the `0x200` grid and centred in its cell (`and -0x200; addiu 0x100`).
fn zenoir_grid(v: i16, trig: i16) -> i16 {
    (((i32::from(v) - half(trig)) & -0x200) + 0x100) as i16
}

/// PROT 0908 (Zenoir) - the camera and countdown half of the tick
/// `0x801F69D8`, for the arms that frame the summoning (`0..=5`). `h` is the
/// heading from the caster to the victim, and the framed point is the
/// victim's grid cell half a unit back along it ([`zenoir_grid`]):
///
/// | arm | gate | camera / state |
/// |---:|---|---|
/// | 0 | - | snap: pitch / yaw `0`, TR `(0, 0x280, 0x2000)`, focus the grid point (`0x801F6C70`) |
/// | 1 | - | TR `(0, 0x280, 0x600)`, the same focus, over `0x78` frames (`0x801F6D98`) |
/// | 2 | - | the creature stream load; skips arm 3 when the band timer `ctx[+0x6D8]` is already spent |
/// | 3 | hold until the band timer `ctx[+0x6D8]` reads `0` | - |
/// | 4 | - | seats the creature on the grid point facing `0x800`; pitch `0x3C`, TR `(0, 0x400, 0xA00)`, the same focus, over `0x78` frames (`0x801F719C`); `ctx[+0x6D8] = scalar * 140` |
/// | 5 | drain `ctx[+0x6D8]`, hold while non-negative | the spell caption |
///
/// Arm 3's wait is on the **band's** timer: the `0x78` frames the actor
/// freeze `0x34` armed and the sustain `0x35` counts down. So the module
/// frames its opening pan for exactly the band's sustain, and from arm 4 on
/// it reuses the same word as its own countdown.
///
/// Arms 6 and later - the creature's clip-paced strike, the hit framings of
/// arms 6..10 - are not directed; they run on the phase chain alone.
///
/// PORT: FUN_801F69D8 (PROT 0908; the camera arms 0/1/4, arm 3's band-timer wait, arm 4's creature placement and arm 5's gate)
pub fn zenoir_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let v = seats.victim;
    let h = heading(v, seats.caster).wrapping_add(0x800) & 0xFFF;
    let (sin, cos) = trig12(h);
    let point = ModuleSeat {
        x: zenoir_grid(v.x, sin),
        y: 0,
        z: zenoir_grid(v.z, cos),
        facing: 0x800,
    };
    match phase {
        0 => ArmDirection::shot(ModuleShot {
            angles: [0; 3],
            tr: [0, 0x280, 0x2000],
            focus: focus_on(point),
            frames: 1,
        }),
        1 => ArmDirection::shot(ModuleShot {
            angles: [0; 3],
            tr: [0, 0x280, 0x600],
            focus: focus_on(point),
            frames: 0x78,
        }),
        3 => gate(seats.band_timer != 0),
        4 => {
            st.creature = Some(point);
            st.countdown.0 = SPEED_SCALAR * 140;
            ArmDirection::shot(ModuleShot {
                angles: [0x3C, 0, 0],
                tr: [0, 0x400, 0xA00],
                focus: focus_on(point),
                frames: 0x78,
            })
        }
        5 => {
            // `sh` into the halfword `ctx[+0x6D8]`, then `bgez` on it
            // sign-extended.
            st.countdown.drain();
            st.countdown.0 = i32::from(st.countdown.0 as i16);
            gate(st.countdown.0 >= 0)
        }
        _ => ArmDirection::PASS,
    }
}
