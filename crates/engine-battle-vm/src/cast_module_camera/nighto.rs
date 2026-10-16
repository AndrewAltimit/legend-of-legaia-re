//! PROT 0907 (Nighto) - the camera and countdown half of its tick body.

use super::*;

/// The countdown word's marker for "arm 13 has passed and not yet said
/// which leg it took": retail stores the leg's own countdown in the pass
/// (`scalar * 120` for the kill, `scalar * 70` for the confuse), and the
/// director, which does not see the fork, seeds it on the leg's first pass
/// instead ([`nighto_leg_countdown`]).
const NIGHTO_FORK_PENDING: i32 = i32::MIN;

/// The countdown PROT 0907's kill leg leaves for arm 14 (`li` / `sll` to
/// `scalar * 0x78` at `0x801F7E68..0x801F7E80`).
pub const NIGHTO_KILL_SETTLE_PER_SCALAR: i32 = 0x78;

/// The countdown PROT 0907's confuse leg leaves for arm 15 (`scalar * 0x46`
/// at `0x801F7E20..0x801F7E44`).
pub const NIGHTO_CONFUSE_SETTLE_PER_SCALAR: i32 = 0x46;

/// The clip PROT 0907's arm 10 waits in: arm 7 stages `1`, arm 8 bumps it to
/// `2` with the `+0x1DC` cut, and the commit of clip 2 seeds the creature's
/// loop counter `+0x21B` with the entry's count `0xFF` (`FUN_8004AD80` at
/// `0x8004BDF4`). Arm 10 holds while the counter reads `0xFF`
/// ([`NIGHTO_LOOP_SENTINEL`]), which the anim tick's window arm takes down
/// the first time clip 2's cursor reaches its park frame (`19`).
pub const NIGHTO_SUMMON_CLIP: u8 = 2;

/// The `+0x21B` value PROT 0907's arm 10 holds on (`li v0,0xff` / `beq` at
/// `0x801F7AC0..0x801F7AC8`).
pub const NIGHTO_LOOP_SENTINEL: u8 = 0xFF;

/// PROT 0907 arm 4's two spawns on the grid point at the floor
/// (`jal 0x80021B04` at `0x801F72C4` / `0x801F72DC`): the summoning disc
/// `0x801F81D0`, whose program calls the stager's arm `1` five times (the
/// rings it sends out), and the flash `0x801F8280`. The arm writes the
/// point's `x` / `z` and the yaw `0` into the stack trios and leaves the
/// height and the pitch / roll slots as the earlier arms left them; the
/// port seats them flat on the floor, which is where the capture shows the
/// disc.
pub const NIGHTO_OPEN_SPAWNS: [ModuleSpawn; 2] = [
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_81D0),
        anchor: SpawnAnchor::ArmPoint,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8280),
        anchor: SpawnAnchor::ArmPoint,
    },
];

/// PROT 0907 arm 5's spawn (`0x801F7398`): the crystal `0x801F82C0`
/// (library mesh `18`), seated on the grid point `0x200` under the floor
/// facing the heading from the victim to the point, raised through the
/// floor clip by its own program; it calls the stager's arm `0` (the eight
/// rays) and then breaks apart on its `+0x7A` ramp.
pub const NIGHTO_CRYSTAL_SPAWNS: [ModuleSpawn; 1] = [ModuleSpawn {
    record: SpawnRecord::Module(0x801F_82C0),
    anchor: SpawnAnchor::ArmPoint,
}];

/// PROT 0907 arm 10's spawn (`0x801F7B6C`): `0x801F83BC`, `0xA0` ahead of
/// the creature along its heading at `y = -0x1A4`, at the creature's angle
/// trio `+0x44`. Its program calls the stager's arm `2` eight times, a
/// ring `0x801F8414` a call.
pub const NIGHTO_SONG_SPAWNS: [ModuleSpawn; 1] = [ModuleSpawn {
    record: SpawnRecord::Module(0x801F_83BC),
    anchor: SpawnAnchor::ArmPoint,
}];

/// PROT 0907 arm 11's spawn (`0x801F7CE0`): `0x801F845C`, back from the
/// victim toward the creature by a sixth of the band's depth
/// (`sin(f) * ctx[+0x6D0] / 0x6000` off its `x`, the same with `cos` off its
/// `z`) at `y = -0x140`, at the creature's angle trio. Its program calls the
/// stager's arm `3` seven times - the red rings that close over the victim.
pub const NIGHTO_RING_SPAWNS: [ModuleSpawn; 1] = [ModuleSpawn {
    record: SpawnRecord::Module(0x801F_845C),
    anchor: SpawnAnchor::ArmPoint,
}];

/// `n / 0x6000` the way arm 11 divides: `mult` by `0x2AAAAAAB`, `mfhi`,
/// `sra 0xC`, minus the sign (`0x801F7C50..0x801F7C78`).
fn div_0x6000(n: i32) -> i32 {
    let hi = ((i64::from(n) * 0x2AAA_AAAB) >> 32) as i32;
    (hi >> 12) - (n >> 31)
}

/// `trunc(v * 0xA0 / 0x1000)` (`sll 2; addu; sll 5`, then `bgez; addiu
/// 0xFFF; sra 0xC`).
fn ahead_0xa0(v: i16) -> i32 {
    (i32::from(v) * 0xA0) / 0x1000
}

/// Seed the countdown of the leg arm 13 forked to, on the leg's first pass.
fn nighto_leg_countdown(st: &mut ModuleCamState, per_scalar: i32) {
    if st.countdown.0 == NIGHTO_FORK_PENDING {
        st.countdown.0 = SPEED_SCALAR * per_scalar;
    }
}

/// The halfword `ctx[+0x6D8]` gate: drain, store, hold while non-negative.
fn nighto_drain_holds(st: &mut ModuleCamState) -> bool {
    st.countdown.drain();
    st.countdown.0 = i32::from(st.countdown.0 as i16);
    st.countdown.0 >= 0
}

/// PROT 0907 (Nighto) - the camera and countdown half of the tick
/// `0x801F69E8`; the phase chain and the kill / confuse fork are
/// [`crate::seru_magic_ticks::nighto_tick`], which the host runs on the pass
/// this lets through. The countdown is the band's word `ctx[+0x6D8]`. The
/// framed point is the victim's grid cell half a unit back toward the caster
/// (PROT 0908's [`zenoir_grid`] arithmetic, `0x801F6B88..0x801F7010`), `y`
/// the heading from the victim to it, `f` the creature's heading, `P` the
/// per-tick frame product.
///
/// | arm | gate | camera / state |
/// |---:|---|---|
/// | 0 | - | snap: pitch `0x40`, yaw `-y`, TR `(0, -0x1000, 0x3000)`, focus the grid point |
/// | 1 | - | pitch `0x400`, yaw `-y`, TR `(0, 0, 0x600)`, the same focus, over `0x78` frames |
/// | 3 | hold until the band timer `ctx[+0x6D8]` reads `0` | - |
/// | 4 | the stream poll | pitch `0x200`, yaw `-y`, TR `(0, 0, 0x600)`, over `0x80` frames; spawns [`NIGHTO_OPEN_SPAWNS`]; `ctx[+0x6D8] = scalar * 129` |
/// | 5 | drift TR z `+P`; drain, hold while non-negative | snap... over `0x10` frames: pitch `0`, TR `(0, 0x500, 0xA00)`; spawns [`NIGHTO_CRYSTAL_SPAWNS`]; `ctx[+0x6D8] = scalar * 96` |
/// | 6 | drift TR z `+2P`; drain, hold while non-negative | seats the creature on the grid point facing the victim; `ctx[+0x6D8] = scalar << 6` |
/// | 7 | drift TR z `-4P`; drain, hold while non-negative | the spell name (`FUN_8003541C` at `0x801F78F8`); `ctx[+0x6D8] = scalar * 180` |
/// | 8 | drain, hold while non-negative | the attack name (`0x801F79E8`); snap: pitch `-0x100`, yaw `0x800 - f`, TR `(0, 0x500, 0x400)`, focus the creature |
/// | 9 | - | pitch `0x40`, yaw `0x800 - f`, TR `(0, 0x700, 0x800)`, over `0xC0` frames |
/// | 10 | hold while the creature's `+0x21B` reads `0xFF` ([`NIGHTO_LOOP_SENTINEL`]), clip `2` committed ([`NIGHTO_SUMMON_CLIP`]) | pitch `0`, yaw `0x800 - f`, TR `(0, 0x700, 0x480)`, over `0x80` frames; spawns [`NIGHTO_SONG_SPAWNS`]; `ctx[+0x6D8] = scalar << 7` |
/// | 11 | drain, hold while non-negative | spawns [`NIGHTO_RING_SPAWNS`] |
/// | 12 | - | snap: yaw `-f`, TR `(0, 0x500, 2 * ctx[+0x6D0] / 3)`, focus the victim; `ctx[+0x6D8] = scalar * 160` |
/// | 13 | drain, hold while non-negative | the fork (the body's) |
/// | 14 | drain | the kill leg's settle: the body holds on the victim's fade |
/// | 15 | drain, hold while non-negative | the confuse leg |
///
/// PORT: FUN_801F69E8 (PROT 0907; the camera arms 0/1/4/5/8/9/10/12, the arm 7/8 captions, the countdown gates of arms 3/5/6/7/8/11/13/15, arm 10's `+0x21B` loop-counter gate, arm 6's creature placement, the TR z drift of arms 5..7 and the spawn calls of arms 4/5/10/11)
pub fn nighto_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let v = seats.victim;
    let h = heading(v, seats.caster).wrapping_add(0x800) & 0xFFF;
    let (sin, cos) = trig12(h);
    let point = ModuleSeat {
        x: zenoir_grid(v.x, sin),
        y: 0,
        z: zenoir_grid(v.z, cos),
        facing: 0,
    };
    // `-FUN_80019B28(victim, point) & 0xFFF`.
    let aim = (-i32::from(heading(v, point)) & 0xFFF) as i16;
    let p = MODULE_DRAIN_PER_TICK;
    let creature = {
        let placed = st.creature.unwrap_or(point);
        match st.creature_live {
            Some(l) => ModuleSeat {
                x: l.x,
                z: l.z,
                ..placed
            },
            None => placed,
        }
    };
    let on_creature = |pitch: i16, k: i32, tr: [i16; 3], frames: u16| {
        ArmDirection::shot(ModuleShot {
            angles: [pitch, yaw_from(k, creature.facing), 0],
            tr,
            focus: focus_on(creature),
            frames,
        })
    };
    let tr_z = |n: i32| CaptureDrift {
        tr_z: (n * p) as i16,
        ..CaptureDrift::default()
    };
    match phase {
        0 => ArmDirection::shot(ModuleShot {
            angles: [0x40, aim, 0],
            tr: [0, -0x1000, 0x3000],
            focus: focus_on(point),
            frames: 1,
        }),
        1 => ArmDirection::shot(ModuleShot {
            angles: [0x400, aim, 0],
            tr: [0, 0, 0x600],
            focus: focus_on(point),
            frames: 0x78,
        }),
        3 => gate(seats.band_timer != 0),
        4 => {
            st.countdown.0 = SPEED_SCALAR * 129;
            ArmDirection {
                spawns: &NIGHTO_OPEN_SPAWNS,
                arm_point: Some(([point.x, 0, point.z], [0; 3])),
                ..ArmDirection::shot(ModuleShot {
                    angles: [0x200, aim, 0],
                    tr: [0, 0, 0x600],
                    focus: focus_on(point),
                    frames: 0x80,
                })
            }
        }
        5 => {
            if nighto_drain_holds(st) {
                return ArmDirection::HOLD.drifted(tr_z(1));
            }
            st.countdown.0 = SPEED_SCALAR * 96;
            // `+0x200` under the floor, yawed by `FUN_80019B28(victim,
            // point)` (`0x801F7350..0x801F7394`); the pitch / roll slots are
            // not written on this path and are seated level.
            let yaw = heading(v, point) as i16;
            ArmDirection {
                spawns: &NIGHTO_CRYSTAL_SPAWNS,
                arm_point: Some(([point.x, 0x200, point.z], [0, yaw, 0])),
                ..ArmDirection::shot(ModuleShot {
                    angles: [0, aim, 0],
                    tr: [0, 0x500, 0xA00],
                    focus: focus_on(point),
                    frames: 0x10,
                })
            }
            .drifted(tr_z(1))
        }
        6 => {
            if nighto_drain_holds(st) {
                return ArmDirection::HOLD.drifted(tr_z(2));
            }
            // `FUN_801F19EC`, then `+0x34` / `+0x38` = the grid point and
            // `+0x46 = FUN_80019B28(victim, point) + 0x800`
            // (`0x801F76C0..0x801F77A4`): seated facing the victim.
            st.creature = Some(ModuleSeat {
                facing: heading(v, point).wrapping_add(0x800) & 0xFFF,
                ..point
            });
            st.countdown.0 = SPEED_SCALAR << 6;
            ArmDirection::PASS.drifted(tr_z(2))
        }
        7 => {
            if nighto_drain_holds(st) {
                return ArmDirection::HOLD.drifted(tr_z(-4));
            }
            st.countdown.0 = SPEED_SCALAR * 180;
            ArmDirection {
                caption: Some(ModuleCaption::SpellName),
                ..ArmDirection::PASS.drifted(tr_z(-4))
            }
        }
        8 => {
            if nighto_drain_holds(st) {
                return ArmDirection::HOLD;
            }
            ArmDirection {
                caption: Some(ModuleCaption::AttackName),
                ..on_creature(-0x100, 0x800, [0, 0x500, 0x400], 1)
            }
        }
        9 => on_creature(0x40, 0x800, [0, 0x700, 0x800], 0xC0),
        10 => {
            // `lbu v1,0x21b(s1); li v0,0xff; beq v1,v0,<epilogue>` at
            // `0x801F7AC0`. Before clip 2 commits the counter still reads
            // clip 1's (no window, `0`); retail's commit lands on the anim
            // pass between arm 8's bump and this arm, so the port also
            // waits for the commit rather than pass on the stale count.
            if st.creature_clip.is_some_and(|c| c != NIGHTO_SUMMON_CLIP)
                || st.creature_loops == Some(NIGHTO_LOOP_SENTINEL)
            {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = SPEED_SCALAR << 7;
            let (fs, fc) = trig12(creature.facing & 0xFFF);
            let ahead = [
                (i32::from(creature.x) + ahead_0xa0(fs)) as i16,
                -0x1A4,
                (i32::from(creature.z) + ahead_0xa0(fc)) as i16,
            ];
            ArmDirection {
                spawns: &NIGHTO_SONG_SPAWNS,
                arm_point: Some((ahead, [0, creature.facing as i16, 0])),
                ..on_creature(0, 0x800, [0, 0x700, 0x480], 0x80)
            }
        }
        11 => {
            if nighto_drain_holds(st) {
                return ArmDirection::HOLD;
            }
            let (fs, fc) = trig12(creature.facing & 0xFFF);
            let d = seats.depth_raw;
            let back = [
                (i32::from(v.x) - div_0x6000(i32::from(fs) * d)) as i16,
                -0x140,
                (i32::from(v.z) - div_0x6000(i32::from(fc) * d)) as i16,
            ];
            ArmDirection {
                spawns: &NIGHTO_RING_SPAWNS,
                arm_point: Some((back, [0, creature.facing as i16, 0])),
                ..ArmDirection::PASS
            }
        }
        12 => {
            st.countdown.0 = SPEED_SCALAR * 160;
            // `2 * ctx[+0x6D0] / 3` by the `0x55555556` reciprocal.
            let depth = (2 * seats.depth_raw) / 3;
            ArmDirection::shot(ModuleShot {
                angles: [0, yaw_from(0, creature.facing), 0],
                tr: [0, 0x500, depth as i16],
                focus: focus_on(v),
                frames: 1,
            })
        }
        13 => {
            if nighto_drain_holds(st) {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = NIGHTO_FORK_PENDING;
            ArmDirection::PASS
        }
        14 => {
            nighto_leg_countdown(st, NIGHTO_KILL_SETTLE_PER_SCALAR);
            st.countdown.drain();
            st.countdown.0 = i32::from(st.countdown.0 as i16);
            ArmDirection::PASS
        }
        15 => {
            nighto_leg_countdown(st, NIGHTO_CONFUSE_SETTLE_PER_SCALAR);
            gate(nighto_drain_holds(st))
        }
        _ => ArmDirection::PASS,
    }
}
