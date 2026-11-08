//! PROT 0919 (Spoon): the holds of [`summon_holds::SPOON`] with the body's
//! camera, captions and spawns.
//!
//! See `ghidra/scripts/funcs/overlay_summon_spoon_0919_801f69d8.txt`.

use super::body_kit::{caption, drift, with_drift};
use super::body_paced::shot;
use super::summon_holds::{SPOON, hold_step};
use super::*;

const fn spoon_spawn(va: u32, anchor: SpawnAnchor) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(va),
        anchor,
    }
}

/// Arm 2's pass: five parts on the creature it seats (`0x801F6E08..`).
const SPOON_SEAT_SPAWNS: [ModuleSpawn; 5] = [
    spoon_spawn(0x801F_866C, SpawnAnchor::Creature),
    spoon_spawn(0x801F_86CC, SpawnAnchor::Creature),
    spoon_spawn(0x801F_874C, SpawnAnchor::Creature),
    spoon_spawn(0x801F_879C, SpawnAnchor::Creature),
    spoon_spawn(0x801F_87EC, SpawnAnchor::Creature),
];

/// One of arm 7's calls on the healed group's centre, the group's extent as
/// the scale ([`spawn_anchor_scale`]).
const fn group_spawn(va: u32, yaw: i16) -> ModuleSpawn {
    spoon_spawn(va, SpawnAnchor::ShotPointFacing { y: 0, yaw })
}

/// Arm 7's pass (`0x801F7E18..0x801F7F30`): the two crossed fans, turned a
/// quarter between their pairs, and the column over the healed group, then the
/// three records under each living party member.
const SPOON_HEAL_SPAWNS: [ModuleSpawn; 20] = [
    group_spawn(0x801F_8974, 0x800),
    group_spawn(0x801F_89C0, 0x800),
    group_spawn(0x801F_8974, 0xC00),
    group_spawn(0x801F_89C0, 0xC00),
    group_spawn(0x801F_8A0C, 0xC00),
    group_spawn(0x801F_8A64, 0xC00),
    group_spawn(0x801F_8ABC, 0xC00),
    group_spawn(0x801F_8B08, 0xC00),
    group_spawn(0x801F_8B60, 0xC00),
    group_spawn(0x801F_8BB8, 0xC00),
    group_spawn(0x801F_8C10, 0xC00),
    spoon_spawn(0x801F_883C, SpawnAnchor::PartySeat(0)),
    spoon_spawn(0x801F_88AC, SpawnAnchor::PartySeat(0)),
    spoon_spawn(0x801F_8918, SpawnAnchor::PartySeat(0)),
    spoon_spawn(0x801F_883C, SpawnAnchor::PartySeat(1)),
    spoon_spawn(0x801F_88AC, SpawnAnchor::PartySeat(1)),
    spoon_spawn(0x801F_8918, SpawnAnchor::PartySeat(1)),
    spoon_spawn(0x801F_883C, SpawnAnchor::PartySeat(2)),
    spoon_spawn(0x801F_88AC, SpawnAnchor::PartySeat(2)),
    spoon_spawn(0x801F_8918, SpawnAnchor::PartySeat(2)),
];

/// The focus trio and extent `FUN_801DCEAC` writes for the cast's target.
fn group_aim(st: &ModuleCamState) -> ([i16; 3], i16) {
    st.target_aim
        .map_or(([0; 3], 0), |a| ([a.centroid_x, 0, a.centroid_z], a.extent))
}

/// Arm 4's pass (`0x801F7098..`): four parts for each of the creature's five
/// eyes, seated on the creature and then moved onto the eye's pose object every
/// pass of arms 5..7 (`World::seat_spoon_eye_parts`).
pub const SPOON_EYE_RECORDS: [u32; 4] = [0x801F_8CC8, 0x801F_8D24, 0x801F_8D80, 0x801F_8DF8];
const SPOON_EYE_SPAWNS: [ModuleSpawn; 20] = {
    let mut out = [spoon_spawn(SPOON_EYE_RECORDS[0], SpawnAnchor::Creature); 20];
    let mut i = 0;
    while i < 20 {
        out[i] = spoon_spawn(SPOON_EYE_RECORDS[i % 4], SpawnAnchor::Creature);
        i += 1;
    }
    out
};

/// PROT 0919's director. `c` is the creature seat, `f` its heading, `P` the
/// frame product a tick.
///
/// | arm | every pass | on its pass |
/// |---:|---|---|
/// | 0 | - | cut: pitch `0x20`, yaw `0x800 - caster[+0x46]`, TR `(0, 0x580, 0x200)`, on the caster (`0x801F6B80`) |
/// | 1, 2 | TR z `+= 2P` while the word runs | arm 2: the creature seated, five parts |
/// | 3 | - | the spell name |
/// | 4 | - | the attack name; cut: pitch `0x80`, yaw `0x800 - f`, TR `(0, 0x4B0, 0x280)`, on `c` (`0x801F707C`) |
/// | 5, 6 | TR z `+= P / 8` | arm 6: pitch `0x80`, yaw `0x800 - f`, TR `(0, 0x5C0, 0x480)`, on `c`, `0x10` frames (`0x801F7874`) |
/// | 7 | - | the caption cleared; cut: pitch `0x80`, yaw `0x800 - f`, TR `(0, 0x200, extent)` on the group (`0x801F7E10`); the heal's parts |
/// | 8 | pitch `0x80`, yaw `(scalar * 0x180 - word + 0x800) - f`, TR `(0, 0x200, 6 * extent)` on the group, `0x100` frames (`0x801F8508`) | - |
///
/// Not carried: arm 2's camera block `0x80084008..0x8008401E`.
///
/// PORT: overlay_summon_spoon_0919_801f69d8 (PROT 0919; the camera arms, captions and the spawns of arms 2 and 7)
pub fn spoon_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let word_ran = st.countdown.0 > 0;
    let hold = hold_step(SPOON, st, phase);
    let c = st.creature_live.or(st.creature).unwrap_or_default();
    let z = |n: i32| Some(drift(0, 0, 0, 0, n));
    let dir = match phase {
        0 => shot(
            [0x20, yaw_from(0x800, seats.caster.facing)],
            [0, 0x580, 0x200],
            focus_on(seats.caster),
            1,
        ),
        1 | 2 => {
            let d = if word_ran {
                z(2 * MODULE_DRAIN_PER_TICK)
            } else {
                None
            };
            ArmDirection {
                drift: d,
                ..ArmDirection::PASS
            }
        }
        3 if !hold => caption(ArmDirection::PASS, ModuleCaption::SpellName),
        // Retail seats these on arm 2's pass, right behind the creature; the
        // engine stands the creature up on arm 3's first tick (`seat_arm`), and
        // the anchor needs it.
        3 if st.wait != (3, 1) => {
            st.wait = (3, 1);
            ArmDirection {
                spawns: &SPOON_SEAT_SPAWNS,
                ..ArmDirection::PASS
            }
        }
        4 if !hold => ArmDirection {
            spawns: &SPOON_EYE_SPAWNS,
            ..caption(
                shot(
                    [0x80, yaw_from(0x800, c.facing)],
                    [0, 0x4B0, 0x280],
                    focus_on(c),
                    1,
                ),
                ModuleCaption::AttackName,
            )
        },
        5 => with_drift(ArmDirection::PASS, z(MODULE_DRAIN_PER_TICK / 8)),
        6 if !hold => with_drift(
            shot(
                [0x80, yaw_from(0x800, c.facing)],
                [0, 0x5C0, 0x480],
                focus_on(c),
                0x10,
            ),
            z(MODULE_DRAIN_PER_TICK / 8),
        ),
        6 => with_drift(ArmDirection::PASS, z(MODULE_DRAIN_PER_TICK / 8)),
        7 if !hold => {
            let (focus, extent) = group_aim(st);
            ArmDirection {
                spawns: &SPOON_HEAL_SPAWNS,
                ..caption(
                    shot(
                        [0x80, yaw_from(0x800, c.facing)],
                        [0, 0x200, extent],
                        focus,
                        1,
                    ),
                    ModuleCaption::Cleared,
                )
            }
        }
        8 => {
            let (focus, extent) = group_aim(st);
            let yaw = (SPEED_SCALAR * 0x180 - st.countdown.0 + 0x800 - i32::from(c.facing)) & 0xFFF;
            shot(
                [0x80, yaw as i16],
                [0, 0x200, extent.wrapping_mul(6)],
                focus,
                0x100,
            )
        }
        _ => ArmDirection::PASS,
    };
    ArmDirection { hold, ..dir }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn party_group() -> legaia_engine_battle_vm::battle_target_group::GroupAim {
        legaia_engine_battle_vm::battle_target_group::GroupAim {
            centroid_x: -100,
            centroid_z: 200,
            extent: 0x400,
        }
    }

    /// The opening frames the caster, and the heal cuts to the group and seats
    /// its parts there.
    #[test]
    fn spoon_opens_on_the_caster_and_heals_on_the_group() {
        let seats = ModuleCamSeats::default();
        let mut st = ModuleCamState::default();
        let open = spoon_direct(&mut st, 0, seats);
        assert!(!open.hold);
        assert!(open.shot.is_some());
        let mut st = ModuleCamState {
            target_aim: Some(party_group()),
            ..Default::default()
        };
        let heal = spoon_direct(&mut st, 7, seats);
        assert!(!heal.hold, "a spent word passes");
        assert_eq!(heal.spawns.len(), SPOON_HEAL_SPAWNS.len());
        assert_eq!(heal.caption, Some(ModuleCaption::Cleared));
    }

    /// Arm 8 re-issues its shot every pass, so the camera circles the party.
    #[test]
    fn spoon_circles_the_party_on_its_last_arm() {
        let seats = ModuleCamSeats::default();
        let mut st = ModuleCamState {
            target_aim: Some(party_group()),
            countdown: ModuleCountdown(0x100, 0),
            ..Default::default()
        };
        let d = spoon_direct(&mut st, 8, seats);
        assert!(d.hold);
        assert!(d.shot.is_some());
    }
}
