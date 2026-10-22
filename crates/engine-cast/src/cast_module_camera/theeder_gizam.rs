//! The directors of PROT 0904 (Theeder) and PROT 0906 (Gizam): the countdown
//! gates and camera arms that run beside their ported tick bodies
//! ([`crate::seru_magic_ticks::theeder_tick`],
//! [`crate::seru_magic_ticks::gizam_tick`]).
//!
//! Both modules keep their countdown in `ctx[+0x6D8]` rather than in a word
//! of their own image, and both run their arms with the director
//! **beside** the body ([`ModuleProfile::owns_phase`] `false`): a director
//! hold keeps the body from running that tick, a pass lets the body run the
//! arm and advance. Where the body itself paces an arm (Theeder's arms
//! 9..13), the director mirrors the body's word only to place that arm's
//! exit shot on the right tick and never holds.

use super::*;

// ---------------------------------------------------------------------------
// PROT 0904 - Theeder
// ---------------------------------------------------------------------------

/// `scalar * 10` - what PROT 0904's arm 3 arms `ctx[+0x6D8]` with
/// (`sll 2; addu; sll 1` at `0x801F6DD4..0x801F6DE8`).
pub const THEEDER_RISE_WAIT: i32 = 10;
/// `scalar * 60` - arm 4's re-arm on its exit (`0x801F6F90..0x801F6FA8`).
pub const THEEDER_SHOW_WAIT: i32 = 60;
/// `scalar * 180` - arm 5's re-arm on its exit (`0x801F7084..0x801F70A0`).
pub const THEEDER_STAGE_WAIT: i32 = 180;
/// `scalar * 96` - the settle word `0x801F9204` arm 13 arms on its exit
/// (`0x801F7FCC..0x801F7FE4`), which arm 14 drains before it polls the row.
pub const THEEDER_SETTLE_WAIT: i32 = 96;

/// The arm-7 shot (`0x801F7228..0x801F7274`): pitch `0`, yaw
/// `0x800 - facing`, TR `(0, 0x480, 0x140)`, focus the creature, snapped.
/// PROT 0904's arm 7 holds on the creature's clip loop count `+0x21B`
/// (the body's), so the host arms this shot on the tick the body passes it.
pub fn theeder_attack_shot(creature: ModuleSeat) -> ModuleShot {
    ModuleShot {
        angles: [0, yaw_from(0x800, creature.facing), 0],
        tr: [0, 0x480, 0x140],
        focus: focus_on(creature),
        frames: 1,
    }
}

/// `sh` into the halfword `ctx[+0x6D8]` after a drain, then `bgez` on it
/// sign-extended: `true` while the arm holds.
fn drain_halfword(st: &mut ModuleCamState) -> bool {
    st.countdown.drain();
    st.countdown.0 = i32::from(st.countdown.0 as i16);
    st.countdown.0 >= 0
}

/// PROT 0904 (Theeder) - the camera and countdown half of the tick
/// `0x801F69D8`. `b` is the heading from the victim to the caster
/// (`FUN_80019B28`), `h = (b + 0x800) & 0xFFF`, and the framed point is half
/// a unit from the victim along `h` - where arm 4 then seats the creature,
/// facing `h` ([`crate::seru_magic_ticks::theeder_seat_placement`]).
///
/// | arm | gate | camera / state |
/// |---:|---|---|
/// | 0 | - | snap: pitch `0xD2`, yaw `0x72E - h`, TR `(0, 0x200, 0x800)`, focus the point (`0x801F6C00`) |
/// | 2 | hold while the band timer `ctx[+0x6D8]` is non-zero (`0x801F6C80`) | - |
/// | 3 | - | pitch `0`, yaw `0x800 - h`, TR `(0, 0x400, 0x300)`, the same focus, over `0x46` frames (`0x801F6E24`); `ctx[+0x6D8] = scalar * 10` |
/// | 4 | drain, hold while non-negative (`0x801F6E5C`) | the creature seated; `ctx[+0x6D8] = scalar * 60` |
/// | 5 | drain, hold while non-negative (`0x801F6FE8`) | `ctx[+0x6D8] = scalar * 180` |
/// | 6 | drain, hold while non-negative (`0x801F7138`) | snap: pitch `-0x80`, yaw `0x900 - facing`, TR `(0x60, 0x480, 0x200)`, focus the creature (`0x801F71A0`) |
/// | 7 | the body's: hold while the creature's `+0x21B` reads `0xFF` | [`theeder_attack_shot`], armed by the host |
/// | 8 | - | pitch `0`, yaw `0x800 - facing`, TR `(0, 0x480, 0x300)`, focus the creature, over `scalar * 15` frames (`0x801F73A4`) |
/// | 9 | the body's prong countdown | on its exit: snap pitch `0x300`, yaw `0xA00 - facing`, TR `(0xA0, 0x1A0, 0x680)`, focus the creature (`0x801F7454`) |
/// | 11 | the body's ramp | on its exit: snap pitch `0x100`, yaw `-0x900 - facing`, TR `(0, 0, 0x2800)`, focus the creature (`0x801F7814`) |
/// | 12 | the body's sweep | every pass: pitch `0x80`, yaw `-0x7C0 - facing`, TR `(0, 0, 0x2C00)`, focus the creature, over `0x10` frames (`0x801F7EE0`) |
/// | 13 | the body's retract | the settle word `= scalar * 96` on the way out |
/// | 14 | drain the settle word while positive, hold while it is | the body's row poll follows |
/// | `0xFF` | - | case 8 on the caster (`jal 0x801D5854` with `(ctx[+0x13], 8)` at `0x801F80E4`) |
///
/// Arm 1's `ctx[+0x276]` gate and arm 3's `FUN_8003F2B8(1)` stream poll
/// read as ready: the engine has the creature resident. Arm 1's skip of arm
/// 2 when the band timer is already spent costs one tick here (arm 2 then
/// passes at once).
///
/// PORT: FUN_801F69D8 (PROT 0904; the camera arms 0/3/6/8/9/11/12, the countdown gates of arms 2/4/5/6 and the settle word of arms 13/14)
pub fn theeder_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let v = seats.victim;
    let h = heading(v, seats.caster).wrapping_add(0x800) & 0xFFF;
    let (sin, cos) = trig12(h);
    let point_focus = [
        (half(sin) - i32::from(v.x)) as i16,
        0,
        (half(cos) - i32::from(v.z)) as i16,
    ];
    let seated = ModuleSeat {
        x: (i32::from(v.x) - half(sin)) as i16,
        y: 0,
        z: (i32::from(v.z) - half(cos)) as i16,
        facing: h,
    };
    let creature = st.creature.unwrap_or(seated);
    let cf = creature.facing;
    let on_creature = |pitch: i16, yaw: i32, tr: [i16; 3], frames: u16| ModuleShot {
        angles: [pitch, yaw_from(yaw, cf), 0],
        tr,
        focus: focus_on(creature),
        frames,
    };
    let d = MODULE_DRAIN_PER_TICK;
    match phase {
        0 => ArmDirection::shot(ModuleShot {
            angles: [0xD2, yaw_from(0x72E, h), 0],
            tr: [0, 0x200, 0x800],
            focus: point_focus,
            frames: 1,
        }),
        2 => gate(seats.band_timer != 0),
        3 => {
            st.countdown.0 = SPEED_SCALAR * THEEDER_RISE_WAIT;
            ArmDirection::shot(ModuleShot {
                angles: [0, yaw_from(0x800, h), 0],
                tr: [0, 0x400, 0x300],
                focus: point_focus,
                frames: 0x46,
            })
        }
        4 => {
            if drain_halfword(st) {
                return ArmDirection::HOLD;
            }
            st.creature = Some(seated);
            st.countdown.0 = SPEED_SCALAR * THEEDER_SHOW_WAIT;
            ArmDirection::PASS
        }
        5 => {
            if drain_halfword(st) {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = SPEED_SCALAR * THEEDER_STAGE_WAIT;
            ArmDirection::PASS
        }
        6 => {
            if drain_halfword(st) {
                return ArmDirection::HOLD;
            }
            ArmDirection::shot(on_creature(-0x80, 0x900, [0x60, 0x480, 0x200], 1))
        }
        // The prong countdown the body seeds in arm 8 and drains in arm 9,
        // mirrored to place arm 9's exit shot.
        8 => {
            st.countdown.0 = crate::seru_magic_ticks::THEEDER_PRONG_COUNT * SPEED_SCALAR;
            ArmDirection::shot(on_creature(
                0,
                0x800,
                [0, 0x480, 0x300],
                (SPEED_SCALAR * 15) as u16,
            ))
        }
        9 => {
            st.countdown.0 = i32::from((st.countdown.0 - d) as i16);
            if st.countdown.0 >= 0 {
                return ArmDirection::PASS;
            }
            ArmDirection::shot(on_creature(0x300, 0xA00, [0xA0, 0x1A0, 0x680], 1))
        }
        // Arm 10 zeroes the word ahead of arm 11's ramp.
        10 => {
            st.countdown.0 = 0;
            ArmDirection::PASS
        }
        11 => {
            st.countdown.0 += d >> 1;
            if st.countdown.0 < i32::from(crate::seru_magic_ticks::THEEDER_RETARGET_RAMP_END) {
                return ArmDirection::PASS;
            }
            ArmDirection::shot(on_creature(0x100, -0x900, [0, 0, 0x2800], 1))
        }
        12 => ArmDirection::shot(on_creature(0x80, -0x7C0, [0, 0, 0x2C00], 0x10)),
        // The settle word is written on arm 13's exit; writing it on every
        // pass of the retract leaves the same value behind when it exits.
        13 => {
            st.countdown.0 = SPEED_SCALAR * THEEDER_SETTLE_WAIT;
            ArmDirection::PASS
        }
        14 => {
            if st.countdown.0 > 0 {
                st.countdown.drain();
            }
            gate(st.countdown.0 > 0)
        }
        0xFF => ArmDirection {
            end_frame: true,
            ..ArmDirection::PASS
        },
        _ => ArmDirection::PASS,
    }
}

// ---------------------------------------------------------------------------
// PROT 0906 - Gizam
// ---------------------------------------------------------------------------

/// The `WAIT_SET` operands of record `0x801F7B80` - the one of the three
/// records arm 4 spawns (`0x801F6D78`) whose program calls the module's
/// stager (move-VM op `0x20`, [`crate::cast_module_ticks::gizam_stager`]):
/// `WAIT 95, WAIT 63, WAIT 30, op20(0), WAIT 0, op20(1), WAIT 63, op20(2)`.
/// A wait `v` holds `v + 1` frames (`+0x54 = v << 3`, drained by
/// `scalar * delta` until it is negative, `FUN_80021DF4`).
const GIZAM_STAGER_RECORD_WAITS: [i32; 5] = [95, 63, 30, 0, 63];

/// Ticks from the arm-4 spawn to the record's `op 0x20` call with stager
/// arm `0` (the white and blue flash pair).
pub const GIZAM_STAGER_FLASH_TICK: i32 =
    GIZAM_STAGER_RECORD_WAITS[0] + GIZAM_STAGER_RECORD_WAITS[1] + GIZAM_STAGER_RECORD_WAITS[2] + 3;
/// ... with stager arm `1` (the creature shown).
pub const GIZAM_STAGER_SHOW_TICK: i32 = GIZAM_STAGER_FLASH_TICK + GIZAM_STAGER_RECORD_WAITS[3] + 1;
/// ... with stager arm `2` - `ctx[+0x279] += 1`, the only way out of
/// PROT 0906's phase-5 hole ([`crate::seru_magic_ticks::GIZAM_MISSING_ARM`]).
pub const GIZAM_STAGER_RELEASE_TICK: i32 =
    GIZAM_STAGER_SHOW_TICK + GIZAM_STAGER_RECORD_WAITS[4] + 1;

/// The stager arm the record's program calls on the tick the phase-5 clock
/// reads `countdown` (after this tick's drain), if any. The host fires arm
/// `0`'s fades and arm `1`'s seat pose; arm `2` is the director's pass out
/// of phase 5.
pub fn gizam_stager_arm_due(phase: u8, countdown: i32) -> Option<u8> {
    if phase != crate::seru_magic_ticks::GIZAM_MISSING_ARM {
        return None;
    }
    let left = countdown / SPEED_SCALAR;
    if countdown % SPEED_SCALAR != 0 {
        return None;
    }
    if left == GIZAM_STAGER_RELEASE_TICK - GIZAM_STAGER_FLASH_TICK {
        Some(0)
    } else if left == GIZAM_STAGER_RELEASE_TICK - GIZAM_STAGER_SHOW_TICK {
        Some(1)
    } else {
        None
    }
}

/// PROT 0906's stager arm `0` (`0x801F7788..0x801F7804`): two additive
/// flashes over `0x20` frames, both id `1` and done when they land - white
/// to black, then blue `(0, 0, 0xFF)` to black.
pub const GIZAM_STAGER_FLASHES: [(
    legaia_engine_battle_vm::battle_action::SummonFadeTemplate,
    i16,
); 2] = [
    (
        legaia_engine_battle_vm::battle_action::SummonFadeTemplate {
            kind: 1,
            duration: 0x20,
            start_rgb: [0xFF, 0xFF, 0xFF],
            end_rgb: [0, 0, 0],
            delay: 0,
            hold: 0,
        },
        1,
    ),
    (
        legaia_engine_battle_vm::battle_action::SummonFadeTemplate {
            kind: 1,
            duration: 0x20,
            start_rgb: [0, 0, 0xFF],
            end_rgb: [0, 0, 0],
            delay: 0,
            hold: 0,
        },
        1,
    ),
];

/// `scalar * 180` - arm 6's countdown (`0x801F6D88..0x801F6DAC`).
pub const GIZAM_ATTACK_WAIT: i32 = 180;
/// `scalar * 170` - arm 8's (`0x801F6FA8..0x801F6FC8`).
pub const GIZAM_CLOSE_WAIT: i32 = 170;
/// `scalar * 20` - arm 9's re-arm (`0x801F7040..0x801F7054`).
pub const GIZAM_CUT_WAIT: i32 = 20;
/// `scalar * 40` - arm 11's (`0x801F712C..0x801F7144`).
pub const GIZAM_WIDE_WAIT: i32 = 40;
/// `scalar << 8` - arm 12's re-arm (`sll v0,v0,0x8` at `0x801F7248`).
pub const GIZAM_SWEEP_WAIT: i32 = 0x100;
/// `scalar << 7` - the settle word `0x801F7FD4` arm 13 arms on its exit
/// (`0x801F75FC`), which arm 14 drains before it polls the row.
pub const GIZAM_SETTLE_WAIT: i32 = 0x80;

const fn gizam_spawn(va: u32, anchor: SpawnAnchor) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(va),
        anchor,
    }
}

/// PROT 0906 arm 4's three spawns (`0x801F6D48..0x801F6D78`), right behind
/// the seat installer: the creature's `+0x34` / `+0x38` copied into a stack
/// trio whose height stays the prologue's zero, on the zeroed angle trio.
/// Records `0x801F79E4` and `0x801F7AB8` are the ground disc and the water
/// pillar (op `0x13` default-arm meshes); `0x801F7B80` is the timing record
/// whose `op 0x20` calls release the phase-5 hole
/// ([`GIZAM_STAGER_RELEASE_TICK`]).
pub const GIZAM_POOL_SPAWNS: [ModuleSpawn; 3] = [
    gizam_spawn(0x801F_79E4, SpawnAnchor::CreatureGround { y: 0 }),
    gizam_spawn(0x801F_7AB8, SpawnAnchor::CreatureGround { y: 0 }),
    gizam_spawn(0x801F_7B80, SpawnAnchor::CreatureGround { y: 0 }),
];

/// PROT 0906 arm 7's spawn on its pass (`0x801F6F28..0x801F6F38`): record
/// `0x801F7EBC` on the creature's `+0x3C` / `+0x44`, whose program calls
/// stager arm `5` twelve times and arm `6` once
/// ([`crate::cast_module_ticks::gizam_stager_spawns`]).
pub const GIZAM_SPRAY_SPAWNS: [ModuleSpawn; 1] = [gizam_spawn(0x801F_7EBC, SpawnAnchor::Creature)];

/// PROT 0906 arm 12's two spawns on its pass (`0x801F7230..0x801F7264`):
/// record `0x801F7C44` on the arm's own shot trios (`sp+0x30` / `sp+0x20`),
/// whose program calls stager arm `3` with each of the six bubble points in
/// turn, eight times over; and record `0x801F7DB4` on the creature's
/// `+0x34` / `+0x44`.
pub const GIZAM_BUBBLE_SPAWNS: [ModuleSpawn; 2] = [
    gizam_spawn(0x801F_7C44, SpawnAnchor::ShotFocus),
    gizam_spawn(0x801F_7DB4, SpawnAnchor::Creature),
];

/// PROT 0906's arm-13 exit shot (`0x801F7514..0x801F75CC`), snapped (`a3 =
/// 1`), yaw `-facing` of the creature either way. On a group target
/// (`caster[+0x1DD] >= 8`) the arm calls `FUN_801DCEAC(code, sp+0x30,
/// sp+0x28)`, which writes the negated centroid over the focus trio and the
/// group's extent over TR z; the arm then stores TR y `0x500`, pitch `0x80`
/// and TR z `= 3 * extent` (`lh v1,0x2c(sp)`; `sll 1; addu`). On a single
/// target it focuses the victim's `+0x34` / `+0x38` and stores TR y
/// `0x600`, pitch `0x40` and TR z `= (2 * ctx[+0x6D0]) / 3` (the
/// `0x55555556` reciprocal, rounded toward zero).
///
/// The group branch reads the enemy row's aim the host measured
/// ([`ModuleCamState::row_aim`], `FUN_801DCEAC(9, ..)`): a player Gizam's
/// group target is that row. With no live monster the focus stays on the
/// origin and the extent is `0`.
pub fn gizam_sweep_exit_shot(
    st: &ModuleCamState,
    creature: ModuleSeat,
    seats: ModuleCamSeats,
) -> ModuleShot {
    let yaw = yaw_from(0, creature.facing);
    if st.victim_slot.is_some_and(|code| code >= 8) {
        let aim = st
            .row_aim
            .unwrap_or(legaia_engine_battle_vm::battle_target_group::GroupAim {
                centroid_x: 0,
                centroid_z: 0,
                extent: 0,
            });
        ModuleShot {
            angles: [0x80, yaw, 0],
            tr: [0, 0x500, (i32::from(aim.extent) * 3) as i16],
            focus: [aim.centroid_x, 0, aim.centroid_z],
            frames: 1,
        }
    } else {
        ModuleShot {
            angles: [0x40, yaw, 0],
            tr: [0, 0x600, ((2 * seats.depth_raw) / 3) as i16],
            focus: focus_on(seats.victim),
            frames: 1,
        }
    }
}

/// PROT 0906 (Gizam) - the camera and countdown half of the tick
/// `0x801F69F4`. The creature is the summon seat `actor_table[7]` as the host
/// last placed it; arms 0 and 1 frame the caster.
///
/// | arm | gate | camera / state |
/// |---:|---|---|
/// | 0 | - | snap: pitch `0x100`, yaw `0x710 - facing`, TR `(0, 0x400, 0x400)`, focus the caster (`0x801F6BE0`) |
/// | 1 | - | pitch `0x80`, yaw `0x878 - facing`, TR `(0, 0x298, 0x900)`, focus the caster, over `0x168` frames |
/// | 3 | hold while the band timer `ctx[+0x6D8]` is non-zero (`0x801F6CE0`) | - |
/// | 4 | - | the creature seated and hidden; spawns [`GIZAM_POOL_SPAWNS`]; the stager record clock starts |
/// | 5 | hold until the record's `op 0x20` stager arm `2` ([`GIZAM_STAGER_RELEASE_TICK`]) | - |
/// | 6 | - | `ctx[+0x6D8] = scalar * 180`; the spell name |
/// | 7 | drain, hold while non-negative (`0x801F6E70`) | the attack name; snap: pitch `-0x40`, yaw `0x788 - facing`, TR `(0, 0x400, 0x400)`, focus the creature (`0x801F6F20`); spawns [`GIZAM_SPRAY_SPAWNS`] |
/// | 8 | - | pitch `0x40`, yaw `0x9E0 - facing`, TR `(0, 0x338, 0x800)`, focus the creature, over `0xAA` frames; `ctx[+0x6D8] = scalar * 170` |
/// | 9 | drain, hold while non-negative (`0x801F7038`) | snap: pitch `-0x40`, yaw `0x900 - facing`, TR `(0, 0x700, 0x300)`, focus the creature (`0x801F709C`); `ctx[+0x6D8] = scalar * 20` |
/// | 10 | drain, hold while non-negative (`0x801F70E4`) | snap: pitch `0`, yaw `0x800 - facing`, TR `(0, 0x600, 0x200)`, focus the creature |
/// | 11 | - | pitch `0xB4`, yaw `0x800 - facing`, TR `(0, 0x280, 0x800)`, focus the creature, over `0x28` frames; `ctx[+0x6D8] = scalar * 40` |
/// | 12 | drain, hold while non-negative (`0x801F71D4`) | pitch `0xC0`, yaw `0x800 - facing`, TR `(0, 0, 0x2800)`, focus the creature, over `0x100` frames (`0x801F7228`); `ctx[+0x6D8] = scalar << 8`; spawns [`GIZAM_BUBBLE_SPAWNS`] |
/// | 13 | drain, hold while non-negative (`0x801F7330`) | the sweep; snap: [`gizam_sweep_exit_shot`] - the enemy row's centroid, or the victim (`0x801F75CC`); the settle word `= scalar << 7` |
/// | 14 | drain the settle word while positive, hold while it is (`0x801F7610..0x801F7640`) | the body's row poll follows |
///
/// Arm 9 frames the creature's `+0x3C` / `+0x40` pair rather than its
/// `+0x34` / `+0x38` seat; the port frames the seat.
///
/// PORT: FUN_801F69F4 (PROT 0906; the camera arms 0/1/7..13, the countdown gates of arms 3/5/7/9/10/12/13/14, the phase-5 stager record clock, the arm 6/7 captions, arm 13's caption clear and the arm 4/7/12 spawn calls)
pub fn gizam_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let c = seats.caster;
    let creature = st.creature_live.or(st.creature).unwrap_or(seats.victim);
    let on = |seat: ModuleSeat, pitch: i16, yaw: i32, tr: [i16; 3], frames: u16| ModuleShot {
        angles: [pitch, yaw_from(yaw, seat.facing), 0],
        tr,
        focus: focus_on(seat),
        frames,
    };
    match phase {
        0 => ArmDirection::shot(on(c, 0x100, 0x710, [0, 0x400, 0x400], 1)),
        1 => ArmDirection::shot(on(c, 0x80, 0x878, [0, 0x298, 0x900], 0x168)),
        3 => gate(seats.band_timer != 0),
        4 => {
            st.countdown.0 = SPEED_SCALAR * GIZAM_STAGER_RELEASE_TICK;
            // The installer `FUN_801F19EC` the arm calls first seats the
            // creature on the caster, wearing its facing; the arm's spawns
            // read that seat before the host has bound the creature's mesh.
            if st.creature_live.is_none() && st.creature.is_none() {
                st.creature = Some(c);
            }
            ArmDirection {
                spawns: &GIZAM_POOL_SPAWNS,
                ..ArmDirection::PASS
            }
        }
        5 => {
            st.countdown.drain();
            gate(st.countdown.0 > 0)
        }
        6 => {
            st.countdown.0 = SPEED_SCALAR * GIZAM_ATTACK_WAIT;
            // The spell name (`FUN_8003541C` at `0x801F6E34`).
            ArmDirection {
                caption: Some(ModuleCaption::SpellName),
                ..ArmDirection::PASS
            }
        }
        7 => {
            if drain_halfword(st) {
                return ArmDirection::HOLD;
            }
            // `FUN_800319A8(0)` clears the spell name and the attack name
            // goes up (`0x801F6E78..0x801F6EC8`) before the shot.
            ArmDirection {
                spawns: &GIZAM_SPRAY_SPAWNS,
                caption: Some(ModuleCaption::AttackName),
                ..ArmDirection::shot(on(creature, -0x40, 0x788, [0, 0x400, 0x400], 1))
            }
        }
        8 => {
            st.countdown.0 = SPEED_SCALAR * GIZAM_CLOSE_WAIT;
            ArmDirection::shot(on(creature, 0x40, 0x9E0, [0, 0x338, 0x800], 0xAA))
        }
        9 => {
            if drain_halfword(st) {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = SPEED_SCALAR * GIZAM_CUT_WAIT;
            ArmDirection::shot(on(creature, -0x40, 0x900, [0, 0x700, 0x300], 1))
        }
        10 => {
            if drain_halfword(st) {
                return ArmDirection::HOLD;
            }
            ArmDirection::shot(on(creature, 0, 0x800, [0, 0x600, 0x200], 1))
        }
        11 => {
            st.countdown.0 = SPEED_SCALAR * GIZAM_WIDE_WAIT;
            ArmDirection::shot(on(creature, 0xB4, 0x800, [0, 0x280, 0x800], 0x28))
        }
        12 => {
            if drain_halfword(st) {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = SPEED_SCALAR * GIZAM_SWEEP_WAIT;
            ArmDirection {
                spawns: &GIZAM_BUBBLE_SPAWNS,
                ..ArmDirection::shot(on(creature, 0xC0, 0x800, [0, 0, 0x2800], 0x100))
            }
        }
        13 => {
            if drain_halfword(st) {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = SPEED_SCALAR * GIZAM_SETTLE_WAIT;
            // The exit shot, then `FUN_800319A8(0)` (`0x801F75D4`): the
            // attack name leaves with the sweep.
            ArmDirection {
                caption: Some(ModuleCaption::Cleared),
                ..ArmDirection::shot(gizam_sweep_exit_shot(st, creature, seats))
            }
        }
        14 => {
            if st.countdown.0 > 0 {
                st.countdown.drain();
            }
            gate(st.countdown.0 > 0)
        }
        _ => ArmDirection::PASS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seats(band_timer: i32) -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 63,
                y: 0,
                z: -591,
                facing: 0,
            },
            victim: ModuleSeat {
                x: 70,
                y: 0,
                z: -260,
                facing: 0x800,
            },
            band_timer,
            first_monster: 0,
            caster_monster: 0,
            action: 0,
            depth_raw: 0,
            caster_latch: 0,
            victim_radius: 0,
        }
    }

    /// Ticks a director runs `phase` for (holds plus the pass).
    fn dwell(direct: ModuleDirector, st: &mut ModuleCamState, phase: u8, s: ModuleCamSeats) -> u32 {
        let mut n = 1;
        while direct(st, phase, s).hold {
            n += 1;
            assert!(n < 10_000, "arm {phase} never passes");
        }
        n
    }

    /// Arms 4..6 hold their seeds `scalar * 10 / 60 / 180`, drained a scalar
    /// a tick and passing on the first negative halfword.
    #[test]
    fn theeder_countdown_arms_hold_their_seeds() {
        let mut st = ModuleCamState::default();
        let s = seats(0);
        assert!(!theeder_direct(&mut st, 3, s).hold);
        assert_eq!(dwell(theeder_direct, &mut st, 4, s), 11);
        assert!(st.creature.is_some(), "arm 4 seats the creature");
        assert_eq!(dwell(theeder_direct, &mut st, 5, s), 61);
        assert_eq!(dwell(theeder_direct, &mut st, 6, s), 181);
    }

    #[test]
    fn theeder_arm_2_waits_on_the_band_timer() {
        let mut st = ModuleCamState::default();
        assert!(theeder_direct(&mut st, 2, seats(5)).hold);
        assert!(!theeder_direct(&mut st, 2, seats(0)).hold);
    }

    /// Arm 9's exit shot lands on the tick the body's prong countdown runs
    /// out, arms 8..13 never hold (the body paces them), and arm 14 drains
    /// the settle word arm 13 left.
    #[test]
    fn theeder_body_paced_arms_only_place_shots() {
        let mut st = ModuleCamState::default();
        let s = seats(0);
        assert!(theeder_direct(&mut st, 8, s).shot.is_some());
        let mut n = 0;
        loop {
            n += 1;
            let d = theeder_direct(&mut st, 9, s);
            assert!(!d.hold);
            if d.shot.is_some() {
                break;
            }
        }
        assert_eq!(n, crate::seru_magic_ticks::THEEDER_PRONG_COUNT as u32 + 1);
        assert!(!theeder_direct(&mut st, 13, s).hold);
        assert_eq!(
            dwell(theeder_direct, &mut st, 14, s),
            THEEDER_SETTLE_WAIT as u32
        );
    }

    /// The phase-5 hole holds for the record's clock, and the stager's arms
    /// 0 and 1 come due on its 191st and 192nd tick.
    #[test]
    fn gizam_phase_5_runs_the_stager_record_clock() {
        let mut st = ModuleCamState::default();
        let s = seats(0);
        assert!(!gizam_direct(&mut st, 4, s).hold);
        let mut due = Vec::new();
        let mut n = 0;
        loop {
            n += 1;
            let d = gizam_direct(&mut st, 5, s);
            if let Some(a) = gizam_stager_arm_due(5, st.countdown.0) {
                due.push((n, a));
            }
            if !d.hold {
                break;
            }
        }
        assert_eq!(n, 256);
        assert_eq!(due, vec![(191, 0), (192, 1)]);
    }

    /// Arms 4, 7 and 12 report their spawn calls on their passes, and arm
    /// 4 seats the creature on the caster when no seat is live yet, so the
    /// pool lands where the installer puts it.
    #[test]
    fn gizam_reports_its_spawn_calls_on_the_seat_spray_and_bubble_arms() {
        let mut st = ModuleCamState::default();
        let s = seats(0);
        let d = gizam_direct(&mut st, 4, s);
        assert_eq!(d.spawns, &GIZAM_POOL_SPAWNS[..]);
        assert_eq!(st.creature, Some(s.caster));
        let at = spawn_anchor_point(d.spawns[0].anchor, st.creature, None, None);
        assert_eq!(at, Some(([63, 0, -591], [0; 3])));
        // Arm 7 holds on its countdown first and spawns only on the pass,
        // beside the attack name.
        st.countdown.0 = SPEED_SCALAR;
        let held = gizam_direct(&mut st, 7, s);
        assert!(held.hold && held.spawns.is_empty() && held.caption.is_none());
        let d = gizam_direct(&mut st, 7, s);
        assert_eq!(d.spawns, &GIZAM_SPRAY_SPAWNS[..]);
        assert_eq!(d.caption, Some(ModuleCaption::AttackName));
        assert_eq!(
            gizam_direct(&mut st, 6, s).caption,
            Some(ModuleCaption::SpellName)
        );
        st.countdown.0 = 0;
        let d = gizam_direct(&mut st, 12, s);
        assert_eq!(d.spawns, &GIZAM_BUBBLE_SPAWNS[..]);
        // The bubble timer sits on the arm's own shot: the negated focus.
        let shot = d.shot.expect("arm 12 arms a shot");
        let at = spawn_anchor_point(d.spawns[0].anchor, st.creature, None, Some(shot));
        assert_eq!(at.map(|a| a.0), Some([-63, 0, 591]));
    }

    /// Arm 13's exit snaps onto the victim at two thirds of the band depth
    /// on a single target, and onto the enemy row's centroid at three
    /// extents on a group code.
    #[test]
    fn gizam_arm_13_exit_frames_the_victim_or_the_row() {
        let mut s = seats(0);
        s.depth_raw = 0x900;
        let mut st = ModuleCamState {
            creature: Some(ModuleSeat {
                x: 0,
                y: 0,
                z: 0,
                facing: 0x100,
            }),
            victim_slot: Some(3),
            ..Default::default()
        };
        st.countdown.0 = 0;
        let d = gizam_direct(&mut st, 13, s);
        let shot = d.shot.expect("arm 13 exit shot");
        assert_eq!(shot.angles, [0x40, 0xF00, 0]);
        assert_eq!(shot.tr, [0, 0x600, 0x600]);
        assert_eq!(shot.focus, [-70, 0, 260]);
        assert_eq!(shot.frames, 1);

        st.victim_slot = Some(9);
        st.row_aim = Some(legaia_engine_battle_vm::battle_target_group::GroupAim {
            centroid_x: -100,
            centroid_z: 300,
            extent: 0x200,
        });
        st.countdown.0 = 0;
        let shot = gizam_direct(&mut st, 13, s).shot.expect("group shot");
        assert_eq!(shot.angles, [0x80, 0xF00, 0]);
        assert_eq!(shot.tr, [0, 0x500, 0x600]);
        assert_eq!(shot.focus, [-100, 0, 300]);
    }

    #[test]
    fn gizam_countdown_arms_hold_their_seeds() {
        let mut st = ModuleCamState::default();
        let s = seats(0);
        assert!(gizam_direct(&mut st, 3, seats(1)).hold);
        assert!(!gizam_direct(&mut st, 6, s).hold);
        assert_eq!(dwell(gizam_direct, &mut st, 7, s), 181);
        assert!(!gizam_direct(&mut st, 8, s).hold);
        assert_eq!(dwell(gizam_direct, &mut st, 9, s), 171);
        assert_eq!(dwell(gizam_direct, &mut st, 10, s), 21);
        assert!(!gizam_direct(&mut st, 11, s).hold);
        assert_eq!(dwell(gizam_direct, &mut st, 12, s), 41);
        assert_eq!(dwell(gizam_direct, &mut st, 13, s), 257);
        assert_eq!(dwell(gizam_direct, &mut st, 14, s), 128);
    }
}
