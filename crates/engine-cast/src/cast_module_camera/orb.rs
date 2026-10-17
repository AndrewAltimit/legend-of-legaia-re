//! PROT 0911 (Orb)'s director, beside its ported tick body
//! (`crate::seru_magic_ticks::orb_tick`).

use super::body_paced::*;
use super::*;

/// PROT 0911 arm 2's four spawns on its pass (`jal 0x80021B04` at
/// `0x801F6C70` / `6C88` / `6CA0` / `6CB8`), seated `0x3E8` over the
/// caster: the three white glow discs and the six-point star of the
/// opening flash. The retail capture of the cast holds the three discs at
/// `(caster x, -1000, caster z)`.
pub const ORB_FLASH_SPAWNS: [ModuleSpawn; 4] = [
    orb_spawn(0x801F_804C, SpawnAnchor::Caster { y: -0x3E8 }),
    orb_spawn(0x801F_8098, SpawnAnchor::Caster { y: -0x3E8 }),
    orb_spawn(0x801F_80E4, SpawnAnchor::Caster { y: -0x3E8 }),
    orb_spawn(0x801F_8130, SpawnAnchor::Caster { y: -0x3E8 }),
];

/// PROT 0911 arm 5's four spawns on its pass (`jal 0x80050ED4` at
/// `0x801F7704` / `771C` / `7734` / `774C`), on the creature `0x140`
/// above its seat: the star, the ray fan, the light column and the record
/// whose op-`0x20` loop calls stager arm `0` eleven times.
pub const ORB_RAY_SPAWNS: [ModuleSpawn; 4] = [
    orb_spawn(0x801F_81DC, SpawnAnchor::CreatureRaised { dy: -0x140 }),
    orb_spawn(0x801F_823C, SpawnAnchor::CreatureRaised { dy: -0x140 }),
    orb_spawn(0x801F_8288, SpawnAnchor::CreatureRaised { dy: -0x140 }),
    orb_spawn(0x801F_82D4, SpawnAnchor::CreatureRaised { dy: -0x140 }),
];

/// PROT 0911 arm 9's spawns on its pass: four on the healed group's centre,
/// turned to the creature's facing plus `0x800` (`jal 0x80050ED4` at
/// `0x801F7994..0x801F79DC`), then the light ring `0x801F84E4` under each
/// live party member (`0x801F7A40`, the loop at `0x801F7A1C`).
///
/// The four group calls pass the group's extent as the scale
/// (`lh a3,0x2C(sp)`, [`spawn_anchor_scale`]); the rings go at `0x1000`.
pub const ORB_HEAL_SPAWNS: [ModuleSpawn; 7] = [
    orb_spawn(
        0x801F_8380,
        SpawnAnchor::ShotPointFacing { y: 0, yaw: 0x800 },
    ),
    orb_spawn(
        0x801F_83E4,
        SpawnAnchor::ShotPointFacing { y: 0, yaw: 0x800 },
    ),
    orb_spawn(
        0x801F_8430,
        SpawnAnchor::ShotPointFacing { y: 0, yaw: 0x800 },
    ),
    orb_spawn(
        0x801F_847C,
        SpawnAnchor::ShotPointFacing { y: 0, yaw: 0x800 },
    ),
    orb_spawn(0x801F_84E4, SpawnAnchor::PartySeat(0)),
    orb_spawn(0x801F_84E4, SpawnAnchor::PartySeat(1)),
    orb_spawn(0x801F_84E4, SpawnAnchor::PartySeat(2)),
];

const fn orb_spawn(va: u32, anchor: SpawnAnchor) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(va),
        anchor,
    }
}

/// PROT 0911 (Orb). `c` is the creature seat `actor_table[7]` (`s4`), `f` its
/// heading, `P` the per-tick frame product, and `g` the cast's target group as
/// `FUN_801DCEAC(caster[+0x1DD], sp+0x30, sp+0x28)` measures it - the focus
/// trio its negated centroid, TR z its extent ([`ModuleCamState::target_aim`]).
///
/// | arm | when | camera |
/// |---:|---|---|
/// | 0 | its pass | cut: pitch `-0x140`, yaw `0x800 - caster[+0x46]`, TR `(0, 0xC00, 0xC00)`, focus the caster (`0x801F6B74`) |
/// | 3 | its pass, once the creature is seated | pitch `0x40`, yaw `0x800 - f`, TR `(0, 0x300, 0x800)`, focus `c`, over `0x80` frames (`0x801F7130`) |
/// | 4 | its pass | the spell name (`FUN_8003541C` at `0x801F74D0`) |
/// | 5 | its pass, `c` stepped forward | the attack name (`0x801F7568`); cut: yaw `0x800 - f`, TR `(0, 0x1400, 0x400)`, focus `c` (`0x801F7634`) |
/// | 9 | every pass | TR z global `+= 2P` (`0x801F77A4..0x801F77C4`) |
/// | 9 | its pass | cut: pitch `0x80`, yaw `0x600 - f`, TR `(0, 0x300, extent)`, focus `g` (`0x801F7890`) |
/// | 10 | every pass | yaw global `+= 4 * delta`, then pitch `0x80`, the live yaw, TR `(0, 0x300, 6 * extent)`, focus `g`, over `0x80` frames (`0x801F7EF4..0x801F7F6C`) |
///
/// Arm 10 re-arms its shot every pass, so the tween's increments are rebuilt
/// from wherever the camera stands each frame: a slow pull-back while the
/// camera circles the healed party.
///
/// PORT: FUN_801F69D8 (PROT 0911; camera arms 0/3/5/9/10, the arm 9 and 10 drift, the arm 4/5 captions, arm 9's caption clear and the spawn calls of arms 2/5/9)
pub fn orb_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let caster = seats.caster;
    run_body_paced(
        st,
        phase,
        BodyPacedArms {
            open: |_| {
                shot(
                    [-0x140, yaw_from(0x800, caster.facing)],
                    [0, 0xC00, 0xC00],
                    focus_on(caster),
                    1,
                )
            },
            pass: |st, arm| {
                let c = st.creature_live.or(st.creature).unwrap_or_default();
                match arm {
                    2 => ArmDirection {
                        spawns: &ORB_FLASH_SPAWNS,
                        ..ArmDirection::PASS
                    },
                    3 => shot(
                        [0x40, yaw_from(0x800, c.facing)],
                        [0, 0x300, 0x800],
                        focus_on(c),
                        0x80,
                    ),
                    4 => captioned(ArmDirection::PASS, ModuleCaption::SpellName),
                    5 => ArmDirection {
                        spawns: &ORB_RAY_SPAWNS,
                        ..captioned(
                            shot(
                                [0, yaw_from(0x800, c.facing)],
                                [0, 0x1400, 0x400],
                                focus_on(c),
                                1,
                            ),
                            ModuleCaption::AttackName,
                        )
                    },
                    // The heal arm's pass ends its line: `FUN_800319A8(0)`
                    // after the per-member `FUN_801D8DE8` loop
                    // (`0x801F7ED0`).
                    9 => {
                        let (focus, extent) = group_aim(st);
                        ArmDirection {
                            spawns: &ORB_HEAL_SPAWNS,
                            ..captioned(
                                shot(
                                    [0x80, yaw_from(0x600, c.facing)],
                                    [0, 0x300, extent],
                                    focus,
                                    1,
                                ),
                                ModuleCaption::Cleared,
                            )
                        }
                    }
                    _ => ArmDirection::PASS,
                }
            },
            each: |st, arm, ticks| match arm {
                9 => drift(0, 0, 0, 2 * MODULE_DRAIN_PER_TICK),
                10 => {
                    let c = st.creature_live.or(st.creature).unwrap_or_default();
                    let step = ORB_SWING_PER_TICK;
                    // The yaw the arm's add leaves; without a live camera,
                    // arm 9's cut plus the swing so far.
                    let yaw = st
                        .live_yaw
                        .map_or(i32::from(yaw_from(0x600, c.facing)) + step * ticks, |y| {
                            i32::from(y) + step
                        });
                    let (focus, extent) = group_aim(st);
                    ArmDirection {
                        drift: drift(0, step, 0, 0).drift,
                        ..shot(
                            [0x80, (yaw & 0xFFF) as i16],
                            [0, 0x300, extent.wrapping_mul(6)],
                            focus,
                            0x80,
                        )
                    }
                }
                _ => ArmDirection::PASS,
            },
        },
    )
}

/// What PROT 0911's arm 10 adds to the yaw global per engine tick: `4 *
/// delta` a battle frame (`lbu 0x7f(0x1F800314)` / `sll 2`), and a battle
/// frame spans two engine ticks.
pub const ORB_SWING_PER_TICK: i32 = 4;

/// The focus trio and extent `FUN_801DCEAC` writes for the cast's target.
fn group_aim(st: &ModuleCamState) -> ([i16; 3], i16) {
    st.target_aim
        .map_or(([0; 3], 0), |a| ([a.centroid_x, 0, a.centroid_z], a.extent))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Arm 9's pass cuts onto the party's group and arm 10 then swings round
    /// it, pulled out to six times the extent.
    #[test]
    fn orb_frames_the_party_and_swings_round_it() {
        let seats = ModuleCamSeats::default();
        let mut st = ModuleCamState {
            target_aim: Some(legaia_engine_battle_vm::battle_target_group::GroupAim {
                centroid_x: -100,
                centroid_z: 200,
                extent: 0x300,
            }),
            ..Default::default()
        };
        assert!(orb_direct(&mut st, 0, seats).shot.is_some());
        orb_direct(&mut st, 9, seats);
        let cut = orb_direct(&mut st, 10, seats).shot.expect("arm 9's cut");
        assert_eq!(
            (cut.tr[2], cut.frames, cut.focus),
            (0x300, 1, [-100, 0, 200])
        );
        st.live_yaw = Some(0x600);
        let d = orb_direct(&mut st, 10, seats);
        let s = d.shot.expect("arm 10 re-arms");
        assert_eq!((s.angles[1], s.tr[2], s.frames), (0x604, 0x1200, 0x80));
        assert_eq!(d.drift.map(|d| d.yaw), Some(4));
    }
}
