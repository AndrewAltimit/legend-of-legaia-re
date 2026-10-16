//! PROT 0909 (Viguro) - the camera and countdown half of its tick body.

use super::*;

/// The depth PROT 0909's arm 4 waits for its creature to cross
/// (`lh v0,0x38(s4)` / `slti v0,v0,0x200` at `0x801F6C7C..0x801F6C84`): the
/// creature its stager seats at `z = 0xC00` walks in on its own clip, and
/// the arm holds until it is nearer than this.
pub const VIGURO_ARRIVAL_Z: i32 = 0x200;

/// The height PROT 0909's arm 12 climbs its countdown to before the strike
/// lands (`slti v0,v0,0x201` on the halfword `ctx[+0x6D8]`).
pub const VIGURO_CHARGE_TOP: i32 = 0x201;

/// PROT 0909 arm 2's two spawns, both at the literal point `(0, -0x100,
/// 0x600)`, the second rolled `0x800` (`jal 0x80021B04` at `0x801F6C4C` /
/// `0x801F6C6C`). Record `0x801F7E18` waits `0x7F` and calls the stager's arm
/// `0`, which seats the creature and releases phase `3`.
pub const VIGURO_OPEN_SPAWNS: [ModuleSpawn; 2] = [
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_7E18),
        anchor: SpawnAnchor::Literal {
            pos: [0, -0x100, 0x600],
            rot: [0; 3],
        },
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_7EA0),
        anchor: SpawnAnchor::Literal {
            pos: [0, -0x100, 0x600],
            rot: [0, 0, 0x800],
        },
    },
];

/// PROT 0909 arm 7's spawn on the creature (`a0 = creature + 0x3C`, `a1 =
/// creature + 0x44`; `0x801F6F6C`). Record `0x801F7F20` runs a six-pass loop
/// and then calls the stager's arm `1`, which releases phase `8`.
pub const VIGURO_CREATURE_SPAWNS: [ModuleSpawn; 1] = [ModuleSpawn {
    record: SpawnRecord::Module(0x801F_7F20),
    anchor: SpawnAnchor::Creature,
}];

/// PROT 0909's two records that call into its stager, as `(record, stager
/// arm, frames from the spawn to the call)`. Record `0x801F7E18` (arm 2's
/// first spawn) is `WAIT 0x7F` then op `0x20` arm `0`; record `0x801F7F20`
/// (arm 7's spawn) waits `0x13`, loops a 20-frame body six times and then
/// calls arm `1`. A host that does not seat a record keeps its clock in its
/// place, so each rendezvous falls where the record's script puts it and
/// never earlier.
pub const VIGURO_STAGER_CALLERS: [(u32, u8, u16); 2] =
    [(0x801F_7E18, 0, 0x7F), (0x801F_7F20, 1, 0x13 + 6 * 0x14)];

/// The `ctx[+0x6D8]` gate the counted arms share: drain by the frame
/// product, store the halfword, and hold while it reads non-negative
/// (`subu; sh; sll 0x10; bgez`).
fn viguro_drain_holds(st: &mut ModuleCamState) -> bool {
    st.countdown.drain();
    st.countdown.0 = i32::from(st.countdown.0 as i16);
    st.countdown.0 >= 0
}

/// The enemy row's framing point, `FUN_801DCEAC(9, ..)`'s negated centroid
/// on the floor; the origin with no live monster.
fn row_focus(st: &ModuleCamState) -> [i16; 3] {
    st.row_aim
        .map_or([0; 3], |a| [a.centroid_x, 0, a.centroid_z])
}

/// PROT 0909 (Viguro) - the camera and countdown half of the tick
/// `0x801F69F4`; the phase chain and the damage sweep are
/// [`crate::seru_magic_ticks::viguro_tick`], which the host runs on the
/// pass this lets through. The module's countdown is the band's word
/// `ctx[+0x6D8]`. `P` is the per-tick frame product (`scalar * delta` a
/// battle pass, [`MODULE_DRAIN_PER_TICK`] a tick); `f` the creature's
/// heading `+0x46`.
///
/// | arm | gate | camera / state |
/// |---:|---|---|
/// | 0 | - | snap: TR `(0, 0x400, 0)`, focus the origin (`0x801F6BD0` / `0x801F7144`) |
/// | 1 | the stream request waits on `ctx[+0x276]` | - |
/// | 2 | the stream poll | spawns [`VIGURO_OPEN_SPAWNS`] |
/// | 3 | - | the rendezvous: the body parks until the stager's arm `0` steps the phase |
/// | 4 | hold while the creature's `+0x38` is at or past [`VIGURO_ARRIVAL_Z`] | the spell caption; pitch `-0x1C0`, yaw `0x800 - f`, TR `(0, 0x500, 0x300)`, focus the creature, over `0x14` frames; `ctx[+0x6D8] = scalar * 180` |
/// | 5 | drain, hold while non-negative | the attack caption; snap: yaw `0x800 - f`, TR `(0, 0x500, 0x400)`, focus the creature |
/// | 6 | - | yaw `0x680 - f`, TR `(0, 0x500, 0xC00)`, focus the creature, over `0x180` frames; `ctx[+0x6D8] = scalar << 7` |
/// | 7 | drain, hold while non-negative | spawns [`VIGURO_CREATURE_SPAWNS`]; `ctx[+0x6D8] = scalar * 60` |
/// | 8 | - | the rendezvous: the body parks until the stager's arm `1` steps the phase |
/// | 9 | drain, hold while non-negative | snap: pitch `0x100`, yaw `-f`, TR z `0x800`, focus the enemy row (`FUN_801DCEAC(9)`); `ctx[+0x6D8] = scalar * 60` |
/// | 10 | - | the same framing pulled to TR z `0x2000` over `0x13C` frames |
/// | 11 | drift yaw `+P/2`; drain, hold while non-negative | `ctx[+0x6D8] = 0` |
/// | 12 | drift yaw `+P/2`; the word **climbs** by `P/4`, hold while below [`VIGURO_CHARGE_TOP`] | - |
/// | 13 | drift yaw `+P/4` | the sweep (the body's); pitch `0xC0`, the live yaw, TR z `0x1800`, focus the enemy row, over `0x100` frames |
/// | 14 | drift yaw `+P/4` | the settle (the body's) |
///
/// Arm 12's sixteen-ray ring (`FUN_801E1D98` per pass) and arm 11's three
/// spawns on the camera's own focus are not ported. Arm 13's shot keeps the
/// live yaw the arms have been drifting, which the host feeds in
/// ([`ModuleCamState::live_yaw`]).
///
/// PORT: FUN_801F69F4 (PROT 0909; the camera arms 0/4/5/6/9/10/13, the countdown gates of arms 5/7/9/11/12, arm 4's arrival gate, the yaw drift of arms 11..14 and the spawn calls of arms 2/7)
pub fn viguro_direct(st: &mut ModuleCamState, phase: u8, _: ModuleCamSeats) -> ArmDirection {
    let p = MODULE_DRAIN_PER_TICK;
    let creature = st.creature_live.or(st.creature).unwrap_or_default();
    let yaw = |k: i32| ((k - i32::from(creature.facing)) & 0xFFF) as i16;
    let drift = |yaw: i32| CaptureDrift {
        yaw: yaw as i16,
        ..CaptureDrift::default()
    };
    match phase {
        0 => ArmDirection::shot(ModuleShot {
            angles: [0; 3],
            tr: [0, 0x400, 0],
            focus: [0; 3],
            frames: 1,
        }),
        2 => ArmDirection {
            spawns: &VIGURO_OPEN_SPAWNS,
            ..ArmDirection::PASS
        },
        4 => {
            // No creature seated (a headless host) has nothing to wait on.
            if st
                .creature_live
                .is_some_and(|c| i32::from(c.z) >= VIGURO_ARRIVAL_Z)
            {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = SPEED_SCALAR * 180;
            ArmDirection {
                caption: Some(ModuleCaption::SpellName),
                ..ArmDirection::shot(ModuleShot {
                    angles: [-0x1C0, yaw(0x800), 0],
                    tr: [0, 0x500, 0x300],
                    focus: focus_on(creature),
                    frames: 0x14,
                })
            }
        }
        5 => {
            if viguro_drain_holds(st) {
                return ArmDirection::HOLD;
            }
            ArmDirection {
                caption: Some(ModuleCaption::AttackName),
                ..ArmDirection::shot(ModuleShot {
                    angles: [0, yaw(0x800), 0],
                    tr: [0, 0x500, 0x400],
                    focus: focus_on(creature),
                    frames: 1,
                })
            }
        }
        6 => {
            st.countdown.0 = SPEED_SCALAR << 7;
            ArmDirection::shot(ModuleShot {
                angles: [0, yaw(0x680), 0],
                tr: [0, 0x500, 0xC00],
                focus: focus_on(creature),
                frames: 0x180,
            })
        }
        7 => {
            if viguro_drain_holds(st) {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = SPEED_SCALAR * 60;
            ArmDirection {
                spawns: &VIGURO_CREATURE_SPAWNS,
                ..ArmDirection::PASS
            }
        }
        9 => {
            if viguro_drain_holds(st) {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = SPEED_SCALAR * 60;
            ArmDirection::shot(ModuleShot {
                angles: [0x100, yaw(0), 0],
                tr: [0, 0, 0x800],
                focus: row_focus(st),
                frames: 1,
            })
        }
        10 => ArmDirection::shot(ModuleShot {
            angles: [0x100, yaw(0), 0],
            tr: [0, 0, 0x2000],
            focus: row_focus(st),
            frames: 0x13C,
        }),
        11 => {
            let d = drift(p / 2);
            if viguro_drain_holds(st) {
                return ArmDirection::HOLD.drifted(d);
            }
            st.countdown.0 = 0;
            ArmDirection::PASS.drifted(d)
        }
        12 => {
            st.countdown.0 += p / 4;
            st.countdown.0 = i32::from(st.countdown.0 as i16);
            gate(st.countdown.0 < VIGURO_CHARGE_TOP).drifted(drift(p / 2))
        }
        // `FUN_801DCEAC(9)` again after the sweep, then pitch `0xC0`, the
        // live yaw the arm has just added to, TR z `0x1800` on the row over
        // `0x100` frames (`0x801F7748..0x801F7788`).
        13 => {
            let shot = st.live_yaw.map(|y| ModuleShot {
                angles: [0xC0, ((i32::from(y) + p / 4) & 0xFFF) as i16, 0],
                tr: [0, 0, 0x1800],
                focus: row_focus(st),
                frames: 0x100,
            });
            ArmDirection {
                shot,
                ..ArmDirection::PASS
            }
            .drifted(drift(p / 4))
        }
        14 => ArmDirection::PASS.drifted(drift(p / 4)),
        _ => ArmDirection::PASS,
    }
}
